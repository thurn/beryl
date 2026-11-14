// Cards changing zone on the stage, with no browser (`npm test`): what a `vanish` takes goes
// from sight where it was and is at its new place as its step ends, what it leaves room for
// closes up after, in time the beat has only then, a card that leaves face down
// turns over on its way, your draw is held up to be read, and the cards the opponent looks at
// lift for as long as the look lasts. The same in a browser, from both seats, is verbs.mjs.
import assert from "node:assert/strict";
import { test } from "node:test";
import { apply, tableOf } from "../src/stage/table.ts";
import { ScriptStage, SNAP } from "../src/stage/stage.ts";
import {
    BOWS,
    CLOSES,
    halfTurned,
    TURN,
    TURNING,
} from "../src/stage/tracks.ts";
import { HELD, HELD_TURN } from "../src/stage/drawn.ts";
import { tippedOf } from "../src/stage/pose.ts";
import { timingOf } from "../src/stage/timing.ts";
import { object, reach, row, stage, view } from "./stage-table.mjs";

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
        assert.equal(made.pose(2).x, 400);
        assert.equal(made.pose(2).y, 0);
        assert.equal(made.pose(2).opacity, 1 - 0.5 ** 2);
        assert.equal(made.pose(3).x, 800);
        reach(made.clock, 400, tick);
        // As the step ends it is gone from there and in the hand, with no flight, and the
        // row has yet to move.
        assert.equal(made.step, null);
        assert.deepEqual(
            {
                x: made.pose(2).x,
                y: made.pose(2).y,
                opacity: made.pose(2).opacity,
            },
            { x: 0, y: 500, opacity: 1 },
        );
        assert.equal(made.shown.resting.get(2).dress.zone, "hand");
        assert.deepEqual(surface.dressed.slice(-1), ["2 hand"]);
        assert.equal(made.pose(3).x, 800);
        assert.equal(made.shown.busy, true);
        // The row closes up in the time after, which is the beat's too.
        const closing = 400 * CLOSES;
        reach(made.clock, 400 + closing / 2, tick);
        assert.equal(made.pose(3).x, 600);
        assert.ok(made.pose(3).depth < 900, "what makes room is not aloft");
        reach(made.clock, 400 + closing - 1, tick);
        assert.equal(made.shown.busy, true);
        reach(made.clock, 400 + closing, tick);
        assert.equal(made.pose(3).x, 400);
        assert.equal(made.shown.busy, false);
    });
}

/** When each step of `beats` began and ended on a stage showing `table`, each vanish taking
 * 400 ms: `[edge, slot, at]`. */
function edgesOf(table, beats) {
    const { stage: made } = stage({ vanish: { duration: 400 } });
    made.play(table);
    const edges = [];
    made.follow(({ edge, step, at }) =>
        edges.push([edge, step.act.subject.slot, at]),
    );
    made.play(table, { beats });
    made.settle();
    return edges;
}

test("what follows a card that leaves waits only for what closes up after it", () => {
    const grave = (slot) => vanish(slot, "destroy", object(slot, "graveyard"));
    // The last of its row leaves room for nothing: the next beat begins as it is gone.
    assert.deepEqual(edgesOf(view([1, 2, 3]), [[grave(3)], [grave(2)]]), [
        ["start", 3, 0],
        ["end", 3, 400],
        ["start", 2, 400],
        ["end", 2, 800],
    ]);
    // The first leaves room the row closes up in, and the next beat begins after that.
    const closed = 400 + 400 * CLOSES;
    assert.deepEqual(edgesOf(view([1, 2, 3]), [[grave(1)], [grave(3)]]), [
        ["start", 1, 0],
        ["end", 1, 400],
        ["start", 3, closed],
        ["end", 3, closed + 400],
    ]);
    // Two that leave together: the second would have closed up after the first, and goes
    // where it lay instead, so nothing closes up and nothing is waited for.
    assert.deepEqual(
        edgesOf(view([1, 2, 3]), [[grave(2), grave(3)], [grave(1)]]),
        [
            ["start", 2, 0],
            ["start", 3, 0],
            ["end", 2, 400],
            ["end", 3, 400],
            ["start", 1, 400],
            ["end", 1, 800],
        ],
    );
});

test("what a finished ability left room for closes up after the cut, and its beat waits", () => {
    // A table whose permanents stand 100 lower while anything is on the stack, as a phone's
    // rows keep clear of its band.
    const banded = {
        ...row,
        rest(table) {
            const out = row.rest(table);
            if (table.view.stack.length === 0) return out;
            for (const [name, lies] of out) {
                if (lies.dress.zone !== "battlefield") continue;
                const pose = { ...lies.pose, y: lies.pose.y + 100 };
                out.set(name, { ...lies, pose });
            }
            return out;
        },
    };
    const ability = { kind: "ability", source: 1, name: "Card 1", targets: [] };
    const finish = {
        act: {
            verb: "vanish",
            subject: { kind: "ability", id: 1 },
            from: "stack",
            why: "resolve",
        },
    };
    const play = (timing) => {
        const made = new ScriptStage({ layout: banded, timings: () => timing });
        const reset = { reset: { abilities: [1] }, beats: [] };
        made.play({ ...view([1]), stack: [ability] }, reset);
        assert.equal(made.pose(1).y, 100);
        made.play(view([1]), { beats: [[finish]] });
        made.clock.advance(0);
        return made;
    };
    // The ability is gone in the tick its step begins, and nothing else has moved yet.
    const made = play({ ...SNAP, closes: 200 });
    assert.equal(made.pose("a1"), null);
    assert.equal(made.pose(1).y, 100);
    // Then the row closes up, slowing into its place, and the beat is over when it has.
    made.clock.advance(100);
    assert.deepEqual([made.pose(1).y, made.shown.busy], [25, true]);
    made.clock.advance(100);
    assert.deepEqual([made.pose(1).y, made.shown.busy], [0, false]);
    // With no motion, it is closed up at once.
    assert.equal(play(SNAP).pose(1).y, 0);
});

test("a layout is told what is still leaving, and no longer once it has landed", () => {
    const told = [];
    const made = new ScriptStage({
        layout: {
            ...row,
            rest(table, leaving) {
                told.push([...leaving]);
                return row.rest(table);
            },
        },
        timings: (act) => ({
            ...SNAP,
            duration: act.verb === "vanish" ? 400 : 0,
        }),
    });
    made.play(view([1, 2, 3]));
    made.play(view([1]), {
        beats: [
            [
                vanish(2, "destroy", object(2, "graveyard")),
                vanish(3, "destroy", object(3, "graveyard")),
            ],
        ],
    });
    made.settle();
    // The first view, each step's beginning, each one's end, and the view at the last.
    assert.deepEqual(told, [[], [2], [2, 3], [3], [], []]);
});

test("a layout is told of a card flying to a pile until it lands: the pile keeps its top", () => {
    const telling = () => {
        const told = [];
        const made = new ScriptStage({
            layout: {
                ...row,
                // A graveyard's cards lie in one place, as a library's do.
                rest(table, leaving) {
                    told.push([...leaving]);
                    const out = row.rest(table);
                    for (const card of table.view.objects) {
                        if (card.zone !== "graveyard") continue;
                        const lies = out.get(1);
                        const pose = { ...lies.pose, x: 3000, y: 0 };
                        out.set(card.slot, { ...lies, object: card, pose });
                    }
                    return out;
                },
            },
            timings: (act) => ({
                ...SNAP,
                duration: act.verb === "fly" ? 400 : 0,
            }),
        });
        return { made, told };
    };
    const flight = (slot, from, zone) => ({
        act: { verb: "fly", from, why: "discard", object: object(slot, zone) },
    });
    // A discard: told as it begins, and the table is laid out again as it lands.
    const { made, told } = telling();
    made.play(view([1], [2]));
    const after = view([1]);
    after.objects.push(object(2, "graveyard"));
    after.players.you.graveyard = [2];
    made.play(after, { beats: [[flight(2, "hand", "graveyard")]] });
    made.settle();
    assert.deepEqual(told, [[], [2], [], []]);
    // A card that flies to no pile lands where the table has had it all along.
    const other = telling();
    other.made.play(view([1], [2]));
    other.made.play(view([1, 2]), {
        beats: [[flight(2, "hand", "battlefield")]],
    });
    other.made.settle();
    assert.deepEqual(other.told, [[], [], []]);
});

test("a token that ceases to exist keeps its node until its vanish is over", () => {
    const { stage: made, surface } = stage({ vanish: { duration: 400 } });
    const table = view([1, 7]);
    table.objects[1].token = true;
    made.play(table);
    made.play(view([1]), { beats: [[vanish(7, "destroy")]] });
    made.clock.advance(200);
    assert.equal(made.pose(7).opacity, 0.75);
    made.clock.advance(199);
    assert.ok(surface.nodes.has(7));
    assert.equal(made.pose(7).x, 400);
    assert.ok(made.pose(7).opacity < 0.01, "it is all but gone");
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

test("a card that turns over on its way is edge-on as its face changes, and flat at its ends", () => {
    const { stage: made } = stage({ fly: { duration: 400 } });
    made.play(theirs());
    const after = theirs();
    after.objects[1] = theirCast.act.object;
    after.stack = [theirCast.act.item];
    made.play(after, { beats: [[theirCast]] });
    const flat = (progress) => {
        made.clock.advance(400 * progress - made.clock.now);
        return Math.round(made.pose(5).flat * 1000) / 1000;
    };
    // Flat until it begins to turn; half its height shows a third of the way to edge-on and
    // as far past it, where its face is up; and it lies flat again for the rest of its way.
    assert.equal(flat(0), 1);
    assert.equal(flat(TURN - TURNING), 1);
    assert.deepEqual(
        [flat(TURN - TURNING / 3), made.pose(5).face],
        [0.5, "back"],
    );
    assert.deepEqual([flat(TURN), made.pose(5).face], [0, "front"]);
    assert.deepEqual(
        [flat(TURN + TURNING / 3), made.pose(5).face],
        [0.5, "front"],
    );
    assert.equal(flat(TURN + TURNING), 1);
    made.clock.advance(400);
    assert.equal(made.pose(5).flat, undefined, "at rest nothing is said of it");
});

/** The opponent's hand faces them: its cards lie half a turn round, about their middles. */
const facing = {
    ...row,
    rest(table) {
        const out = row.rest(table);
        for (const [name, lies] of out) {
            if (lies.dress.zone !== "hand") continue;
            const middle = row.middle(lies, lies.pose);
            out.set(name, {
                ...lies,
                pose: halfTurned(lies.pose, middle, 180),
            });
        }
        return out;
    },
};

test("a card that lands the other way up the table turns over, and never spins round", () => {
    const made = new ScriptStage({
        layout: facing,
        timings: (act) => ({ ...SNAP, duration: act.verb === "fly" ? 400 : 0 }),
    });
    made.play(theirs());
    const lies = made.shown.resting.get(5);
    assert.equal(lies.pose.rotation, 180);
    const from = row.middle(lies, { ...lies.pose, x: 0, y: 500, rotation: 0 });
    const after = theirs();
    after.objects[1] = theirCast.act.object;
    after.stack = [theirCast.act.item];
    made.play(after, { beats: [[theirCast]] });
    // Its card's middle, wherever its corner is: about that corner it is turned half round
    // while its back shows.
    const middle = () => {
        const pose = made.pose(5);
        const upright = row.middle(made.shown.resting.get(5), pose);
        const turned = Math.abs(pose.rotation - 180) < 90;
        return {
            x: turned ? 2 * pose.x - upright.x : upright.x,
            y: turned ? 2 * pose.y - upright.y : upright.y,
        };
    };
    let last = from;
    for (let at = 0; at <= 400; at += 10) {
        made.clock.advance(at - made.clock.now);
        const pose = made.pose(5);
        // Upside down while its back shows and upright once its face does, but for the few
        // degrees it leans in its bow (`BOWS.theirs`): it never spins round between.
        const off = pose.rotation - (pose.face === "back" ? 180 : 0);
        assert.ok(
            Math.abs(off) <= Math.abs(BOWS.theirs.lean),
            `at ${at}: ${off}`,
        );
        const now = middle();
        const moved = Math.hypot(now.x - last.x, now.y - last.y);
        assert.ok(moved < 400, `its middle went ${moved} in a tick at ${at}`);
        last = now;
    }
    assert.deepEqual(last, { x: 2050, y: 70 });
});

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
        // It turns over on its way in, off the straight line toward the table.
        reach(made.clock, Math.floor(1000 * HELD_TURN) - 10, tick);
        assert.equal(made.pose(9).face, "back");
        reach(made.clock, Math.ceil(1000 * HELD_TURN) + 10, tick);
        assert.ok(made.pose(9).y < 500 - 300 * (1 - made.pose(9).x / 2600));
        assert.equal(made.pose(9).face, "front");
        assert.equal(made.shown.resting.get(9).dress.back, false);
        // It is held where the layout holds a card drawn (over the hand's second place, at
        // x 400, here), large, and sinks a little toward the hand while it is held.
        reach(made.clock, 1000 * HELD.in, tick);
        const { x: left, y: top, width: wide } = made.pose(9);
        assert.deepEqual([left, top, wide], [400, 200, 200]);
        reach(made.clock, 1000 * (HELD.in + HELD.hold), tick);
        const sunk = made.pose(9);
        assert.equal(sunk.x, 400);
        assert.ok(sunk.y > 200 && sunk.y < 240, `it sinks: ${sunk.y}`);
        assert.ok(sunk.width > 180, "still large");
        assert.ok(sunk.depth >= 900, "it is over what rests");
        reach(made.clock, 1000, tick);
        const { x, y, width, depth } = made.pose(9);
        assert.deepEqual(
            { x, y, width, depth },
            { x: 400, y: 500, width: 100, depth: 9 },
        );
        assert.equal(made.shown.busy, false);
    });
}

test("a card that comes to a hand joins its right end, whatever its slot", () => {
    const { stage: made } = stage({ fly: { duration: 400 } });
    const table = view([1], [4, 7]);
    table.objects.push(object(2, "library", { name: null }));
    table.players.you.library = [2];
    made.play(table);
    made.play(view([1], [2, 4, 7]), { beats: [[draw(2)]] });
    made.clock.advance(400);
    const at = (slot) => made.shown.resting.get(slot).pose.x;
    assert.deepEqual([at(4), at(7), at(2)], [0, 400, 800]);
    // And it keeps that place when a later view is taken whole, until it leaves the hand.
    made.play(view([1], [2, 4, 7]));
    made.clock.advance(0);
    assert.deepEqual([at(4), at(7), at(2)], [0, 400, 800]);
});

test("several cards drawn together sweep past where one is held, to a hand that opens once", () => {
    const ahead = { duration: 1000, stagger: 100, ahead: true };
    const { stage: made } = stage({ fly: ahead });
    const table = drawing();
    table.objects.push(object(10, "library", { name: null }));
    table.players.you.library = [10, 9];
    made.play(table);
    made.play(view([1], [4, 9, 10]), { beats: [[draw(9), draw(10)]] });
    made.clock.advance(0);
    // Both are in the hand as the first leaves: the second waits at the library for its
    // turn, a card back, and the place the first flies to is where it stays.
    const [first, second] = [
        made.shown.resting.get(9),
        made.shown.resting.get(10),
    ];
    assert.deepEqual([first.pose.x, second.pose.x], [400, 800]);
    assert.deepEqual([made.pose(10).x, made.pose(10).y], [3000, 500]);
    made.clock.advance(50);
    assert.deepEqual([made.pose(10).x, made.pose(10).y], [3000, 500]);
    assert.equal(made.pose(10).face, "back");
    made.clock.advance(450);
    // Half way, the first is as far up the table and as large as a card drawn alone is
    // held (over its place in the hand, at twice its width, here), face up, and never past
    // that place; it does not stop there.
    const { x, y, width } = made.pose(9);
    assert.deepEqual([x, y, width], [1050, 200, 200]);
    assert.equal(made.pose(9).face, "front");
    made.clock.advance(50);
    assert.ok(made.pose(9).y > 200, "it goes on");
    // The second left a stagger after, and each lands where the hand had it from the first.
    assert.ok(made.pose(10).x < 3000 && made.pose(10).x > 800);
    made.clock.advance(600);
    assert.deepEqual([made.pose(9).x, made.pose(9).y], [400, 500]);
    assert.deepEqual([made.pose(10).x, made.pose(10).y], [800, 500]);
    assert.equal(made.shown.busy, false);
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
    // Half way along, and off the line toward the table, which lies down the stage of them.
    assert.equal(made.pose(9).x, 1500);
    assert.ok(made.pose(9).y > 700, `it dips: ${made.pose(9).y}`);
    assert.equal(made.pose(9).face, "back");
    assert.equal(made.pose(9).width, 100);
});

test("a card back that lands the other way up the table leaves that way up, and never spins", () => {
    const made = new ScriptStage({
        layout: facing,
        timings: (act) => ({ ...SNAP, duration: act.verb === "fly" ? 400 : 0 }),
    });
    const table = view([1]);
    const theirCard = { name: null, owner: "opponent", controller: "opponent" };
    table.objects.push(object(9, "library", theirCard));
    table.players.opponent.library = [9];
    made.play(table);
    assert.equal(made.pose(9).rotation, 0, "their library lies upright");
    const left = made.pose(9);
    const act = {
        verb: "fly",
        from: "library",
        why: "draw",
        object: object(9, "hand", theirCard),
    };
    const after = view([1]);
    after.objects.push(act.object);
    made.play(after, { beats: [[{ act }]] });
    // Half a turn round from the tick it begins to move, over the place it left, and so all
    // the way: a card back shows no other way up to turn through.
    const lay = row.middle(made.shown.resting.get(9), left);
    for (let at = 0; at <= 400; at += 10) {
        made.clock.advance(at - made.clock.now);
        const pose = made.pose(9);
        assert.equal(pose.rotation, 180, `at ${at}`);
        assert.equal(pose.face, "back", `at ${at}`);
        if (at > 0) continue;
        assert.deepEqual(
            [pose.x, pose.y],
            [2 * lay.x - left.x, 2 * lay.y - left.y],
            "about its own middle",
        );
    }
});

test("a card turning over is tipped in depth, its top away and then its foot down", () => {
    const pose = { face: "back", flat: 1 };
    assert.equal(tippedOf(pose), "", "flat, nothing is said");
    assert.equal(tippedOf({ ...pose, flat: undefined }), "");
    assert.equal(tippedOf({ ...pose, flat: 0.5 }), "60deg");
    assert.equal(tippedOf({ ...pose, flat: 0 }), "90deg");
    assert.equal(tippedOf({ face: "front", flat: 0 }), "-90deg");
    assert.equal(tippedOf({ face: "front", flat: 0.5 }), "-60deg");
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

// UI Conventions, "A batch exile": "Cards exiled together leave one at a time, about 0.3 s
// apart." The old board's check of it watched six cards leave a graveyard in a page
// ("simultaneous exile departures span three hundred milliseconds", by-w8gv.21.65); here the
// stage plays a beat of each kind with the design language's own timings, and tells when
// each step begins.
test("cards exiled together leave one at a time, about 0.3 s apart", () => {
    const exiled = (slot) => object(slot, "exile");
    const batches = {
        // Permanents exiled where they lie.
        permanents: [1, 2, 3].map((slot) =>
            vanish(slot, "exile", exiled(slot)),
        ),
        // Cards exiled from a graveyard, as a cost's are, fly to the exile pile.
        cards: [1, 2, 3].map((slot) => ({
            act: {
                verb: "fly",
                from: "graveyard",
                why: "exile",
                object: exiled(slot),
            },
        })),
    };
    for (const [what, beat] of Object.entries(batches)) {
        const { stage: made } = stage(timingOf);
        const before = view(what === "permanents" ? [1, 2, 3] : []);
        if (what === "cards") {
            before.objects = [1, 2, 3].map((slot) => object(slot, "graveyard"));
            before.players.you.graveyard = [1, 2, 3];
        }
        made.play(before);
        const began = [];
        made.follow((edge) => edge.edge === "start" && began.push(edge.at));
        const after = view([]);
        after.objects = [1, 2, 3].map(exiled);
        made.play(after, { beats: [beat] });
        reach(made.clock, 5000, 16);
        assert.equal(began.length, 3, what);
        for (const [i, at] of began.entries()) {
            if (i === 0) continue;
            const apart = at - began[i - 1];
            assert.ok(
                apart >= 250 && apart <= 350,
                `${what}: one left ${apart} ms after the one before`,
            );
        }
    }
});
