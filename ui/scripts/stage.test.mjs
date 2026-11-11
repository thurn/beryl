// The stage's clock and sequencing, with no browser (`npm test`, the fast check's ui lane):
// the model in ui/src/stage is plain TypeScript, which Node runs as it is. A stage here draws
// on a surface that only records, over a layout that lays the battlefield out in a row, so
// every expectation is a number worked out by hand.
import assert from "node:assert/strict";
import { test } from "node:test";
import { Clock } from "../src/stage/clock.ts";
import { ScriptStage, SNAP } from "../src/stage/stage.ts";
import { apply, tableOf } from "../src/stage/table.ts";

const object = (slot, zone = "battlefield", more = {}) => ({
    slot,
    zone,
    owner: "you",
    controller: "you",
    name: `Card ${slot}`,
    facing: "up",
    tapped: false,
    ...more,
});

/** A view holding `battlefield`, in order, and the cards in `hand`. */
const view = (battlefield, hand = [], more = {}) => ({
    objects: [
        ...battlefield.map((slot) => object(slot)),
        ...hand.map((slot) => object(slot, "hand")),
    ].sort((a, b) => a.slot - b.slot),
    battlefield,
    stack: [],
    players: {
        you: { life: 20, library: [], graveyard: [] },
        opponent: { life: 20, library: [], graveyard: [] },
    },
    turn: { number: 1, active: "you", step: "precombatMain" },
    ...more,
});

/** Permanents in a row 400 apart at y 0; a hand's cards in a row at y 500. Cards in no zone
 * drawn here (a graveyard's) rest nowhere. */
const row = {
    rest(table) {
        const out = new Map();
        const place = (slot, x, y) => {
            const record = table.objects.find((o) => o.slot === slot);
            out.set(slot, {
                object: record,
                pose: {
                    x,
                    y,
                    width: 100,
                    rotation: 0,
                    skew: 0,
                    opacity: 1,
                    depth: slot,
                    face: "front",
                },
                dress: {
                    zone: record.zone,
                    look: record.zone === "hand" ? "hand" : "field",
                    frame: 100,
                    back: false,
                    rim: false,
                },
            });
        };
        table.battlefield.forEach((slot, i) => place(slot, i * 400, 0));
        table.objects
            .filter((o) => o.zone === "hand")
            .forEach((o, i) => place(o.slot, i * 400, 500));
        return out;
    },
    anchors() {
        throw new Error("no test asks for an anchor");
    },
};

/** A surface that records what the stage did to it. */
function recorder() {
    const log = [];
    const nodes = new Map();
    return {
        log,
        nodes,
        make: (slot) => (nodes.set(slot, null), log.push(`make ${slot}`)),
        remove: (slot) => (nodes.delete(slot), log.push(`remove ${slot}`)),
        dress: () => {},
        draw: (slot, pose) => nodes.set(slot, pose),
    };
}

/** A stage on a held clock. `durations` gives the verbs that take time. */
function stage(durations = {}) {
    const surface = recorder();
    const made = new ScriptStage({
        layout: row,
        surface,
        timings: (verb) => ({ ...SNAP, ...durations[verb] }),
    });
    return { stage: made, surface };
}

/** A step moving the permanent in `slot` to place `at` of the battlefield's order. */
const move = (slot, at) => ({
    act: {
        verb: "fly",
        from: "battlefield",
        why: "effect",
        object: object(slot),
        at,
    },
});

/** Advances the clock to `time` in ticks of `tick`. */
function reach(clock, time, tick) {
    while (clock.now < time) clock.advance(Math.min(tick, time - clock.now));
}

test("the clock moves only when advanced, by exactly what it is given", () => {
    const spans = [];
    const clock = new Clock((from, to) => spans.push([from, to]));
    assert.equal(clock.now, 0);
    clock.advance(16);
    clock.advance(0);
    clock.advance(284);
    assert.equal(clock.now, 300);
    assert.deepEqual(spans, [
        [0, 16],
        [16, 16],
        [16, 300],
    ]);
    assert.throws(() => clock.advance(-1), RangeError);
    assert.throws(() => clock.advance(NaN), RangeError);
});

test("released, the clock sums the time between frames; held, it ignores them", () => {
    const pending = new Map();
    let handle = 0;
    const frames = {
        request: (frame) => (pending.set(++handle, frame), handle),
        cancel: (h) => pending.delete(h),
    };
    const frame = (timestamp) => {
        const due = [...pending.values()];
        pending.clear();
        for (const run of due) run(timestamp);
    };
    const clock = new Clock(() => {}, frames);
    frame(1000);
    assert.equal(clock.now, 0, "it starts held");
    clock.release();
    // The first frame after a release only sets where time is counted from: the page's
    // timestamp is no stage time.
    frame(5000);
    assert.equal(clock.now, 0);
    frame(5016);
    frame(5033);
    assert.equal(clock.now, 33);
    clock.hold();
    frame(6000);
    assert.equal(clock.now, 33);
    clock.release();
    frame(9000);
    frame(9010);
    assert.equal(clock.now, 43);
});

test("a view with no script is drawn at rest at once, one node an object", () => {
    const { stage: made, surface } = stage();
    made.play(view([1, 2], [3]));
    assert.deepEqual(surface.log, ["make 1", "make 2", "make 3"]);
    assert.equal(made.clock.now, 0);
    assert.deepEqual(
        [1, 2, 3].map((slot) => [made.pose(slot).x, made.pose(slot).y]),
        [
            [0, 0],
            [400, 0],
            [0, 500],
        ],
    );
    assert.deepEqual(surface.nodes.get(2), made.pose(2));
    assert.equal(made.pose(4), null);
    assert.equal(made.step, null);
});

// The case the probe on by-w8gv.21.3 found a promise chain to fail: a step of 600 ms and then
// one of 400 ms back, read at 800 ms, where the second step is half done. 600 is no multiple
// of 16, so the second step begins inside a tick, at 600 and not at the tick's end.
for (const tick of [16, 100, 1000]) {
    test(`two steps read the same at 800 ms in ticks of ${tick} ms`, () => {
        // Two verbs, so that the two steps differ in length.
        const { stage: made, surface } = stage({
            fly: { duration: 600 },
            appear: { duration: 400 },
        });
        made.play(view([1, 2]));
        // Card 1 goes behind card 2, from x 0 to x 400, and then back before it.
        const back = { act: { ...move(1, 0).act, verb: "appear" } };
        made.play(view([1, 2]), { beats: [[move(1, 1)], [back]] });
        reach(made.clock, 300, tick);
        if (tick <= 300) {
            assert.equal(made.pose(1).x, 200);
            assert.deepEqual(made.step, {
                index: 0,
                verb: "fly",
                from: 0,
                to: 600,
            });
        }
        reach(made.clock, 800, tick);
        assert.equal(made.pose(1).x, 200);
        assert.equal(made.pose(2).x, 200);
        assert.deepEqual(made.step, {
            index: 1,
            verb: "appear",
            from: 600,
            to: 1000,
        });
        assert.deepEqual(surface.nodes.get(1), made.pose(1));
        reach(made.clock, 1000, tick);
        assert.equal(made.pose(1).x, 0);
        assert.equal(made.step, null);
    });
}

test("the steps of a beat run together, each its stagger after the one before", () => {
    const { stage: made } = stage({ fly: { duration: 400, stagger: 100 } });
    made.play(view([1, 2, 3]));
    // 1 goes last, and a tenth of a second later 2 goes last: 3, 1, 2.
    made.play(view([3, 1, 2]), { beats: [[move(1, 2), move(2, 2)]] });
    made.clock.advance(50);
    assert.deepEqual(made.step, { index: 0, verb: "fly", from: 0, to: 400 });
    // The first step alone has begun: 2 and 3 each make for one place nearer.
    assert.equal(made.pose(1).x, 100);
    assert.equal(made.pose(2).x, 350);
    made.clock.advance(150);
    // At 200 the second step is 100 ms old: 2 turned back, from where it was at 100, for the
    // far end.
    assert.equal(made.pose(2).x, 300 + (800 - 300) * 0.25);
    made.clock.advance(250);
    assert.deepEqual(made.step, { index: 1, verb: "fly", from: 100, to: 500 });
    made.clock.advance(50);
    assert.equal(made.step, null);
    assert.deepEqual(
        [3, 1, 2].map((slot) => made.pose(slot).x),
        [0, 400, 800],
    );
});

test("a script given while another plays begins where that one ends", async () => {
    const { stage: made } = stage({ fly: { duration: 600 } });
    made.play(view([1, 2]));
    made.play(view([2, 1]), { beats: [[move(1, 1)]] });
    made.clock.advance(100);
    made.play(view([1, 2]), { beats: [[move(1, 0)]] });
    let idle = false;
    void made.idle().then(() => (idle = true));
    made.clock.advance(800);
    // 900: the second script began at 600, not at the tick that found the first over.
    assert.equal(made.pose(1).x, 200);
    assert.deepEqual(made.step, { index: 0, verb: "fly", from: 600, to: 1200 });
    await Promise.resolve();
    assert.equal(idle, false);
    made.clock.advance(300);
    await Promise.resolve();
    assert.equal(idle, true);
});

test("settle plays everything out and says how long it took", () => {
    const { stage: made } = stage({
        fly: { duration: 600 },
        dwell: { duration: 250 },
    });
    made.play(view([1, 2]));
    assert.equal(made.settle(), 0);
    const dwell = {
        act: { verb: "dwell", subject: { kind: "object", slot: 1 } },
    };
    made.play(view([2, 1]), { beats: [[move(1, 1)], [dwell]] });
    made.play(view([1, 2]), { beats: [[move(1, 0)]] });
    assert.equal(made.settle(), 1450);
    assert.equal(made.clock.now, 1450);
    assert.equal(made.pose(1).x, 0);
    made.play(view([2, 1]), { beats: [[move(1, 1)]] });
    assert.throws(() => made.settle(500), /did not settle within 500 ms/);
});

test("with no durations every step is over in the tick that begins it", () => {
    const { stage: made } = stage();
    made.play(view([1, 2]));
    made.play(view([2, 1]), {
        beats: [[move(1, 1)], [move(2, 1)], [move(1, 1)]],
    });
    assert.equal(made.pose(1).x, 0, "nothing happens outside a tick");
    made.clock.advance(0);
    assert.equal(made.pose(1).x, 400);
    assert.equal(made.step, null);
});

test("a step makes and removes nodes; a reset drops what was playing", () => {
    const { stage: made, surface } = stage({ fly: { duration: 600 } });
    made.play(view([1], [2]));
    surface.log.length = 0;
    const token = { ...object(7), token: true };
    const appear = {
        act: { verb: "appear", from: "nowhere", why: "created", object: token },
    };
    const vanish = {
        act: {
            verb: "vanish",
            subject: { kind: "object", slot: 7 },
            from: "battlefield",
            why: "stateBased",
        },
    };
    const cast = {
        act: {
            verb: "fly",
            from: "hand",
            why: "play",
            object: object(2, "battlefield"),
        },
    };
    made.play(view([1, 2]), { beats: [[appear], [cast], [vanish]] });
    made.clock.advance(300);
    // The token stands second; the card from the hand is half way to the third place.
    assert.deepEqual(surface.log, ["make 7"]);
    assert.equal(made.pose(7).x, 400);
    assert.deepEqual([made.pose(2).x, made.pose(2).y], [400, 250]);
    made.clock.advance(300);
    assert.deepEqual(surface.log, ["make 7", "remove 7"]);
    assert.equal(made.pose(7), null);
    assert.equal(made.pose(2).x, 400);
    // An undone view: nothing of the script still to play survives it, and a card it caught
    // in flight is drawn where the view rests it, even where it was flying to.
    made.play(view([2, 1]), { beats: [[move(1, 1)]] });
    made.clock.advance(100);
    made.play(view([1], [2]), { reset: { abilities: [] }, beats: [] });
    assert.equal(made.step, null);
    assert.deepEqual([made.pose(1).x, made.pose(2).y], [0, 500]);
    made.clock.advance(1000);
    assert.deepEqual([made.pose(1).x, made.pose(2).y], [0, 500]);
    made.play(view([1, 2]));
    made.play(view([2, 1]), { beats: [[move(1, 1)]] });
    made.clock.advance(100);
    assert.notEqual(surface.nodes.get(1).x, 400);
    made.play(view([2, 1]));
    assert.equal(surface.nodes.get(1).x, 400);
    assert.deepEqual(surface.nodes.get(1), made.pose(1));
});

test("those who draw from the stage hear of records and tables, never of motion", () => {
    const { stage: made } = stage({ fly: { duration: 600 } });
    let heard = 0;
    made.subscribe(() => heard++);
    made.play(view([1, 2]));
    assert.equal(heard, 1);
    const before = made.shown.resting.get(2);
    made.play(view([2, 1]), { beats: [[move(1, 1)]] });
    made.clock.advance(16);
    assert.equal(heard, 2);
    for (let i = 0; i < 20; i++) made.clock.advance(16);
    assert.equal(heard, 2);
    // Card 2 moved and nothing about its record or its dress changed: its face is left alone.
    assert.equal(made.shown.resting.get(2).object, before.object);
    assert.equal(made.shown.resting.get(2).dress, before.dress);
    assert.equal(made.shown.table.battlefield.join(), "2,1");
});

test("replaying a step changes the table as the script module says", () => {
    const spell = { kind: "spell", slot: 3, targets: [] };
    const ability = { kind: "ability", name: "Trigger", targets: [] };
    let table = tableOf(view([1, 2], [3]), { abilities: [] });
    const run = (act) => (table = apply(table, act));
    run({
        verb: "fly",
        from: "hand",
        why: "cast",
        object: object(3, "stack"),
        item: spell,
    });
    run({ verb: "emerge", id: 1, item: ability });
    run({ verb: "emerge", id: 2, item: { ...ability, name: "Second" } });
    assert.deepEqual(
        table.view.stack.map((item) => item.name ?? item.slot),
        [3, "Trigger", "Second"],
    );
    run({
        verb: "vanish",
        subject: { kind: "ability", id: 1 },
        from: "stack",
        why: "resolve",
    });
    assert.deepEqual(table.abilities, [2]);
    run({
        verb: "set",
        fact: {
            kind: "item",
            subject: { kind: "ability", id: 2 },
            item: { ...ability, name: "Retargeted" },
        },
    });
    assert.deepEqual(
        table.view.stack.map((item) => item.name ?? item.slot),
        [3, "Retargeted"],
    );
    // A spell resolving to the battlefield leaves the stack with its item; one destroyed
    // lies on top of its owner's graveyard, or under it at 0.
    run({ verb: "fly", from: "stack", why: "resolve", object: object(3) });
    assert.deepEqual(table.view.battlefield, [1, 2, 3]);
    assert.equal(table.view.stack.length, 1);
    run({
        verb: "vanish",
        subject: { kind: "object", slot: 1 },
        from: "battlefield",
        why: "destroy",
        object: object(1, "graveyard"),
    });
    run({
        verb: "vanish",
        subject: { kind: "object", slot: 2 },
        from: "battlefield",
        why: "destroy",
        object: object(2, "graveyard"),
        at: 0,
    });
    assert.deepEqual(table.view.battlefield, [3]);
    assert.deepEqual(table.view.players.you.graveyard, [2, 1]);
    assert.equal(
        table.view.objects.find((o) => o.slot === 1).zone,
        "graveyard",
    );
    run({
        verb: "count",
        tally: { kind: "life", player: "opponent", life: 17 },
    });
    run({
        verb: "set",
        fact: { kind: "turn", number: 2, active: "opponent", step: "upkeep" },
    });
    assert.equal(table.view.players.opponent.life, 17);
    assert.deepEqual(table.view.turn, {
        number: 2,
        active: "opponent",
        step: "upkeep",
    });
    // A token that is no more leaves no record.
    run({ verb: "appear", from: "nowhere", why: "created", object: object(9) });
    run({
        verb: "vanish",
        subject: { kind: "object", slot: 9 },
        from: "battlefield",
        why: "stateBased",
    });
    assert.deepEqual(table.view.battlefield, [3]);
    assert.equal(
        table.view.objects.some((o) => o.slot === 9),
        false,
    );
});
