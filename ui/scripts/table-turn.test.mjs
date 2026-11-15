import assert from "node:assert/strict";
import { test } from "node:test";
import { advancePhase } from "../src/table/turn.ts";

test("Shift+Tab advances an opponent's phase without acting for either seat", () => {
    for (const [from, to] of [
        ["upkeep", "draw"],
        ["end", "untap"],
    ]) {
        const view = {
            table: { phase: from, seats: 2 },
            turn: { active: "opponent" },
        };
        assert.deepEqual(advancePhase(view, true), [{ phase: { phase: to } }]);
    }
});

test("Shift+Tab draws for the active player in seat two", () => {
    const view = {
        table: { phase: "upkeep", seat: 1, seats: 2 },
        turn: { active: "you" },
    };
    assert.deepEqual(advancePhase(view, true), [
        { phase: { phase: "draw" } },
        { draw: { seat: 1, count: 1 } },
    ]);
});
