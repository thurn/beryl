// The pointer on the stage, with no browser (`npm test`): what the stage tells of hover, click
// and drag (ui/src/stage/pointer.ts), and the held card's pose, the one the pointer drives. A
// test makes the calls the page's surface makes (dom.ts), in stage pixels.
import assert from "node:assert/strict";
import { test } from "node:test";
import { ALOFT } from "../src/stage/pose.ts";
import { heldPose } from "../src/stage/pointer.ts";
import { move, object, stage, view } from "./stage-table.mjs";

/** A stage showing permanents 1 and 2 and the hand's card 3, and everything it tells of the
 * pointer, in order. `taken` is what its listener says of a drop. */
function listening({ holdable = () => true, taken = false, timings } = {}) {
    const { stage: made, surface } = stage(timings);
    made.play(view([1, 2], [3]));
    const told = [];
    made.pointer.holdable = holdable;
    made.pointer.listen({
        hover: (name) => told.push(["hover", name]),
        click: (name, at) => told.push(["click", name, at]),
        lift: (name) => told.push(["lift", name]),
        drop: (drop) => (told.push(["drop", drop]), taken),
    });
    return { made, surface, told };
}

const LINEAR = { duration: 200, delay: 0, stagger: 0, ease: (p) => p };

test("hover is told as the pointer comes onto a node and leaves it", () => {
    const { made, told } = listening();
    made.pointer.over(1);
    made.pointer.over(1);
    made.pointer.over(2);
    made.pointer.over(undefined);
    assert.deepEqual(told, [
        ["hover", 1],
        ["hover", 2],
        ["hover", undefined],
    ]);
    assert.equal(made.pointer.hovered, undefined);
});

test("a press let go where it began is a click; one that wandered is nothing", () => {
    const { made, told } = listening({ holdable: () => false });
    made.pointer.press(2, { x: 410, y: 20 });
    made.pointer.move({ x: 412, y: 21 });
    made.pointer.release({ x: 412, y: 21 });
    assert.deepEqual(told, [["click", 2, { x: 412, y: 21 }]]);
    // A card that may not be picked up stays where it rests however far the press goes.
    made.pointer.press(2, { x: 410, y: 20 });
    made.pointer.move({ x: 500, y: 300 });
    assert.equal(made.pointer.held, undefined);
    assert.deepEqual(made.pose(2), { ...made.pose(2), x: 400, y: 0 });
    made.pointer.release({ x: 500, y: 300 });
    assert.equal(told.length, 1);
});

test("a held card's pose is the pointer's, and the stage's model returns it", () => {
    const { made, surface, told } = listening({
        holdable: (name) => name === 3,
    });
    made.release = LINEAR;
    made.pointer.over(3);
    // The hand's card rests at 0, 500, 100 wide; the press is 30, 40 inside it.
    made.pointer.press(3, { x: 30, y: 540 });
    made.pointer.move({ x: 33, y: 540 });
    assert.equal(made.pointer.held, undefined, "under the threshold");
    made.pointer.move({ x: 230, y: 340 });
    assert.deepEqual(made.pointer.held, {
        name: 3,
        start: { x: 30, y: 540 },
        at: { x: 230, y: 340 },
    });
    const held = made.pose(3);
    assert.deepEqual(
        [held.x, held.y, held.width, held.rotation],
        [200, 300, 100, 0],
    );
    assert.ok(held.depth > 900, "over what rests and what flies");
    assert.deepEqual(surface.nodes.get(3), held, "drawn as it moves");
    // The clock does not move it.
    made.clock.advance(5000);
    assert.deepEqual(made.pose(3), held);
    // Nothing is hovered while a card is held.
    made.pointer.over(1);
    made.pointer.move({ x: 430, y: 60 });
    made.pointer.release({ x: 430, y: 60 }, { name: 1 });
    assert.deepEqual(told, [
        ["hover", 3],
        ["hover", undefined],
        ["lift", 3],
        ["lift", undefined],
        [
            "drop",
            {
                name: 3,
                start: { x: 30, y: 540 },
                at: { x: 430, y: 60 },
                pose: { ...held, x: 400, y: 20 },
                onto: { name: 1 },
            },
        ],
        ["hover", 1],
    ]);
    // A drop nobody took: the card glides home on the stage's clock.
    assert.equal(made.pointer.held, undefined);
    assert.deepEqual([made.pose(3).x, made.pose(3).y], [400, 20]);
    made.clock.advance(100);
    assert.deepEqual([made.pose(3).x, made.pose(3).y], [200, 260]);
    made.clock.advance(100);
    assert.deepEqual(made.pose(3), {
        ...held,
        x: 0,
        y: 500,
        depth: 3,
    });
});

test("a card let go over itself is not hovered until it is home", () => {
    // Hovered on its way, a hand card would rise out of the fan as it rests there, upright
    // and lifted, while it is still elsewhere, and drop back once the pointer left it: a kick.
    const { made, told } = listening({ holdable: (name) => name === 3 });
    made.release = LINEAR;
    made.pointer.over(3);
    made.pointer.press(3, { x: 30, y: 540 });
    made.pointer.move({ x: 230, y: 340 });
    made.pointer.release({ x: 230, y: 340 });
    assert.equal(told.at(-1)[0], "drop");
    assert.equal(made.pointer.hovered, undefined);
    made.clock.advance(100);
    assert.equal(made.pointer.hovered, undefined, "still on its way");
    // Home, the stage asks its surface what the pointer is on, which says it is the card.
    let asked = 0;
    made.subscribe(() => (asked += 1));
    made.clock.advance(100);
    assert.ok(asked > 0, "those who draw the stage are told");
    assert.equal(made.pointer.hovered, undefined);
    made.pointer.over(3);
    assert.deepEqual(told.at(-1), ["hover", 3], "home, it is hovered");
});

test("a card flying into the hand is not hovered until it lands", () => {
    // A card you draw is in your hand while it flies there. Hovered on its way, it would
    // rise out of the fan as it will rest there, turned back from the fan's lean, while it
    // flies upright beside the stack: it jumped to a tilt the other way.
    const { made, told } = listening({ timings: { fly: LINEAR } });
    made.play(view([2], [3, 1]), {
        beats: [[{ act: { ...move(1).act, object: object(1, "hand") } }]],
    });
    made.clock.advance(100);
    assert.ok(made.pose(1).depth >= ALOFT, "it flies");
    made.pointer.over(1);
    assert.equal(made.pointer.hovered, undefined, "on its way");
    assert.equal(told.length, 0);
    let asked = 0;
    made.subscribe(() => (asked += 1));
    made.clock.advance(100);
    assert.ok(asked > 0, "those who draw the stage are told it landed");
    made.pointer.over(1);
    assert.deepEqual(told.at(-1), ["hover", 1], "in the hand, it is hovered");
});

test("a card let go with nothing to travel may be hovered at once", () => {
    const { made, told } = listening({ holdable: (name) => name === 3 });
    made.pointer.over(3);
    made.pointer.press(3, { x: 30, y: 540 });
    made.pointer.move({ x: 230, y: 340 });
    made.pointer.release({ x: 230, y: 340 });
    assert.equal(made.pointer.hovered, undefined);
    made.pointer.over(3);
    assert.deepEqual(told.at(-1), ["hover", 3]);
});

test("a taken drop is not hovered while it waits, nor on its way home", () => {
    const { made, told } = listening({ taken: true, timings: { fly: LINEAR } });
    made.release = LINEAR;
    made.pointer.over(3);
    made.pointer.press(3, { x: 0, y: 500 });
    made.pointer.move({ x: 1000, y: 100 });
    made.pointer.release({ x: 1000, y: 100 });
    made.clock.advance(5000);
    assert.equal(
        made.pointer.hovered,
        undefined,
        "waiting where it was let go",
    );
    // The play did nothing: the card goes home when the script is over.
    made.play(view([2, 1], [3]), { beats: [[move(2, 0)]] });
    made.clock.advance(300);
    made.pointer.over(undefined);
    made.pointer.over(3);
    assert.equal(made.pointer.hovered, undefined, "on its way home");
    made.clock.advance(100);
    assert.equal(made.pointer.settling, undefined, "home");
    made.pointer.over(undefined);
    made.pointer.over(3);
    assert.deepEqual(told.at(-1), ["hover", 3]);
});

test("a taken drop stays where it was let go, and its step starts from there", () => {
    const { made } = listening({ taken: true, timings: { fly: LINEAR } });
    made.release = LINEAR;
    made.pointer.press(3, { x: 0, y: 500 });
    made.pointer.move({ x: 1000, y: 100 });
    made.pointer.release({ x: 1000, y: 100 });
    made.clock.advance(5000);
    assert.deepEqual([made.pose(3).x, made.pose(3).y], [1000, 100]);
    // The play it made: the card enters the battlefield, third in its row (800, 0).
    made.play(view([1, 2, 3]), {
        beats: [
            [
                {
                    act: {
                        verb: "fly",
                        from: "hand",
                        why: "effect",
                        object: object(3),
                    },
                },
            ],
        ],
    });
    made.clock.advance(100);
    assert.deepEqual([made.pose(3).x, made.pose(3).y], [900, 50]);
    made.settle();
    assert.deepEqual([made.pose(3).x, made.pose(3).y], [800, 0]);
});

test("a taken drop that no step moves goes home when its script is over", () => {
    const { made } = listening({ taken: true, timings: { fly: LINEAR } });
    made.release = LINEAR;
    made.pointer.press(3, { x: 0, y: 500 });
    made.pointer.move({ x: 1000, y: 100 });
    made.pointer.release({ x: 1000, y: 100 });
    made.play(view([2, 1], [3]), { beats: [[move(2, 0)]] });
    made.clock.advance(200);
    assert.deepEqual([made.pose(3).x, made.pose(3).y], [1000, 100]);
    made.clock.advance(100);
    assert.deepEqual([made.pose(3).x, made.pose(3).y], [500, 300]);
    made.clock.advance(100);
    assert.deepEqual([made.pose(3).x, made.pose(3).y], [0, 500]);
});

test("a held card turned where it rests is held by the point that was pressed", () => {
    const rest = {
        x: 100,
        y: 100,
        width: 100,
        rotation: 90,
        skew: 5,
        turn: 0,
        opacity: 1,
        depth: 7,
        face: "front",
    };
    // Turned a quarter about its corner, the point 20 along its top edge lies 20 below the
    // corner. Upright, the corner is 20 left of the pointer.
    const held = heldPose(rest, { x: 100, y: 120 }, { x: 500, y: 300 });
    assert.deepEqual(
        [Math.round(held.x), Math.round(held.y), held.rotation, held.skew],
        [480, 300, 0, 0],
    );
});

test("a held card that leaves the table is let go untold of a drop", () => {
    const { made, told } = listening();
    made.pointer.over(3);
    made.pointer.press(3, { x: 0, y: 500 });
    made.pointer.move({ x: 300, y: 300 });
    // Discarded while held: this layout rests nothing in a graveyard.
    made.play(view([1, 2]), {
        beats: [
            [
                {
                    act: {
                        verb: "fly",
                        from: "hand",
                        why: "effect",
                        object: object(3, "graveyard"),
                    },
                },
            ],
        ],
    });
    made.settle();
    assert.equal(made.pose(3), null);
    assert.equal(made.pointer.held, undefined);
    assert.deepEqual(told.slice(2), [
        ["lift", 3],
        ["lift", undefined],
    ]);
    made.pointer.release({ x: 300, y: 300 });
    assert.equal(told.length, 4);
});

test("an announcement is shown for as long as its step plays", () => {
    const { stage: made } = stage({ announce: { duration: 1200 } });
    made.play(view([1]));
    const step = {
        act: { verb: "announce", notice: { kind: "turn", active: "you" } },
    };
    made.play(view([1, 2]), { beats: [[step, move(1, 0)]] });
    made.clock.advance(0);
    assert.deepEqual(made.shown.notices, [step]);
    const shown = made.shown.notices;
    made.clock.advance(1199);
    assert.equal(
        made.shown.notices,
        shown,
        "the same list while nothing changes",
    );
    made.clock.advance(1);
    assert.deepEqual(made.shown.notices, []);
    // Skipped, it is never shown.
    made.play(view([1]), { beats: [[step]] });
    made.skip();
    assert.deepEqual(made.shown.notices, []);
});
