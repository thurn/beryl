// The stage's light, with no browser (`npm test`): what is lit at a stage time is a function
// of that time and of where the two ends of a line lie (light.ts), an arrow drains over the
// step that ends it and what follows waits for that (spans.ts, beat.ts), and a creature's
// mark of combat begins as the stage first draws it fighting (overlay.ts). How it is painted
// is glow.ts's, which the browser checks see.
import assert from "node:assert/strict";
import { test } from "node:test";
import { lightOf, widthOf } from "../src/stage/light.ts";
import { wayOf } from "../src/stage/lines.ts";
import { move, object, recorder, stage, view } from "./stage-table.mjs";

/** A card of the tests' layout with its corner at `x`, `y`: 100 wide and 140 tall. */
const card = (x, y, toward = -1) => ({
    middle: { x: x + 50, y: y + 70 },
    half: { x: 50, y: 70 },
    round: false,
    toward,
    ahead: 100 / 6,
});
const slot = (n) => ({ kind: "object", slot: n });
const point = (span, arrow, from, to) => ({
    act: { verb: "point", span, arrow, from, to },
});
/** A line as the light is drawn from it, whole since stage time 1000. */
const line = (arrow, from, to, more = {}) => ({
    key: `${arrow} a b`,
    arrow,
    way: wayOf(arrow, from, to),
    from,
    to,
    shown: 1,
    left: 0,
    whole: 1000,
    ...more,
});

test("what is lit at a stage time is that time's alone", () => {
    const [spell, target] = [card(1000, 300), card(0, 0, 1)];
    const lines = [line("hostile-target", spell, target)];
    const fighters = [{ reach: card(0, 500), fights: "attacks", since: 900 }];
    assert.deepEqual(
        lightOf(1234, lines, fighters),
        lightOf(1234, lines, fighters),
    );
    assert.notDeepEqual(
        lightOf(1234, lines, fighters).dots,
        lightOf(1250, lines, fighters).dots,
        "its sparks and motes move with the time",
    );
});

test("an arrow is a ribbon in its relationship's color, drawn as far as it is shown", () => {
    const [spell, target] = [card(1000, 300), card(0, 0, 1)];
    const way = wayOf("friendly-target", spell, target);
    const half = line("friendly-target", spell, target, {
        shown: 0.5,
        whole: 2000,
    });
    const { ribbons, glints, dots } = lightOf(1000, [half], []);
    assert.equal(ribbons.length, 1);
    assert.deepEqual(
        [ribbons[0].tone, ribbons[0].cut, ribbons[0].from, ribbons[0].to],
        ["friendly", "arrow", 0, 0.5],
    );
    assert.deepEqual(ribbons[0].curve, way, "along the line's own way");
    // Nothing lands until it is whole.
    assert.deepEqual([glints, dots], [[], []]);
    const hostile = lightOf(1000, [line("hostile-target", spell, target)], []);
    assert.equal(hostile.ribbons[0].tone, "hostile");
    // As it lands it flashes and throws sparks; long after, only its motes are left.
    const landed = hostile.glints.filter((glint) => glint.kind === "flash");
    assert.ok(landed.some((glint) => glint.gain > 0.8));
    const later = lightOf(9000, [line("hostile-target", spell, target)], []);
    assert.ok(later.dots.length < hostile.dots.length);
    assert.ok(later.ribbons[0].gain < hostile.ribbons[0].gain);
});

test("a line shown from the start, with no time it became whole, is lit and throws nothing", () => {
    const [spell, target] = [card(1000, 300), card(0, 0, 1)];
    const always = line("hostile-target", spell, target, { whole: -Infinity });
    const scene = lightOf(500, [always], []);
    const numbers = [
        ...scene.dots,
        ...scene.ribbons.map((ribbon) => ribbon.gain),
        ...scene.glints.flatMap((glint) => [glint.gain, glint.ring]),
    ];
    assert.ok(numbers.every(Number.isFinite), "every number is one");
    assert.equal(scene.ribbons[0].gain, 1.05);
});

test("an arrow that drains is drawn from where it has drained to", () => {
    const [spell, target] = [card(1000, 300), card(0, 0, 1)];
    const draining = line("hostile-target", spell, target, { left: 0.6 });
    const [ribbon] = lightOf(5000, [draining], []).ribbons;
    assert.deepEqual([ribbon.from, ribbon.to], [0.6, 1]);
    const gone = line("hostile-target", spell, target, { left: 1 });
    assert.deepEqual(lightOf(5000, [gone], []).dots, []);
});

test("the line to a source is a pale thread that grows out of the source", () => {
    const [ability, source] = [card(1000, 300), card(0, 500)];
    const way = wayOf("source", ability, source);
    const [ribbon] = lightOf(
        1000,
        [line("source", ability, source, { shown: 0.4 })],
        [],
    ).ribbons;
    assert.deepEqual([ribbon.tone, ribbon.cut], ["pale", "thread"]);
    assert.deepEqual(ribbon.curve[0], way[2], "from the source's end");
    assert.deepEqual([ribbon.from, ribbon.to], [0, 0.4]);
});

test("a blocker's beam meets the tip of the wedge its attacker wears", () => {
    // The blocker is the opponent's, over your attacker, which faces up the stage.
    const [blocker, attacker] = [card(0, 0, 1), card(0, 500)];
    const [ribbon] = lightOf(
        1000,
        [line("block", blocker, attacker)],
        [],
    ).ribbons;
    assert.deepEqual([ribbon.tone, ribbon.cut], ["gold", "beam"]);
    assert.deepEqual(ribbon.curve[0], { x: 50, y: 140 });
    assert.deepEqual(ribbon.curve[2], { x: 50, y: 500 - 100 / 6 });
});

test("an attacker's wedge pops out of its leading edge, and turns gold once blocked", () => {
    const reach = card(0, 500);
    const wedge = (now, more = {}) =>
        lightOf(now, [], [{ reach, fights: "attacks", since: 1000, ...more }])
            .glints[0];
    assert.equal(wedge(1000).size.y, 0, "nothing of it as it begins");
    assert.ok(wedge(1100).size.y > 100 / 6, "past its height on the way");
    const settled = wedge(1190);
    assert.equal(settled.kind, "wedge");
    assert.deepEqual(settled.at, { x: 50, y: 500 }, "on its leading edge");
    assert.deepEqual(settled.up, { x: 0, y: -1 }, "toward the opponent");
    assert.ok(
        Math.abs(settled.size.y - 100 / 6) < 1e-9,
        "its tip the line's start",
    );
    assert.equal(settled.warm, 0);
    assert.equal(wedge(2080, { blocked: 2000 }).warm, 0.5);
    assert.equal(wedge(3000, { blocked: 2000 }).warm, 1);
    // A layout whose attackers wear no mark draws none (the phone's).
    const bare = { ...reach, ahead: 0 };
    assert.deepEqual(
        lightOf(2000, [], [{ reach: bare, fights: "attacks", since: 0 }])
            .glints,
        [],
    );
});

test("a blocker has a gold crescent under the edge that faces its attacker", () => {
    const [glint] = lightOf(
        2000,
        [],
        [{ reach: card(0, 0, 1), fights: "blocks", since: 1000 }],
    ).glints;
    assert.deepEqual([glint.kind, glint.tone], ["crescent", "gold"]);
    assert.equal(glint.at.x, 50);
    assert.ok(glint.at.y > 140, "below its foot");
    assert.deepEqual(glint.up, { x: 0, y: 1 });
});

test("each cut of ribbon comes to a point where it ends", () => {
    for (const cut of ["arrow", "beam", "thread"]) {
        assert.equal(widthOf(cut, 7, 400, 400), 0, `${cut}'s head`);
        assert.ok(widthOf(cut, 7, 200, 400) > 0, `${cut}'s middle`);
    }
    assert.ok(
        widthOf("arrow", 7, 40, 400) < widthOf("arrow", 7, 340, 400),
        "an arrow swells toward its head",
    );
    assert.ok(
        widthOf("beam", 1, 40, 400) > widthOf("beam", 1, 340, 400),
        "a beam narrows toward its dart",
    );
});

/** As timing.ts has them: a line's end takes 50 ms, with the step before it, and leads. */
const drains = (act) =>
    act.verb === "point"
        ? {
              with: true,
              duration: act.span === "end" ? 50 : 100,
              leads: act.span === "end",
          }
        : { duration: act.verb === "fly" ? 200 : 0 };

test("a line drains over the step that ends it, and what follows waits until it has", () => {
    const { stage: made } = stage(drains);
    const block = (span) => point(span, "block", slot(2), slot(1));
    made.play(view([1, 2]), {
        reset: { abilities: [] },
        beats: [[block("begin")]],
    });
    assert.deepEqual(
        made.lines().map(({ left, whole }) => [left, whole]),
        [[0, -Infinity]],
    );
    const told = [];
    made.follow(({ edge, at, step }) => told.push([edge, at, step.act.verb]));
    made.play(view([2, 1]), { beats: [[block("end"), move(2, 0)]] });
    made.clock.advance(25);
    assert.equal(
        made.lines()[0].left,
        0.25,
        "a quarter gone half way: it quickens",
    );
    made.clock.advance(24);
    assert.equal(made.lines().length, 1);
    made.clock.advance(1);
    assert.deepEqual(made.lines(), []);
    made.settle();
    assert.deepEqual(told, [
        ["start", 0, "point"],
        ["end", 50, "point"],
        ["start", 50, "fly"],
        ["end", 250, "fly"],
    ]);
});

test("a creature's mark of combat begins as the stage first draws it fighting", () => {
    const fighting = (combat) =>
        view([1, 2], [], {
            objects: [object(1, "battlefield", { combat }), object(2)],
        });
    const attacks = { kind: "attacking", planeswalker: null, blocked: false };
    const scenes = [];
    const surface = { ...recorder(), light: (scene) => scenes.push(scene) };
    const { stage: made } = stage();
    made.mount(surface);
    // What fights as the stage starts over has fought since before it: no pop.
    made.play(fighting(attacks));
    assert.equal(scenes.at(-1).glints.length, 1);
    assert.ok(Math.abs(scenes.at(-1).glints[0].size.y - 100 / 6) < 1e-9);
    // One that begins to fight under the stage's eye pops from that tick.
    made.play(view([1, 2]));
    assert.deepEqual(scenes.at(-1).glints, []);
    made.clock.advance(500);
    made.play(fighting(attacks), { beats: [] });
    made.clock.advance(0);
    assert.equal(scenes.at(-1).glints[0].size.y, 0);
    made.clock.advance(1000);
    assert.equal(scenes.at(-1).glints[0].warm, 0);
    // Blocked, it turns gold from the tick that shows it so.
    made.play(fighting({ ...attacks, blocked: true }), { beats: [] });
    made.clock.advance(0);
    made.clock.advance(80);
    assert.equal(scenes.at(-1).glints[0].warm, 0.5);
    // The stage taken off its surface paints no more on it.
    made.unmount(surface);
    assert.equal(scenes.at(-1), undefined);
});
