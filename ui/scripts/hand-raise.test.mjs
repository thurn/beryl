// How large a hovered hand card grows and where it stands, with no browser (`npm test`;
// ui/src/game/placement.ts `raisedPose`), against Arena's raised hand card filmed whole: about
// 0.3 of the table's height wide, its top 0.58 of the way down and its foot on the bottom edge.
import assert from "node:assert/strict";
import { test } from "node:test";
import { RAISE, raisedPose } from "../src/game/raise.ts";

// A card at rest in the middle of the fan, as placement.ts `HAND` lays it (that module needs
// a browser).
const HAND = { width: 182, top: 726.5 };

const HEIGHT = 900;
const TALL = (HAND.width * 88) / 63;
const middle = {
    x: 800 - HAND.width / 2,
    y: HAND.top,
    width: HAND.width,
    rotation: 0,
};

/** Where the card at `pose` raised by `raised` stands: its top, its foot and its width. */
const stands = (pose, { scale, rise }) => ({
    top: pose.y - rise,
    foot: pose.y - rise + scale * TALL,
    width: scale * pose.width,
});

test("a hovered hand card grows to Arena's size and stands on the bottom edge", () => {
    const raised = raisedPose(middle, HEIGHT);
    const { top, foot, width } = stands(middle, raised);
    assert.ok(Math.abs(foot - (HEIGHT - RAISE.floor)) < 0.5, `foot at ${foot}`);
    assert.ok(width / HEIGHT > 0.28 && width / HEIGHT < 0.32, `${width} wide`);
    assert.ok(top / HEIGHT > 0.55 && top / HEIGHT < 0.61, `top at ${top}`);
});

test("it grows less where it would cover a card a prompt asks the player to pick", () => {
    const target = { left: 750, right: 850, top: 400, bottom: 560 };
    const raised = raisedPose(middle, HEIGHT, [target]);
    assert.ok(raised.scale < RAISE.scale && raised.scale >= 1);
    assert.ok(
        stands(middle, raised).top >= target.bottom,
        "clear of the target",
    );
    // A target off to the side leaves it whole.
    const aside = { ...target, left: 1300, right: 1400 };
    assert.equal(raisedPose(middle, HEIGHT, [aside]).scale, RAISE.scale);
});

test("with no room to grow, it rises only as far as keeps it clear", () => {
    const low = { left: 700, right: 900, top: 600, bottom: 700 };
    const raised = raisedPose(middle, HEIGHT, [low]);
    assert.equal(raised.scale, 1);
    assert.ok(stands(middle, raised).top >= low.bottom, "clear of the target");
});
