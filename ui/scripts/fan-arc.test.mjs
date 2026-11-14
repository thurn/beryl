// The hand fans along one circle, as Arena's does: a card's turn follows its distance from the
// fan's middle, so a crowded hand turns and sinks less per card than a spread one instead of
// curling at its ends. The Arena values are read from whole-table frames (fanArc.ts).
import assert from "node:assert/strict";
import { test } from "node:test";
import { onArc } from "../src/game/fanArc.ts";

// Arena's table at 1920 wide is Beryl's stage at 1600, so its 180px cards are 150 wide.
const ARENA_CARD = 150;
const px = (x) => (x * 1600) / 1920;

test("the outer card turns as far as Arena's at the same distance", () => {
    // [cards in hand, the outer card's center from the middle in Arena px, its turn in degrees]
    for (const [n, dx, turn] of [
        [8, 3.5 * 170, 11.5],
        [10, 4.5 * 125, 14],
        [18, 8.5 * 69, 15],
    ]) {
        const got = onArc(px(dx), ARENA_CARD).turn;
        assert.ok(
            Math.abs(got - turn) <= 3,
            `${n} cards: ${got.toFixed(1)}° vs ${turn}°`,
        );
    }
});

test("a crowded hand curls no more than a spread one", () => {
    // Ten 182-wide cards squeezed 60 apart, as the narrowest stage holds them, turn their outer
    // card well under Arena's outermost and sink it under a tenth of a card.
    const outer = onArc(4.5 * 60, 182);
    assert.ok(outer.turn < 8, `turn ${outer.turn}`);
    assert.ok(outer.sink < 18.2, `sink ${outer.sink}`);
    // Each card sinks along the circle the turn says it lies on: no card drops further than
    // the card beside it is turned.
    for (let i = 1; i <= 4; i++) {
        const near = onArc((i - 1) * 60, 182);
        const far = onArc(i * 60, 182);
        const slope = (far.sink - near.sink) / 60;
        assert.ok(slope <= Math.tan((far.turn * Math.PI) / 180) + 1e-9);
    }
});
