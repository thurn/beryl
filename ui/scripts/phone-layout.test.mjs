// A crowded row still shows creature names, and phones outside the standard widths use the
// same layout column, scaled or centered.
import assert from "node:assert/strict";
import { test } from "node:test";
import { creatureHeight, phoneFrame } from "../src/mobile/sizing.ts";

test("creature cards stay at least 56px high when width or height is crowded", () => {
    assert.equal(creatureHeight(100, 28), 56);
    assert.equal(creatureHeight(42, 90), 56);
    assert.equal(creatureHeight(96, 80), 80);
    assert.equal(creatureHeight(150, 150), 128);
});

test("the phone centers at 430px and scales the 375px column below that width", () => {
    assert.deepEqual(phoneFrame(480, 850), {
        width: 430,
        height: 850,
        scale: 1,
    });
    assert.deepEqual(phoneFrame(393, 759), {
        width: 393,
        height: 759,
        scale: 1,
    });
    const narrow = phoneFrame(360, 647);
    assert.equal(narrow.width, 375);
    assert.equal(narrow.scale, 360 / 375);
    assert.equal(narrow.height * narrow.scale, 647);
});
