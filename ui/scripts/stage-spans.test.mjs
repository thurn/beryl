// The stage's arrows, lines and marks, with no browser (`npm test`): the shape of each kind of
// line from how its two ends lie (lines.ts), and what the stage does with a `point` or `mark`
// step (spans.ts): a span is one thing from the step that begins it to the step that ends
// it, across scripts; a line is drawn each tick from where the model has its ends; an arrow
// waits for the spell it leaves to land; an end is a cut. The same in a browser, with the
// design language's timings and looks from both seats, is verbs.mjs.
import assert from "node:assert/strict";
import { test } from "node:test";
import { shapeOf } from "../src/stage/lines.ts";
import { object, reach, recorder, row, stage, view } from "./stage-table.mjs";

/** A card of the tests' layout with its corner at `x`, `y`: 100 wide and 140 tall. */
const card = (x, y, toward = -1) => ({
    middle: { x: x + 50, y: y + 70 },
    half: { x: 50, y: 70 },
    round: false,
    toward,
    ahead: 100 / 6,
});
/** A life total 160 across about `x`, `y`. */
const orb = (x, y) => ({
    middle: { x, y },
    half: { x: 80, y: 80 },
    round: true,
    toward: 0,
    ahead: 0,
});
/** Every number in a path, in order. */
const numbers = (d) => d.match(/-?\d+(\.\d+)?/g).map(Number);
const close = (found, wanted, what) =>
    found.forEach((n, i) =>
        assert.ok(Math.abs(n - wanted[i]) < 0.06, `${what}: ${found}`),
    );
const at = ({ x, y }) => [x, y];

test("a blocker's line runs straight from its middle to its attacker's", () => {
    // Over both cards, as Arena's is (IEXcjBn3BXk 22:23.0): from the blocker below to the
    // middle of the attacker above.
    const shape = shapeOf("block", card(0, 500), card(0, 0, 1), 1);
    assert.deepEqual(shape, {
        d: "M50.0 570.0 L50.0 70.0",
        from: { x: 50, y: 570 },
        to: { x: 50, y: 70 },
    });
    // Half drawn, it has come half way from the blocker.
    assert.equal(
        shapeOf("block", card(0, 500), card(0, 0, 1), 0.5).d,
        "M50.0 570.0 L50.0 320.0",
    );
    assert.equal(shapeOf("block", card(0, 500), card(0, 0, 1), 0), null);
    // A blocker that lies right over its attacker has no line to it.
    assert.equal(shapeOf("block", card(0, 20), card(0, 0, 1), 1), null);
});

test("an attacker's line runs from its middle to the planeswalker it attacks", () => {
    // As Arena's red line to an attacked planeswalker (UrGalw_f35w 48:56.6).
    const walker = shapeOf("attack", card(0, 500), card(400, 0, 1), 1);
    assert.deepEqual(walker.from, { x: 50, y: 570 }, "the attacker's middle");
    assert.deepEqual(walker.to, { x: 450, y: 70 }, "the planeswalker's middle");
});

test("an arrow comes onto its target from the table in front of it, its head on its face", () => {
    // From a spell on the stack 2000 to the right of your permanent, which faces up the
    // stage: out of the spell's near edge, round a point over the target, down onto it.
    const spell = card(2000, 0);
    const target = card(0, 0);
    const shape = shapeOf("hostile-target", spell, target, 1);
    close(at(shape.from), [2000, 64.67], "out of the spell's near edge");
    const [shaft, head] = shape.d.split(" Z ");
    const [, , roundX, roundY] = numbers(shaft);
    close([roundX, roundY], [80, -140], "round a point over its target");
    // Its tip is on the card's face, some way in from the edge that faces the table.
    const [tipX, tipY] = numbers(head);
    close([tipX, tipY], at(shape.to), "the head's tip is the arrow's end");
    assert.ok(tipX > 0 && tipX < 100, `within the card, at ${tipX}`);
    assert.ok(tipY > 14 && tipY < 56, `on its face, at ${tipY}`);
    // Half drawn, its head is on its way, and its ends are the whole arrow's.
    const half = shapeOf("hostile-target", spell, target, 0.5);
    assert.ok(numbers(half.d.split(" Z ")[1])[0] > 500, "on its way");
    assert.deepEqual(half.to, shape.to);
    // The opponent's permanent faces down the stage: the arrow comes up onto it.
    const theirs = shapeOf("hostile-target", spell, card(0, 0, 1), 1);
    assert.ok(numbers(theirs.d)[3] > 140, "round a point under it");
    assert.ok(
        theirs.to.y > 84 && theirs.to.y < 126,
        "onto its face from below",
    );
    // At a player it goes over the top, onto the face of their life total.
    const you = shapeOf("friendly-target", spell, orb(1000, 900), 1);
    const rim = Math.hypot(you.to.x - 1000, you.to.y - 900);
    assert.ok(rim > 30 && rim < 60, `within its rim, ${rim} from its middle`);
});

test("an arrow between two things with little between them goes round their side", () => {
    // A phone's stack lies just under the row its target is in: 30 between the two.
    const shape = shapeOf("hostile-target", card(0, 170), card(0, 0, 1), 1);
    close([shape.from.x], [0], "out of the side of the one");
    assert.ok(shape.from.y > 170, "on its own edge");
    assert.ok(shape.to.x > 0 && shape.to.x < 50, "onto the other's face");
    assert.ok(shape.to.y > 0 && shape.to.y < 140);
    const [, , round] = numbers(shape.d);
    assert.ok(round < -50, `it bows clear of both, to ${round}`);
});

test("an arrow at the stack item under its own comes round to that item's strip", () => {
    // The item on top lies 20 right of and 30 below the one it answers, whose strip shows
    // above it and to its left.
    const shape = shapeOf("hostile-target", card(2020, 30), card(2000, 0), 1);
    assert.deepEqual(shape.from, { x: 2020, y: 65 }, "out of its left side");
    assert.deepEqual(shape.to, { x: 2000, y: 15 }, "into the strip's middle");
    const [, , round] = numbers(shape.d);
    assert.equal(round, 1965, "by way of the table to the left of both");
    // Its head points into the strip from outside it.
    const head = numbers(shape.d.split(" Z ")[1]);
    assert.ok(head[2] < 2000 && head[4] < 2000, "the head lies beside them");
});

test("the line to a source grows out of the source, once the ability is clear of it", () => {
    const ability = card(2000, 0);
    const source = card(0, 0);
    assert.equal(
        shapeOf("source", ability, source, 1).d,
        "M100.0 60.9 Q1050.0 -112.0 2000.0 60.9",
    );
    close(
        numbers(shapeOf("source", ability, source, 0.5).d),
        [100, 60.9, 575, -25.55, 1050, -25.55],
        "half drawn, from the source",
    );
    // An ability that has yet to leave its source has no line to it.
    assert.equal(shapeOf("source", card(10, 10), source, 1), null);
});

/** The lowest point of the curve the path `d` makes from its first point round its second
 * to its third. */
const lowestOf = (d) => {
    const [, ay, , cy, , by] = numbers(d);
    let lowest = -Infinity;
    for (let t = 0; t <= 1; t += 0.001) {
        const y = (1 - t) ** 2 * ay + 2 * t * (1 - t) * cy + t ** 2 * by;
        lowest = Math.max(lowest, y);
    }
    return lowest;
};

test("on the desktop the line to a source far across the table hangs low between them, as Arena's does", () => {
    // Arena's leaves the bottom of the source, sags far below both and comes up into the
    // bottom of the stack card (`0VsYGs88NXA 19:39.85`, `2VW6ZzmS8U4 32:16.0`): on a stage
    // 900 high, from a source 800 across the table from the stack card, to 810, 0.9 of its
    // height, which the layout gives as the lowest it may go.
    const ability = card(1400, 470);
    const source = card(600, 330, 1);
    const shape = shapeOf("source", ability, source, 1, 810);
    assert.deepEqual(
        shape.from,
        { x: 1450, y: 610 },
        "into the ability's bottom",
    );
    assert.deepEqual(
        shape.to,
        { x: 650, y: 470 },
        "out of the source's bottom",
    );
    const lowest = lowestOf(shape.d);
    assert.ok(
        Math.abs(lowest - 810) < 0.5,
        `it sags to 0.9 of the stage's height: ${lowest}`,
    );
    const [, , cx] = numbers(shape.d);
    assert.ok(cx > 650 && cx < 1450, "between them");
});

test("the line to a source on the far row near the stack card sags only a little below it", () => {
    // An opponent's planeswalker on the far row 270 across from the stack card, as in a
    // player's report: a curve that hung to 0.9 of the table swung down to the bottom of the
    // screen and back. Its lowest point lies about 0.32 of 270 below the stack card's bottom, well
    // above the 810 it may sag to.
    const ability = card(1400, 300);
    const source = card(1130, 40, 1);
    const shape = shapeOf("source", ability, source, 1, 810);
    assert.deepEqual(
        shape.from,
        { x: 1450, y: 440 },
        "into the ability's bottom",
    );
    assert.deepEqual(
        shape.to,
        { x: 1180, y: 180 },
        "out of the source's bottom",
    );
    const y = lowestOf(shape.d);
    assert.ok(y > 440 && y <= 530, `just below the stack card: ${y}`);
});

test("the line to a source right under the stack card is a short curve into the source's top", () => {
    // The player's own permanent on their row below the stack card: a curve that hung to 0.9
    // of the table was a tall loop through the buttons below, two lines almost side by side.
    // It goes straight down from the stack card into the top of the source instead.
    const ability = card(1400, 300);
    const source = card(1420, 470);
    const shape = shapeOf("source", ability, source, 1, 810);
    assert.deepEqual(
        shape.from,
        { x: 1450, y: 440 },
        "out of the ability's bottom",
    );
    assert.deepEqual(shape.to, { x: 1470, y: 470 }, "into the source's top");
    const [, ay, , cy, , by] = numbers(shape.d);
    assert.ok(
        Math.max(ay, cy, by) <= 470,
        `no lower than the source's top: ${shape.d}`,
    );
});

/** Whether the path `d`, a curve from its first point round its second to its third,
 * passes over the card `end` anywhere between its own ends. */
const crossesCard = (d, end) => {
    const [ax, ay, cx, cy, bx, by] = numbers(d);
    for (let t = 0.01; t < 0.99; t += 0.01) {
        const x = (1 - t) ** 2 * ax + 2 * t * (1 - t) * cx + t ** 2 * bx;
        const y = (1 - t) ** 2 * ay + 2 * t * (1 - t) * cy + t ** 2 * by;
        if (
            Math.abs(x - end.middle.x) < end.half.x &&
            Math.abs(y - end.middle.y) < end.half.y
        ) {
            return true;
        }
    }
    return false;
};

test("the line to a source on the far row above the stack card never crosses the stack card", () => {
    // The opponent's planeswalker on their row right above the stack card, as in a player's
    // report: its line fell from the planeswalker's bottom straight down across the ability's
    // text and came back up into its bottom. It goes up from the top of the stack card into
    // the bottom of the source instead.
    const ability = card(1400, 300);
    const source = card(1360, 40, 1);
    const shape = shapeOf("source", ability, source, 1, 810);
    assert.deepEqual(
        shape.from,
        { x: 1450, y: 300 },
        "out of the ability's top",
    );
    assert.deepEqual(shape.to, { x: 1410, y: 180 }, "into the source's bottom");
    assert.ok(!crossesCard(shape.d, ability), `off the card: ${shape.d}`);
    // Nor does one from a source above and a little to the side of the stack card, whose
    // hanging curve would cut across the stack card's corner, nor one from either side at any
    // height on the far row.
    for (let x = 1000; x <= 1800; x += 10) {
        for (const y of [0, 40, 100]) {
            const beside = shapeOf("source", ability, card(x, y, 1), 1, 810);
            assert.ok(
                !crossesCard(beside.d, ability),
                `from a source at ${x}, ${y}: ${beside.d}`,
            );
        }
    }
});

const point = (span, arrow, from, to) => ({
    act: { verb: "point", span, arrow, from, to },
});
const mark = (span, name, on) => ({
    act: { verb: "mark", span, mark: name, on },
});
const slot = (n) => ({ kind: "object", slot: n });
const ability = (id) => ({ kind: "ability", id });
const player = (side) => ({ kind: "player", player: side });
const trigger = (source) => ({
    kind: "ability",
    source,
    controller: "you",
    name: "Ability",
    triggered: true,
    targets: [],
});
const emerge = (id, source) => ({
    act: { verb: "emerge", id, item: trigger(source) },
});
const finish = (id) => ({
    act: {
        verb: "vanish",
        subject: ability(id),
        from: "stack",
        why: "resolve",
    },
});
const spell = (n, targets = []) => ({ kind: "spell", slot: n, targets });
const cast = (n, targets) => ({
    act: {
        verb: "fly",
        from: "hand",
        why: "cast",
        object: object(n, "stack"),
        item: spell(n, targets),
    },
});
/** A step that begins or ends an arrow or a mark is played with the step before it, an end
 * is a cut, and an arrow waits for its ends to land, as timing.ts has them. */
const timed =
    (durations) =>
    ({ verb, span, arrow, from, to }) => ({
        duration: span === "end" ? 0 : (durations[verb] ?? 0),
        with: verb === "point",
        after:
            verb === "point" && arrow !== "source"
                ? [from, to].flatMap((end) => (end.slot ? [end.slot] : []))
                : [],
    });

for (const tick of [16, 100, 1000]) {
    test(`a line is one thing from its beginning to its end, across scripts: ticks of ${tick} ms`, () => {
        const { stage: made, surface } = stage(
            timed({ emerge: 400, point: 100, mark: 100 }),
        );
        made.play(view([1, 2]));
        assert.deepEqual(made.lines(), []);
        made.play(view([1, 2], [], { stack: [trigger(1)] }), {
            beats: [
                [emerge(1, 1), point("begin", "source", ability(1), slot(1))],
            ],
        });
        // A second script, of another view, leaves it as it is, and a third ends it.
        made.play(view([1, 2], [], { stack: [trigger(1)] }), {
            beats: [[mark("begin", "eligible", slot(2))]],
        });
        made.play(view([1, 2]), {
            beats: [
                [point("end", "source", ability(1), slot(1))],
                [finish(1), mark("end", "eligible", slot(2))],
            ],
        });
        reach(made.clock, 400, tick);
        if (made.clock.now === 400) {
            // The ability is on the stack, 2000 to the right of its source.
            const [line] = made.lines();
            assert.equal(line.key, "source a1 1");
            assert.deepEqual([line.from, line.to], ["a1", 1]);
            assert.equal(line.shown, 1);
            assert.equal(line.shape.d, surface.lines.get("source a1 1"));
            close(at(line.shape.to), [100, 60.9], "on its source");
            close(at(line.shape.from), [2000, 60.9], "on the ability");
        }
        made.settle();
        assert.deepEqual(made.lines(), []);
        assert.deepEqual(made.marks(), []);
        assert.deepEqual(
            surface.spans,
            ["+ source a1 1", "+ 2 eligible", "- source a1 1", "- 2 eligible"],
            "each was made once and taken away once",
        );
    });
}

test("a line is drawn every tick from where the model has its ends", () => {
    const { stage: made, surface } = stage(timed({ emerge: 400, point: 100 }));
    made.play(view([1]));
    made.play(view([1], [], { stack: [trigger(1)] }), {
        beats: [[emerge(1, 1), point("begin", "source", ability(1), slot(1))]],
    });
    let last = -Infinity;
    made.clock.advance(0);
    for (let now = 0; now <= 400; now += 20) {
        reach(made.clock, now, 20);
        const [line] = made.lines();
        const pose = made.pose("a1");
        const stacked = { dress: { zone: "stack" } };
        const lifted = {
            middle: row.middle(stacked, pose),
            half: row.half(stacked, pose),
            round: false,
            toward: -1,
            ahead: 0,
        };
        // The very shape of an ability at that pose and its source, at rest at 0, 0.
        const shape = shapeOf("source", lifted, card(0, 0), line.shown);
        assert.deepEqual(line.shape, shape, `at ${now} ms`);
        assert.equal(surface.lines.get(line.key), shape?.d ?? null);
        // It stretches as the ability goes: never shorter than it was.
        const far = shape ? shape.from.x - shape.to.x : -Infinity;
        assert.ok(far >= last, `at ${now} ms it is ${far} long after ${last}`);
        last = far;
    }
    assert.ok(last > 1800, "and joins the stack to the source at the last");
});

test("an arrow waits for the spell it leaves to land, and holds up nothing after it", () => {
    const { stage: made } = stage(timed({ fly: 400, point: 100, tap: 50 }));
    made.play(view([1], [2]));
    const tap = { act: { verb: "tap", object: object(1, "battlefield") } };
    made.play(view([1], [], { stack: [spell(2, [])] }), {
        beats: [
            [cast(2), point("begin", "hostile-target", slot(2), slot(1))],
            [point("end", "hostile-target", slot(2), slot(1))],
            [tap],
        ],
    });
    // While the spell flies there is no arrow from it.
    reach(made.clock, 399, 21);
    assert.deepEqual(made.lines(), []);
    // It is drawn out over its own time from the moment the spell lands.
    made.clock.advance(51);
    assert.equal(made.lines()[0].shown, 0.5);
    made.clock.advance(49);
    assert.equal(made.lines()[0].shown, 0.99);
    assert.equal(made.step.verb, "point");
    // Its end is a cut: the step that follows it begins at that very time.
    made.clock.advance(1);
    assert.deepEqual(made.lines(), []);
    const { verb, from } = made.step;
    assert.deepEqual({ verb, from }, { verb: "tap", from: 500 });
});

test("an arrow stands whole for its hold before the step that ends it", () => {
    // An end holds until its arrow has stood 300 ms; the line to a source is not held.
    const held = (act) => ({
        ...timed({ point: 100, tap: 50 })(act),
        ...(act.span === "end" && act.arrow !== "source" ? { hold: 300 } : {}),
    });
    const { stage: made } = stage(held);
    made.play(view([1, 2], [], { stack: [trigger(1)] }), {
        reset: { abilities: [1] },
        beats: [],
    });
    const arrow = (span) => point(span, "hostile-target", ability(1), slot(2));
    const source = (span) => point(span, "source", ability(1), slot(1));
    made.play(view([1, 2]), {
        beats: [
            [source("begin"), arrow("begin")],
            [source("end"), arrow("end")],
            [finish(1)],
        ],
    });
    // Both are whole at 100 ms. The line to the source goes then; the arrow at 400.
    reach(made.clock, 399, 21);
    assert.deepEqual(
        made.lines().map((line) => [line.arrow, line.shown]),
        [["hostile-target", 1]],
    );
    assert.notEqual(made.pose("a1"), null, "what follows waits with it");
    made.clock.advance(1);
    assert.deepEqual(made.lines(), []);
    assert.equal(made.pose("a1"), null);
    // One that has stood longer than that already is cut at once.
    made.play(view([1, 2]), {
        beats: [[point("begin", "block", slot(2), slot(1))]],
    });
    made.settle();
    made.clock.advance(1000);
    made.play(view([1, 2]), {
        beats: [[point("end", "block", slot(2), slot(1))]],
    });
    made.clock.advance(0);
    assert.deepEqual(made.lines(), []);
});

test("a line ends as the step that ends it does, unless it is begun again meanwhile", () => {
    // The end takes 50 ms, and the line is over as it is.
    const slow = (act) => ({
        ...timed({ point: 100 })(act),
        ...(act.span === "end" ? { duration: 50 } : {}),
    });
    const { stage: made, surface } = stage(slow);
    const line = (span) => point(span, "block", slot(2), slot(1));
    const show = { reset: { abilities: [] }, beats: [[line("begin")]] };
    made.play(view([1, 2]), show);
    made.play(view([1, 2]), { beats: [[line("end")]] });
    made.clock.advance(49);
    assert.equal(made.lines().length, 1, "there until its end is over");
    made.clock.advance(1);
    assert.deepEqual(made.lines(), []);
    // Ended and begun in one beat, it is the same line still, and stays.
    made.play(view([1, 2]), show);
    made.play(view([1, 2]), { beats: [[line("end"), line("begin")]] });
    made.settle();
    made.clock.advance(1000);
    assert.equal(made.lines().length, 1);
    assert.deepEqual(surface.spans.slice(-1), ["+ block 2 1"]);
    assert.equal(surface.lines.size, 1);
});

test("a view the stage starts over from shows its spans at once, and drops the last", () => {
    const { stage: made, surface } = stage(timed({ point: 100, mark: 100 }));
    const fight = point("begin", "block", slot(2), slot(1));
    made.play(view([1, 2]), {
        reset: { abilities: [] },
        beats: [[fight, mark("begin", "selected", slot(2))]],
    });
    const [line] = made.lines();
    assert.equal(line.shown, 1);
    assert.equal(surface.lines.get("block 2 1"), line.shape.d);
    assert.deepEqual(made.marks(), [{ mark: "selected", on: 2, shown: 1 }]);
    assert.equal(surface.marks.get("2 selected"), 1);
    made.play(view([1, 2]), { reset: { abilities: [] }, beats: [] });
    assert.deepEqual(made.lines(), []);
    assert.equal(surface.lines.size + surface.marks.size, 0);
});

test("a span goes with what it is on, and a surface mounted later draws those open", () => {
    const { stage: made, surface } = stage(timed({ point: 100 }));
    made.play(view([1, 2, 3]), {
        reset: { abilities: [] },
        beats: [
            [
                point("begin", "block", slot(2), slot(1)),
                point("begin", "attack", slot(3), player("opponent")),
                mark("begin", "eligible", player("you")),
            ],
        ],
    });
    assert.equal(surface.lines.size, 2);
    made.unmount();
    assert.equal(surface.lines.size + surface.marks.size, 0, "taken with it");
    const other = recorder();
    made.mount(other);
    assert.deepEqual(
        [...other.lines.keys()],
        ["block 2 1", "attack 3 opponent"],
    );
    assert.deepEqual([...other.marks], [["you eligible", 1]]);
    // The blocker is no more, by a step that says nothing of its line.
    made.play(view([1, 3]), {
        beats: [
            [
                {
                    act: {
                        verb: "vanish",
                        subject: slot(2),
                        from: "battlefield",
                        why: "sacrifice",
                    },
                },
            ],
        ],
    });
    made.settle();
    assert.deepEqual([...other.lines.keys()], ["attack 3 opponent"]);
});

test("the line to a source is hidden while the player is asked something", () => {
    const { stage: made, surface } = stage(timed({}));
    const asked = (kind) => ({ kind: "deciding", decision: { kind } });
    const stack = { stack: [trigger(1)] };
    made.play(view([1], [], { ...stack, status: asked("priority") }), {
        reset: { abilities: [1] },
        beats: [[point("begin", "source", ability(1), slot(1))]],
    });
    assert.notEqual(made.lines()[0].shape, null, "shown with priority");
    made.play(view([1], [], { ...stack, status: asked("target") }), {
        beats: [],
    });
    made.settle();
    const [line] = made.lines();
    assert.deepEqual([line.hidden, line.shape], [true, null]);
    assert.equal(
        surface.lines.get(line.key),
        null,
        "its element draws nothing",
    );
    made.play(view([1], [], { ...stack, status: asked("priority") }), {
        beats: [],
    });
    made.settle();
    assert.equal(made.lines()[0].hidden, false);
    assert.deepEqual(
        surface.spans,
        ["+ source a1 1"],
        "one element throughout",
    );
});

test("an ability whose resolution asks something stays on the stage, its line with it", () => {
    const { stage: made, surface } = stage(timed({}));
    const item = trigger(1);
    made.play(view([1], [], { stack: [item] }), {
        reset: { abilities: [1] },
        beats: [[point("begin", "source", ability(1), slot(1))]],
    });
    // The view that asks leaves the ability out of its stack and says it is resolving: on
    // a script's table it is on the stack still (script/table.rs `Table::of`).
    const asks = { kind: "yesNo", resolving: item };
    const asking = view([1], [], {
        stack: [],
        status: { kind: "deciding", decision: asks },
    });
    made.play(asking, { beats: [] });
    made.settle();
    assert.notEqual(made.pose("a1"), null, "the ability is on the stage");
    assert.deepEqual(
        made.lines().map((line) => [line.key, line.hidden]),
        [["source a1 1", true]],
    );
    made.play(view([1]), {
        beats: [[point("end", "source", ability(1), slot(1))], [finish(1)]],
    });
    made.settle();
    assert.equal(made.pose("a1"), null);
    assert.deepEqual(surface.spans, ["+ source a1 1", "- source a1 1"]);
});

test("a permanent picked to pay turns as it would tap, and back when it is not", () => {
    const { stage: made, surface } = stage(timed({ mark: 100 }));
    const table = view([1, 2], [], {
        objects: [object(1), object(2, "battlefield", { tapped: true })],
    });
    made.play(table);
    made.play(table, {
        beats: [
            [
                mark("begin", "paying", slot(1)),
                mark("begin", "paying", slot(2)),
            ],
        ],
    });
    reach(made.clock, 50, 10);
    assert.equal(made.pose(1).turn, 45);
    assert.equal(surface.nodes.get(1).turn, 45, "and is drawn so");
    assert.equal(surface.marks.get("1 paying"), 0.5);
    made.settle();
    assert.equal(made.pose(1).turn, 90);
    assert.equal(surface.nodes.get(1).turn, 90);
    assert.equal(made.pose(1).depth, 201, "over the card beside it");
    assert.equal(made.pose(2).turn, 0, "one that is tapped lies as it did");
    made.play(table, { beats: [[mark("end", "paying", slot(1))]] });
    made.settle();
    assert.equal(made.pose(1).turn, 0);
    assert.equal(made.pose(1).depth, 1);
    assert.equal(surface.nodes.get(1).turn, 0);
    assert.equal(surface.marks.has("1 paying"), false);
});
