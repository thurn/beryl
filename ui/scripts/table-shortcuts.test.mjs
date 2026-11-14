import assert from "node:assert/strict";
import { beforeEach, test } from "node:test";
import { setProfile } from "../src/profile.ts";
import { Bindings } from "../src/table/bindings.ts";
import { commands } from "../src/table/commands.ts";
import {
    bindingChord,
    bindingFor,
    describe,
    resolve,
} from "../src/table/shortcuts.ts";

beforeEach((t) => {
    const entries = new Map();
    const original = Object.getOwnPropertyDescriptor(
        globalThis,
        "localStorage",
    );
    Object.defineProperty(globalThis, "localStorage", {
        configurable: true,
        value: {
            getItem: (key) => entries.get(key) ?? null,
            setItem: (key, value) => entries.set(key, value),
            removeItem: (key) => entries.delete(key),
        },
    });
    t.after(() => {
        setProfile(null);
        if (original) {
            Object.defineProperty(globalThis, "localStorage", original);
        } else {
            delete globalThis.localStorage;
        }
    });
});

function press(code, modifiers = {}) {
    return {
        code,
        metaKey: false,
        ctrlKey: false,
        altKey: false,
        shiftKey: false,
        isComposing: false,
        ...modifiers,
    };
}

const command = { metaKey: true };
const control = { ctrlKey: true };
const option = { altKey: true };

test("physical Option+U works when the event carries an accent", () => {
    const mac = new Bindings("macos");
    const event = press("KeyU", { ...option, key: "¨" });
    assert.equal(resolve(event, "table", mac), "permanent.skip_untap");
    assert.equal(bindingFor(event, "macos"), "Option+U");
    assert.equal(
        resolve({ ...event, isComposing: true }, "table", mac),
        undefined,
    );
});

test("Cmd on macOS and Ctrl on other platforms require exact modifiers", () => {
    for (const [platform, right, wrong] of [
        ["macos", command, control],
        ["windows", control, command],
        ["linux", control, command],
    ]) {
        const bindings = new Bindings(platform);
        assert.equal(
            resolve(press("KeyW", right), "table", bindings),
            "library.look_top",
        );
        assert.equal(
            resolve(press("KeyW", wrong), "table", bindings),
            undefined,
        );
        assert.equal(
            resolve(press("KeyW", { ...right, ...wrong }), "table", bindings),
            undefined,
        );
    }
});

test("the same shortcut can resolve independently in the table and lobby", () => {
    const mac = new Bindings("macos");
    for (const [event, table, lobby] of [
        [press("KeyN", command), "library.keep_top_revealed", "lobby.host"],
        [
            press("KeyN", { ...command, shiftKey: true }),
            "library.keep_top_known",
            "lobby.game_settings",
        ],
        [
            press("KeyU", { ...command, ...option }),
            "permanent.unattach",
            "lobby.unload_deck",
        ],
    ]) {
        assert.equal(resolve(event, "table", mac), table);
        assert.equal(resolve(event, "lobby", mac), lobby);
        assert.equal(resolve(event, undefined, mac), undefined);
        assert.equal(resolve(event, "engine", mac), undefined);
    }
});

test("registry defaults use physical keys for punctuation and aliases", () => {
    const mac = new Bindings("macos");
    for (const [event, id] of [
        [press("Tab"), "phase.next"],
        [press("Space"), "phase.next"],
        [press("F11"), "life.lose_one"],
        [press("Minus"), "life.lose_one"],
        [press("Equal", command), "stats.power_up"],
        [press("Equal", { ...command, shiftKey: true }), "stats.power_up"],
        [press("Comma", command), "counter.second_remove"],
        [
            press("Comma", { ...command, shiftKey: true }),
            "counter.third_remove",
        ],
        [press("Period", { ...command, shiftKey: true }), "counter.third_add"],
        [press("Slash", { ...command, shiftKey: true }), "counter.third_set"],
        [press("Backspace", command), "move.graveyard"],
    ]) {
        assert.equal(resolve(event, "table", mac), id);
    }
    const windows = new Bindings("windows");
    assert.equal(
        resolve(press("Delete", control), "table", windows),
        "move.graveyard",
    );
});

test("every default and alias resolves to its own command on both platforms", () => {
    for (const [platform, variant] of [
        ["macos", "mac"],
        ["windows", "other"],
    ]) {
        const bindings = new Bindings(platform);
        for (const { id, scope, key, key_aliases } of commands) {
            if (!key) continue;
            for (const binding of [key, ...(key_aliases ?? [])]) {
                const chord = bindingChord(binding[variant], platform);
                assert.ok(chord, binding[variant]);
                assert.equal(
                    bindings.lookup(scope, chord),
                    id,
                    binding[variant],
                );
            }
        }
    }
});

test("rebinding an alias displaces its owner and preserves the other scope", () => {
    const mac = new Bindings("macos");
    assert.equal(mac.bind("table.undo", "Shift+Cmd+="), "stats.power_up");
    assert.equal(
        resolve(press("Equal", { ...command, shiftKey: true }), "table", mac),
        "table.undo",
    );
    assert.equal(resolve(press("Equal", command), "table", mac), undefined);
    assert.equal(mac.get("stats.power_up"), undefined);
    assert.equal(resolve(press("KeyZ", command), "table", mac), undefined);

    assert.equal(mac.bind("table.undo", "Cmd+N"), "library.keep_top_revealed");
    assert.equal(resolve(press("KeyN", command), "table", mac), "table.undo");
    assert.equal(resolve(press("KeyN", command), "lobby", mac), "lobby.host");
    mac.bind("table.undo", null);
    assert.equal(resolve(press("KeyN", command), "table", mac), undefined);
    mac.reset();
    assert.equal(resolve(press("KeyZ", command), "table", mac), "table.undo");
    assert.equal(
        resolve(press("Equal", command), "table", mac),
        "stats.power_up",
    );
});

test("overrides, unbound commands and reset survive reloads per profile", () => {
    setProfile("one");
    const first = new Bindings("macos");
    assert.equal(first.bind("table.undo", "Space"), "phase.next");
    const reloaded = new Bindings("macos");
    assert.equal(reloaded.get("table.undo"), "Space");
    assert.equal(reloaded.get("phase.next"), undefined);
    assert.equal(resolve(press("Tab"), "table", reloaded), undefined);
    assert.equal(resolve(press("Space"), "table", reloaded), "table.undo");

    setProfile("two");
    const other = new Bindings("macos");
    assert.equal(resolve(press("Space"), "table", other), "phase.next");

    setProfile("one");
    reloaded.reset();
    assert.equal(
        resolve(press("Tab"), "table", new Bindings("macos")),
        "phase.next",
    );
});

test("shortcut descriptions use the platform modifiers", () => {
    assert.equal(describe("Cmd+W", "macos"), "⌘W");
    assert.equal(describe("Cmd+W", "windows"), "Ctrl+W");
    assert.equal(describe("Cmd+Shift+Option+W", "macos"), "⌥⇧⌘W");
    assert.equal(describe("Ctrl+Alt+W", "windows"), "Ctrl+Alt+W");
    assert.equal(describe("Cmd++", "macos"), "⌘+");
    assert.equal(describe(null, "linux"), "");
});
