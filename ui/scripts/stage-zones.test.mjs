// Cards changing zone on the stage, with no browser (`npm test`): what a `vanish` takes goes
// from sight where it was and is only then at its new place, what it leaves room for waits
// until it has gone, a card that leaves face down
// turns over on its way, your draw is held up to be read, and the cards the opponent looks at
// lift for as long as the look lasts. The same in a browser, from both seats, is verbs.mjs.
import assert from "node:assert/strict";
import { test } from "node:test";
import { apply, tableOf } from "../src/stage/table.ts";
import { GONE, HELD, TURN } from "../src/stage/tracks.ts";
import { object, reach, stage, view } from "./stage-table.mjs";

const vanish = (slot, why, to) => ({
    act: {
        verb: "vanish",
        subject: { kind: "object", slot },
        from: "battlefield",
        why,
        ...(to ? { object: to } : {}),
    },
});

for (const tick of [16, 100, 1000]) {
    test(`a returned permanent goes from sight in place, then is in its hand: ticks of ${tick} ms`, () => {
        const { stage: made, surface } = stage({ vanish: { duration: 400 } });
        made.play(view([1, 2, 3]));
        made.play(view([1, 3], [2]), {
            beats: [[vanish(2, "return", object(2, "hand"))]],
        });
        made.clock.advance(0);
        // It is where it lay, whole, and dressed as it was.
        assert.deepEqual(surface.nodes.get(2), made.pose(2));
        assert.equal(made.pose(2).x, 400);
        assert.equal(made.pose(2).opacity, 1);
        assert.equal(made.shown.resting.get(2).dress.zone, "battlefield");
        // The table has it gone already, but the row closes up only once it has gone.
        assert.deepEqual(made.shown.table.battlefield, [1, 3]);
        reach(made.clock, 200, tick);
        if (made.clock.now === 200) {
            assert.equal(made.pose(2).x, 400);
            assert.equal(made.pose(2).y, 0);
            assert.equal(made.pose(2).opacity, 1 - (0.5 / GONE) ** 2);
            assert.equal(made.pose(3).x, 800);
        }
        reach(made.clock, 340, tick);
        if (made.clock.now === 340) {
            assert.equal(made.pose(2).opacity, 0);
            assert.equal(made.pose(3).x, 600);
            assert.ok(made.pose(3).depth < 900, "what makes room is not aloft");
        }
        reach(made.clock, 400, tick);
        // At the step's end it is in the hand, with no flight.
        assert.deepEqual(
            {
                x: made.pose(2).x,
                y: made.pose(2).y,
                opacity: made.pose(2).opacity,
            },
            { x: 0, y: 500, opacity: 1 },
        );
        assert.equal(made.shown.resting.get(2).dress.zone, "hand");
        assert.equal(made.pose(3).x, 400);
        assert.equal(made.shown.busy, false);
        assert.deepEqual(surface.dressed.slice(-1), ["2 hand"]);
    });
}

test("a token that ceases to exist keeps its node until its vanish is over", () => {
    const { stage: made, surface } = stage({ vanish: { duration: 400 } });
    const table = view([1, 7]);
    table.objects[1].token = true;
    made.play(table);
    made.play(view([1]), { beats: [[vanish(7, "destroy")]] });
    made.clock.advance(399);
    assert.ok(surface.nodes.has(7));
    assert.equal(made.pose(7).x, 400);
    assert.equal(made.pose(7).opacity, 0);
    assert.equal(surface.log.at(-1), "make 7");
    made.clock.advance(1);
    assert.equal(made.pose(7), null);
    assert.equal(surface.log.at(-1), "remove 7");
});

test("a reset while a card is leaving puts everything where the view has it", () => {
    const { stage: made } = stage({ vanish: { duration: 400 } });
    made.play(view([1, 2]));
    made.play(view([1], [2]), {
        beats: [[vanish(2, "return", object(2, "hand"))]],
    });
    made.clock.advance(100);
    made.play(view([1, 2]));
    assert.deepEqual(
        { x: made.pose(2).x, y: made.pose(2).y, opacity: made.pose(2).opacity },
        { x: 400, y: 0, opacity: 1 },
    );
    assert.equal(made.shown.busy, false);
});

test("a step about a card still leaving finds it gone", () => {
    const { stage: made } = stage({
        vanish: { duration: 400 },
        fly: { duration: 200, delay: 100 },
    });
    made.play(view([1, 2]));
    const back = {
        act: {
            verb: "fly",
            from: "hand",
            why: "effect",
            object: object(2),
        },
    };
    made.play(view([1, 2]), {
        beats: [[vanish(2, "return", object(2, "hand")), back]],
    });
    made.clock.advance(100);
    // The flight begins from where it was leaving, to where it rests again.
    assert.equal(made.shown.resting.get(2).dress.zone, "battlefield");
    made.clock.advance(200);
    assert.deepEqual(
        { x: made.pose(2).x, y: made.pose(2).y, opacity: made.pose(2).opacity },
        { x: 400, y: 0, opacity: 1 },
    );
    made.settle();
    assert.equal(made.pose(2).x, 400);
});

/** A view in which the opponent holds the card in slot 5, unseen. */
const theirs = () => {
    const table = view([1]);
    table.objects.push(
        object(5, "hand", {
            name: null,
            owner: "opponent",
            controller: "opponent",
        }),
    );
    return table;
};
const theirCast = {
    act: {
        verb: "fly",
        from: "hand",
        why: "cast",
        object: object(5, "stack", {
            owner: "opponent",
            controller: "opponent",
        }),
        item: { kind: "spell", slot: 5, targets: [] },
    },
};

for (const tick of [16, 100]) {
    test(`a card that leaves face down turns over a little past half way: ticks of ${tick} ms`, () => {
        const { stage: made, surface } = stage({ fly: { duration: 400 } });
        made.play(theirs());
        assert.equal(made.pose(5).face, "back");
        const after = theirs();
        after.objects[1] = theirCast.act.object;
        after.stack = [theirCast.act.item];
        made.play(after, { beats: [[theirCast]] });
        made.clock.advance(0);
        assert.equal(made.pose(5).face, "back");
        assert.equal(made.shown.resting.get(5).dress.back, true);
        assert.equal(made.shown.resting.get(5).object.name, "Card 5");
        const turn = 400 * TURN;
        reach(made.clock, turn - 20, tick);
        assert.equal(made.pose(5).face, "back");
        assert.equal(made.shown.resting.get(5).dress.back, true);
        reach(made.clock, turn, tick);
        assert.equal(made.pose(5).face, "front");
        assert.equal(made.shown.resting.get(5).dress.back, false);
        reach(made.clock, 400, tick);
        assert.equal(made.pose(5).face, "front");
        assert.deepEqual(surface.dressed.slice(-2), ["5 back", "5 stack"]);
    });
}

test("a card that only makes room, or whose face shows from the first, never turns", () => {
    const { stage: made } = stage({ fly: { duration: 400 } });
    made.play(view([1], [3]));
    const cast = {
        act: {
            verb: "fly",
            from: "hand",
            why: "cast",
            object: object(3, "stack"),
            item: { kind: "spell", slot: 3, targets: [] },
        },
    };
    made.play(view([1]), { beats: [[cast]] });
    made.clock.advance(10);
    assert.equal(made.pose(3).face, "front");
    assert.equal(made.shown.resting.get(3).dress.back, false);
});

/** A view in which your library holds the card in slot 9, unseen, and your hand slot 4. */
const drawing = () => {
    const table = view([1], [4]);
    table.objects.push(object(9, "library", { name: null }));
    table.players.you.library = [9];
    return table;
};
const draw = (slot) => ({
    act: {
        verb: "fly",
        from: "library",
        why: "draw",
        object: object(slot, "hand"),
    },
});

for (const tick of [10, 50]) {
    test(`your draw is held up on its way to your hand: ticks of ${tick} ms`, () => {
        const { stage: made } = stage({ fly: { duration: 1000 } });
        made.play(drawing());
        made.play(view([1], [4, 9]), { beats: [[draw(9)]] });
        made.clock.advance(0);
        assert.deepEqual([made.pose(9).x, made.pose(9).y], [3000, 500]);
        assert.equal(made.pose(9).face, "back");
        assert.equal(made.shown.resting.get(9).dress.back, true);
        // It turns over on its way in.
        reach(made.clock, Math.floor(1000 * HELD.in * TURN) - 10, tick);
        assert.equal(made.pose(9).face, "back");
        reach(made.clock, Math.ceil(1000 * HELD.in * TURN) + 10, tick);
        assert.equal(made.pose(9).face, "front");
        assert.equal(made.shown.resting.get(9).dress.back, false);
        // It is held over where it will lie (the hand's second place, at x 400), large.
        const held = { x: 400, y: 200, width: 200 };
        for (const at of [1000 * HELD.in, 1000 * (HELD.in + HELD.hold)]) {
            reach(made.clock, at, tick);
            const { x, y, width } = made.pose(9);
            assert.deepEqual({ x, y, width }, held, `at ${at} ms`);
            assert.ok(made.pose(9).depth >= 900, "it is over what rests");
        }
        reach(made.clock, 1000, tick);
        const { x, y, width, depth } = made.pose(9);
        assert.deepEqual(
            { x, y, width, depth },
            { x: 400, y: 500, width: 100, depth: 9 },
        );
        assert.equal(made.shown.busy, false);
    });
}

test("several cards drawn together fly straight to the hand", () => {
    const { stage: made } = stage({ fly: { duration: 1000, stagger: 100 } });
    const table = drawing();
    table.objects.push(object(10, "library", { name: null }));
    table.players.you.library = [10, 9];
    made.play(table);
    made.play(view([1], [4, 9, 10]), { beats: [[draw(9), draw(10)]] });
    made.clock.advance(500);
    // Half way, the first is half way: linear, from (3000, 500) to (400, 500).
    assert.deepEqual([made.pose(9).x, made.pose(9).y], [1700, 500]);
    assert.equal(made.pose(9).width, 100);
});

test("the opponent's draw flies face down, and is not held up", () => {
    const { stage: made } = stage({ fly: { duration: 1000 } });
    const table = view([1]);
    const theirCard = { name: null, owner: "opponent", controller: "opponent" };
    table.objects.push(object(9, "library", theirCard));
    table.players.opponent.library = [9];
    made.play(table);
    const drawn = {
        act: {
            verb: "fly",
            from: "library",
            why: "draw",
            object: object(9, "hand", theirCard),
        },
    };
    const after = view([1]);
    after.objects.push(drawn.act.object);
    made.play(after, { beats: [[drawn]] });
    made.clock.advance(500);
    assert.deepEqual([made.pose(9).x, made.pose(9).y], [1500, 500]);
    assert.equal(made.pose(9).face, "back");
    assert.equal(made.pose(9).width, 100);
});

test("a lift keeps the cards it began with lifted until it ends", () => {
    const handling = {
        zone: "library",
        side: "opponent",
        count: 2,
        scry: true,
    };
    const start = view([1]);
    start.players.opponent.library = [5, 6, 7];
    const table = tableOf(start);
    const lift = (span) => ({ verb: "lift", span, handling });
    const lifted = apply(table, lift("begin"));
    assert.deepEqual(lifted.view.handling, handling);
    // The top two, top first.
    assert.deepEqual(lifted.handled, [7, 6]);
    // The look puts one on the bottom: the same two are still the ones handled.
    const order = { kind: "library", player: "opponent", order: [7, 5, 6] };
    const ordered = apply(lifted, { verb: "set", fact: order });
    assert.deepEqual(ordered.handled, [7, 6]);
    const lowered = apply(lifted, lift("end"));
    assert.deepEqual(lowered, table);
});
