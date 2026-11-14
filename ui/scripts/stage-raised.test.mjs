// How a hovered hand card eases up out of the fan and back down into it, with no browser
// (`npm test`; ui/src/stage/raised.ts `raisedAt`), against Arena's hand filmed at 30 frames a
// second: up in about a tenth of a second and down in about a sixth, about half the way in
// the first frame of each.
import assert from "node:assert/strict";
import { test } from "node:test";
import { DROP_MS, RISE_MS, raisedAt } from "../src/stage/raised.ts";

const FRAME = 1000 / 30;

test("a hovered card rises fast at first and is all the way up within Arena's rise", () => {
    const rise = { from: 0, to: 1, since: 0 };
    assert.equal(raisedAt(rise, 0), 0);
    const first = raisedAt(rise, FRAME);
    assert.ok(first > 0.5 && first < 0.9, `after one frame ${first}`);
    assert.ok(raisedAt(rise, 2 * FRAME) > 0.9);
    assert.equal(raisedAt(rise, 130), 1);
    assert.ok(RISE_MS >= 70 && RISE_MS <= 130);
});

test("a card the pointer leaves drops back into the fan over Arena's drop, not at once", () => {
    const drop = { from: 1, to: 0, since: 0 };
    const first = raisedAt(drop, FRAME);
    assert.ok(first > 0.3 && first < 0.7, `after one frame ${first}`);
    assert.ok(
        raisedAt(drop, 2 * FRAME) > 0.1,
        "still on its way after two frames",
    );
    assert.equal(raisedAt(drop, 200), 0);
    assert.ok(DROP_MS >= 130 && DROP_MS <= 200);
});

test("a card that turns back partway takes only the share of the way it has left", () => {
    // The pointer comes back onto a card a third of the way up as it drops.
    const back = { from: 1 / 3, to: 1, since: 0 };
    assert.equal(raisedAt(back, (2 / 3) * RISE_MS), 1);
    const half = raisedAt(back, (1 / 3) * RISE_MS);
    assert.ok(half > 1 / 3 && half < 1);
});
