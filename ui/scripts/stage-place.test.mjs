// Changes on a card where it lies and to a player, on the stage, with no browser (`npm test`):
// a permanent turns as it taps, about its card's own middle; a creature lunges at what it
// strikes and drifts back; a permanent turns over where it lies; and a number that changes
// says so for as long as its step plays. The same in a browser, from both seats, is verbs.mjs.
import assert from "node:assert/strict";
import { test } from "node:test";
import { lifeChange } from "../src/stage/notices.ts";
import { ALOFT } from "../src/stage/pose.ts";
import { FLIP, STRIKE } from "../src/stage/tracks.ts";
import { object, reach, stage, view } from "./stage-table.mjs";

const LINEAR = { duration: 200 };
const tap = (slot, tapped) => ({
    act: { verb: "tap", object: object(slot, "battlefield", { tapped }) },
});
const at = ({ x, y, width, turn, depth }) => ({ x, y, width, turn, depth });

for (const tick of [16, 100, 1000]) {
    test(`a permanent turns a quarter about its card's middle as it taps: ticks of ${tick} ms`, () => {
        const { stage: made, surface } = stage({ tap: LINEAR });
        made.play(view([1, 2]));
        made.play(view([1, 2]), { beats: [[tap(2, true)]] });
        made.clock.advance(0);
        // Dressed lying from the first, its card a quarter back and where it stood: the middle
        // of a card 100 by 140 at (400, 0) is that of one lying 140 by 100 at (380, 20).
        assert.equal(made.shown.resting.get(2).object.tapped, true);
        assert.deepEqual(at(made.pose(2)), {
            x: 380,
            y: 20,
            width: 100,
            turn: -90,
            depth: 2,
        });
        assert.deepEqual(surface.nodes.get(2), made.pose(2));
        reach(made.clock, 100, tick);
        if (made.clock.now === 100) {
            assert.deepEqual(at(made.pose(2)), {
                x: 390,
                y: 10,
                width: 100,
                turn: -45,
                depth: 2,
            });
        }
        reach(made.clock, 200, tick);
        assert.deepEqual(at(made.pose(2)), {
            x: 400,
            y: 0,
            width: 100,
            turn: 0,
            depth: 2,
        });
        assert.equal(made.pose(1).x, 0);
    });
}

test("an untap turns it back the other way, and a card that lay so already does not turn", () => {
    const { stage: made } = stage({ tap: LINEAR });
    made.play({
        ...view([1, 2]),
        objects: [
            object(1, "battlefield", { tapped: true }),
            object(2, "battlefield", { tapped: true }),
        ],
    });
    made.play(view([1, 2]), { beats: [[tap(1, false), tap(2, true)]] });
    made.clock.advance(0);
    assert.deepEqual(at(made.pose(1)), {
        x: 20,
        y: -20,
        width: 100,
        turn: 90,
        depth: 1,
    });
    assert.equal(made.pose(2).turn, 0);
    made.settle();
    assert.equal(made.pose(1).turn, 0);
});

test("a card that taps and then leaves has done turning as it begins to go", () => {
    const { stage: made } = stage({
        tap: LINEAR,
        vanish: { duration: 300 },
    });
    made.play(view([1, 2]));
    const gone = {
        act: {
            verb: "vanish",
            subject: { kind: "object", slot: 2 },
            from: "battlefield",
            why: "sacrifice",
            object: object(2, "graveyard", { tapped: false }),
        },
    };
    made.play(view([1]), { beats: [[tap(2, true)], [gone]] });
    const edges = [];
    made.follow((edge) =>
        edges.push([
            edge.edge,
            edge.step.act.verb,
            edge.at,
            made.pose(2)?.turn ?? null,
        ]),
    );
    made.settle();
    // The script orders them: the departure begins at the stage time the turn ends, with
    // the card turned all the way; in the graveyard it rests nowhere on this table.
    assert.deepEqual(edges, [
        ["start", "tap", 0, 0],
        ["end", "tap", 200, 0],
        ["start", "vanish", 200, 0],
        ["end", "vanish", 500, null],
    ]);
});

const strike = (source, to) => ({
    act: { verb: "strike", source, to, amount: 2 },
});

test("a creature lunges at the player it strikes and drifts back", () => {
    const { stage: made } = stage({ strike: { duration: 1000 } });
    made.play(view([1, 2]));
    made.play(view([1, 2]), {
        beats: [[strike(1, { kind: "player", player: "opponent" })]],
    });
    // From its middle (50, 70) to the opponent's life total (1000, -400).
    const toward = { x: 950, y: -470 };
    made.clock.advance(STRIKE.wind * 1000);
    assert.ok(Math.abs(made.pose(1).x + toward.x * STRIKE.back) < 1e-9);
    assert.ok(made.pose(1).y > 0, "it pulls back first");
    made.clock.advance((STRIKE.hit - STRIKE.wind) * 1000);
    assert.ok(Math.abs(made.pose(1).x - toward.x * STRIKE.reach) < 1e-9);
    assert.ok(Math.abs(made.pose(1).y - toward.y * STRIKE.reach) < 1e-9);
    assert.ok(made.pose(1).depth >= ALOFT);
    assert.equal(made.pose(1).width, 100);
    assert.deepEqual(at(made.pose(2)), at({ ...made.pose(2), x: 400, y: 0 }));
    made.clock.advance(250);
    const back = made.pose(1);
    assert.ok(back.x > 0 && back.x < toward.x * STRIKE.reach);
    made.settle();
    assert.deepEqual(at(made.pose(1)), {
        x: 0,
        y: 0,
        width: 100,
        turn: 0,
        depth: 1,
    });
});

test("creatures that strike one another lunge at each other's middles", () => {
    const { stage: made } = stage({ strike: { duration: 1000 } });
    made.play(view([1, 2]));
    made.play(view([1, 2]), {
        beats: [
            [
                strike(1, { kind: "object", slot: 2 }),
                strike(2, { kind: "object", slot: 1 }),
            ],
        ],
    });
    made.clock.advance(STRIKE.hit * 1000);
    assert.ok(Math.abs(made.pose(1).x - 400 * STRIKE.reach) < 1e-9);
    assert.ok(Math.abs(made.pose(2).x - (400 - 400 * STRIKE.reach)) < 1e-9);
    assert.equal(made.pose(1).y, 0);
    made.settle();
    assert.equal(made.pose(1).x, 0);
    assert.equal(made.pose(2).x, 400);
});

const flip = (slot, zone = "battlefield") => ({
    act: { verb: "flip", object: object(slot, zone) },
});

for (const tick of [16, 100, 1000]) {
    test(`a permanent turns over where it lies, grown about its middle: ticks of ${tick} ms`, () => {
        const { stage: made, surface } = stage({ flip: { duration: 1000 } });
        made.play({
            ...view([1, 2]),
            objects: [object(1), object(2, "battlefield", { name: null })],
        });
        made.play(view([1, 2]), { beats: [[flip(2)]] });
        made.clock.advance(0);
        // As it was, at first: face down where it lay.
        assert.equal(made.pose(2).face, "back");
        assert.equal(made.shown.resting.get(2).object.name, null);
        assert.equal(made.pose(2).x, 400);
        reach(made.clock, 400, tick);
        if (made.clock.now === 400) {
            // Grown about its middle (450, 70), and still showing its back.
            const grown = made.pose(2);
            assert.equal(grown.width, 100 * FLIP.size);
            assert.ok(Math.abs(grown.x - (450 - 50 * FLIP.size)) < 1e-9);
            assert.ok(Math.abs(grown.y - (70 - 70 * FLIP.size)) < 1e-9);
            assert.equal(grown.face, "back");
            assert.ok(grown.depth >= ALOFT);
        }
        reach(made.clock, 600, tick);
        if (made.clock.now === 600) {
            assert.equal(made.pose(2).face, "front");
            assert.equal(made.pose(2).width, 100 * FLIP.size);
            assert.equal(made.shown.resting.get(2).object.name, "Card 2");
        }
        reach(made.clock, 1000, tick);
        assert.deepEqual(at(made.pose(2)), {
            x: 400,
            y: 0,
            width: 100,
            turn: 0,
            depth: 2,
        });
        assert.equal(made.shown.resting.get(2).object.name, "Card 2");
        assert.deepEqual(surface.nodes.get(2), made.pose(2));
        assert.equal(made.pose(1).x, 0);
    });
}

test("a life total that changes says by how much while its count plays", () => {
    const { stage: made } = stage({ count: { duration: 400 } });
    const life = (player, to) => ({
        act: { verb: "count", tally: { kind: "life", player, life: to } },
    });
    made.play(view([1]));
    made.play(view([1]), {
        beats: [
            [life("opponent", 17), life("opponent", 16)],
            [life("you", 24)],
        ],
    });
    let told = 0;
    made.subscribe(() => (told += 1));
    made.clock.advance(0);
    // The total is the new one from the first, and the change beside it is of both counts.
    assert.equal(made.shown.table.players.opponent.life, 16);
    assert.equal(lifeChange(made.shown.counts, "opponent"), -4);
    assert.equal(lifeChange(made.shown.counts, "you"), undefined);
    const shown = made.shown;
    made.clock.advance(399);
    assert.equal(made.shown, shown, "nothing is told while the count plays");
    made.clock.advance(1);
    assert.equal(lifeChange(made.shown.counts, "opponent"), undefined);
    assert.equal(lifeChange(made.shown.counts, "you"), 4);
    made.settle();
    assert.deepEqual(made.shown.counts, []);
    assert.equal(lifeChange(made.shown.counts, "you"), undefined);
    assert.ok(told >= 3);
});
