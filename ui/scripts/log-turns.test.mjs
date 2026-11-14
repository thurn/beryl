// The game log numbers turns per player, as players count them: the opponent's first turn
// and the viewer's first turn are both "Turn 1".
import assert from "node:assert/strict";
import { test } from "node:test";
import { logLines } from "../src/game/log.ts";

const view = (number, ownNumber, active) => ({
    turn: { number, ownNumber, active, step: "untap", priority: active },
    objects: [],
    stack: [],
    players: {},
    events: [{ kind: "stepBegins", step: "untap", active }],
});

test("the log counts each player's own turns", () => {
    const first = logLines(view(1, 1, "opponent"), new Map());
    const second = logLines(view(2, 1, "you"), new Map());
    assert.equal(first[0].text, "Turn 1: Opponent's turn");
    assert.equal(second[0].text, "Turn 1: your turn");
    assert.equal(
        logLines(view(3, 2, "opponent"), new Map())[0].text,
        "Turn 2: Opponent's turn",
    );
});
