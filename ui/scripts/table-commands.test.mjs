import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { commands, commandsFor } from "../src/table/commands.ts";

const appendix = new URL(
    "../../docs/manual-multiplayer-cockatrice.md",
    import.meta.url,
);

function keysFor(command) {
    return command.key ? [command.key, ...(command.key_aliases ?? [])] : [];
}

function defaults(scope, platform) {
    const bound = new Map();
    for (const command of commands) {
        if (command.scope !== scope) continue;
        for (const pair of keysFor(command)) {
            const key = pair[platform];
            assert.ok(
                !bound.has(key),
                `${scope} ${key}: ${bound.get(key)} and ${command.id}`,
            );
            bound.set(key, command.id);
        }
    }
    return bound;
}

function readAppendixKeys() {
    let section;
    let keyColumn = -1;
    const rows = [];
    for (const line of readFileSync(appendix, "utf8").split(/\r?\n/u)) {
        if (line.startsWith("## ")) {
            section = line.slice(3);
            keyColumn = -1;
        }
        if (!line.startsWith("|")) continue;

        const cells = line
            .split("|")
            .slice(1, -1)
            .map((cell) => cell.trim());
        if (cells.at(-1) === "In Beryl") {
            keyColumn = cells.indexOf("Key");
            continue;
        }
        const key = cells[keyColumn];
        if (
            keyColumn < 0 ||
            !key ||
            /^[—-]+$/u.test(key) ||
            key === "as above" ||
            cells.at(-1).startsWith("Not carried over")
        ) {
            continue;
        }

        if (key === "Option+1 to Option+0") {
            for (const digit of "1234567890") {
                rows.push(["table", `Option+${digit}`]);
            }
            continue;
        }
        const lobby =
            section === "Setup and match" &&
            !["Concede / Unconcede", "Leave game"].includes(cells[0]);
        for (const binding of key
            .replaceAll(/\\(?=[\[\]\\])/gu, "")
            .split(/ \/ |,? or /u)) {
            if (binding !== "Cmd+Space" && binding !== "—") {
                rows.push([lobby ? "lobby" : "table", binding]);
            }
        }
    }
    return rows;
}

test("registry ids and default keys are unique within their scope", () => {
    assert.equal(new Set(commands.map(({ id }) => id)).size, commands.length);
    for (const scope of ["table", "lobby"]) {
        for (const platform of ["mac", "other"]) defaults(scope, platform);
    }
    for (const command of commands) {
        assert.doesNotMatch(command.label, /\b(chat|spectators?|judges?)\b/iu);
        for (const { mac, other } of keysFor(command)) {
            assert.equal(
                other,
                mac.replaceAll("Cmd", "Ctrl").replaceAll("Option", "Alt"),
            );
        }
    }
});

test("default keys cover the appendix in the intended scope", () => {
    const table = defaults("table", "mac");
    const lobby = defaults("lobby", "mac");
    for (const [scope, key] of readAppendixKeys()) {
        assert.ok(
            (scope === "lobby" ? lobby : table).has(key),
            `${scope} ${key}`,
        );
    }
    assert.equal(table.has("Cmd+Space"), false);
    assert.equal(table.get("Tab"), "phase.next");
    assert.equal(table.get("Space"), "phase.next");
    assert.equal(table.get("Cmd+Z"), "table.undo");
    assert.equal(table.get("Cmd+B"), "move.library_bottom");
    assert.equal(table.get("Cmd+Shift+B"), "table.bug_report");
    assert.equal(lobby.get("Cmd+Shift+B"), "lobby.sideboard");
    assert.equal(table.get("Escape"), "viewer.close");
    assert.equal(table.get("Cmd+K"), "table.command_list");
});

function firstRows(target, count) {
    return commandsFor(target, { scope: "table" })
        .filter(({ menu }) => menu && !menu.submenu)
        .sort((left, right) => left.menu.order - right.menu.order)
        .slice(0, count)
        .map(({ label }) => label);
}

test("each menu starts with the actions specified for its target", () => {
    const moves = [
        "Graveyard",
        "Exile",
        "Hand",
        "Top of library",
        "Bottom of library",
    ];
    assert.deepEqual(firstRows("permanent", 6), ["Tap", ...moves]);
    assert.deepEqual(firstRows("their_permanent", 6), [
        ...moves,
        "Take control",
    ]);
    assert.deepEqual(firstRows("hand_card", 4), [
        "Play",
        "Play face down",
        "Reveal",
        "Graveyard",
    ]);
    assert.deepEqual(firstRows("stack_entry", 2), ["Resolve", "Graveyard"]);
    assert.deepEqual(firstRows("pile_card", 3), [
        "Play",
        "Battlefield",
        "Hand",
    ]);
    assert.deepEqual(firstRows("library", 5), [
        "Draw",
        "Draw cards…",
        "Look at top cards…",
        "Search",
        "Shuffle",
    ]);
    assert.deepEqual(firstRows("life", 5), [
        "−1",
        "+1",
        "−5",
        "+5",
        "Set life…",
    ]);
});

test("target, scope, enabled assists, and conditions filter commands", () => {
    const ids = (target, context) =>
        commandsFor(target, context).map(({ id }) => id);
    const scope = "table";
    assert.deepEqual(ids("permanent", { scope: "lobby" }), []);
    assert.ok(ids("lobby", { scope: "lobby" }).includes("lobby.change_deck"));
    assert.ok(!ids("permanent", { scope }).includes("card.transform"));
    assert.ok(
        ids("stack_entry", {
            scope,
            conditions: new Set(["two_faced"]),
        }).includes("card.transform"),
    );
    assert.ok(!ids("library", { scope }).includes("library.scry"));
    assert.ok(
        ids("library", { scope, assists: new Set(["library_flows"]) }).includes(
            "library.scry",
        ),
    );
    assert.equal(
        commandsFor("pile_card", { scope }).find(
            ({ id }) => id === "hand_card.play_face_down",
        ).menu,
        undefined,
    );
});
