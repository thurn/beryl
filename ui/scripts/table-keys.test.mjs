import assert from "node:assert/strict";
import { test } from "node:test";
import {
    isBugReportShortcut,
    setBugReportTableOpen,
} from "../src/components/bugReportKeys.ts";
import { claims } from "../src/table/keys.ts";

function command(code, modifiers = {}) {
    return {
        code,
        metaKey: true,
        ctrlKey: false,
        altKey: false,
        shiftKey: false,
        isComposing: false,
        ...modifiers,
    };
}

test("the table claims exactly the platform collisions", () => {
    for (const code of [
        "KeyW",
        "KeyM",
        "Comma",
        "KeyR",
        "KeyP",
        "Equal",
        "Minus",
        "KeyQ",
    ]) {
        assert.ok(claims(command(code)), code);
    }
    for (const code of ["KeyB", "KeyZ", "Space", "NumpadSubtract", "KeyA"]) {
        assert.ok(!claims(command(code)), code);
    }
});

test("the table ignores extra modifiers and composing key events", () => {
    for (const modifiers of [
        { metaKey: false },
        { ctrlKey: true },
        { shiftKey: true },
        { altKey: true },
        { isComposing: true },
    ]) {
        assert.ok(
            !claims(command("KeyW", modifiers)),
            JSON.stringify(modifiers),
        );
    }
    const control = command("KeyR", { metaKey: false, ctrlKey: true });
    assert.ok(!claims(control, "macos"));
    assert.ok(claims(control, "windows"));
});

test("the bug report gives up its unshifted shortcut only at a table", (t) => {
    t.after(() => setBugReportTableOpen(false));

    for (const modifier of [{}, { metaKey: false, ctrlKey: true }]) {
        const plain = { ...command("KeyB", modifier), key: "b" };
        const shifted = { ...plain, key: "B", shiftKey: true };

        assert.ok(isBugReportShortcut(plain));
        assert.ok(!isBugReportShortcut(shifted));
        setBugReportTableOpen(true);
        assert.ok(!isBugReportShortcut(plain));
        assert.ok(isBugReportShortcut(shifted));
        assert.ok(isBugReportShortcut({ ...shifted, key: "X" }));
        assert.ok(!isBugReportShortcut({ ...shifted, code: "KeyX" }));
        assert.ok(!isBugReportShortcut({ ...shifted, altKey: true }));
        setBugReportTableOpen(false);
        assert.ok(isBugReportShortcut(plain));
    }
});
