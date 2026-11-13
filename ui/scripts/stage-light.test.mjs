// The stage's light, with no browser (`npm test`): what is lit at a stage time is a function
// of that time and of where the two ends of a line lie (light.ts), an arrow drains over the
// step that ends it and what follows waits for that (spans.ts, beat.ts), and a creature's
// mark of combat begins as the stage first draws it fighting (overlay.ts). How it is painted
// is glow.ts's, which the browser checks see.
import assert from "node:assert/strict";
import { test } from "node:test";
import { flaresOf, SETTLE, STREAK } from "../src/stage/flare.ts";
import { lightOf as layered, sought, widthOf } from "../src/stage/light.ts";
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
/** Everything lit, whichever layer it is painted in. */
const lightOf = (...given) => {
    const { under, over } = layered(...given);
    return {
        ribbons: [...under.ribbons, ...over.ribbons],
        glints: [...under.glints, ...over.glints],
        dots: [...under.dots, ...over.dots],
    };
};
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

test("the line to a source is a pale swell that grows out of the source", () => {
    const [ability, source] = [card(1000, 300), card(0, 500)];
    const way = wayOf("source", ability, source);
    const [ribbon] = lightOf(
        1000,
        [line("source", ability, source, { shown: 0.4 })],
        [],
    ).ribbons;
    assert.deepEqual([ribbon.tone, ribbon.cut], ["pale", "swell"]);
    assert.deepEqual(ribbon.curve[0], way[2], "from the source's end");
    assert.deepEqual([ribbon.from, ribbon.to], [0, 0.4]);
    // As Arena's: a hairline at the source, widest a little past half way, gone before the
    // stack card.
    const at = (share) => widthOf("swell", 3, share * 1000, 1000);
    assert.ok(
        at(0.03) < 0.5 * at(0.6),
        "a hairline where it leaves the source",
    );
    assert.ok(at(0.6) >= at(0.4) && at(0.6) >= at(0.8), "widest past half way");
    assert.equal(at(1), 0, "faded out at the ability");
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
    // Whole in the frame it begins, a little wide and tall, as Arena's (22:16.53).
    const first = wedge(1000);
    assert.ok(first.size.y > 100 / 6 && first.size.x > 50, "whole, and large");
    assert.ok(first.gain > wedge(1100).gain, "and bright, which settles");
    const settled = wedge(2000);
    assert.equal(settled.kind, "wedge");
    assert.deepEqual(settled.at, { x: 50, y: 500 }, "on its leading edge");
    assert.deepEqual(settled.up, { x: 0, y: -1 }, "toward the opponent");
    assert.ok(
        Math.abs(settled.size.y - 100 / 6) < 1e-6,
        "its tip the line's start",
    );
    assert.ok(Math.abs(settled.size.x - 50) < 1e-6, "as wide as its card");
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
    // A dome about a fifth of its width tall, a little clear of its edge (Arena 22:23.0).
    assert.ok(Math.abs(glint.at.y - (140 + 0.16 * 50)) < 1e-9);
    assert.ok(Math.abs(glint.size.y - 0.36 * 50) < 1e-9);
    assert.deepEqual(glint.up, { x: 0, y: 1 });
});

test("combat is lit under the cards, and an arrow and a source's line over them", () => {
    const [spell, target] = [card(1000, 300), card(0, 0, 1)];
    const [blocker, attacker] = [card(400, 0, 1), card(400, 500)];
    const { under, over } = layered(
        1000,
        [
            line("hostile-target", spell, target),
            line("source", spell, attacker),
            line("block", blocker, attacker),
            line("attack", attacker, target),
        ],
        [
            { reach: attacker, fights: "attacks", since: 1000 },
            { reach: blocker, fights: "blocks", since: 1000 },
        ],
    );
    assert.deepEqual(
        over.ribbons.map((ribbon) => ribbon.cut),
        ["arrow", "swell"],
    );
    assert.deepEqual(
        under.ribbons.map((ribbon) => ribbon.tone),
        ["gold", "ember"],
    );
    assert.deepEqual(
        under.glints.map((glint) => glint.kind).filter((k) => k !== "flash"),
        ["wedge", "crescent"],
    );
    assert.ok(over.glints.every((glint) => glint.kind === "flash"));
});

test("the arrow a choice of targets pulls runs to the pointer, thin and pale", () => {
    const spell = card(1000, 300);
    const { ribbons, glints, dots } = sought(5000, spell, { x: 300, y: 100 });
    assert.equal(ribbons.length, 1);
    const [ribbon] = ribbons;
    assert.deepEqual([ribbon.tone, ribbon.cut], ["pale", "arrow"]);
    assert.deepEqual(
        ribbon.curve[2],
        { x: 300, y: 100 },
        "to the pointer itself",
    );
    assert.equal(ribbon.curve[0].x, 1000, "out of the edge that faces it");
    assert.deepEqual([glints, dots], [[], []]);
    // Nothing while the pointer is on what asks.
    assert.deepEqual(sought(5000, spell, { x: 1050, y: 370 }).ribbons, []);
});

test("on what may be chosen it becomes the arrow the choice will draw", () => {
    const [spell, target] = [card(1000, 300), card(0, 0, 1)];
    const lock = (since, friendly = false) => ({ to: target, friendly, since });
    const at = { x: 20, y: 30 };
    // As the pointer comes onto it the arrow is still the pointer's, and then swings.
    const [first] = sought(5000, spell, at, lock(5000)).ribbons;
    assert.deepEqual(first.curve[2], at);
    assert.equal(first.tone, "hostile");
    const landed = sought(9000, spell, at, lock(5000));
    const way = wayOf("hostile-target", spell, target);
    landed.ribbons[0].curve.forEach((p, i) => {
        assert.ok(Math.hypot(p.x - way[i].x, p.y - way[i].y) < 1e-6);
    });
    assert.ok(landed.ribbons[0].wide > first.wide, "and swells to its width");
    assert.equal(
        sought(9000, spell, at, lock(5000, true)).ribbons[0].tone,
        "friendly",
    );
    // It lands with a flash and sparks, which are gone a moment later.
    const fresh = sought(5040, spell, at, lock(5000));
    assert.ok(fresh.glints[0].gain > 0.5);
    assert.ok(fresh.dots.length > landed.dots.length);
    assert.deepEqual(
        sought(5040, spell, at, lock(5000)),
        fresh,
        "the same at the same stage time",
    );
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
    assert.equal(scenes.at(-1).under.glints.length, 1);
    assert.ok(Math.abs(scenes.at(-1).under.glints[0].size.y - 100 / 6) < 1e-6);
    // One that begins to fight under the stage's eye pops from that tick.
    made.play(view([1, 2]));
    assert.deepEqual(scenes.at(-1).under.glints, []);
    made.clock.advance(500);
    made.play(fighting(attacks), { beats: [] });
    made.clock.advance(0);
    assert.ok(scenes.at(-1).under.glints[0].size.y > 100 / 6 + 1);
    made.clock.advance(1000);
    assert.equal(scenes.at(-1).under.glints[0].warm, 0);
    // Blocked, it turns gold from the tick that shows it so.
    made.play(fighting({ ...attacks, blocked: true }), { beats: [] });
    made.clock.advance(0);
    made.clock.advance(80);
    assert.equal(scenes.at(-1).under.glints[0].warm, 0.5);
    // The stage taken off its surface paints no more on it.
    made.unmount(surface);
    assert.equal(scenes.at(-1), undefined);
});

test("mana in flight is a ribbon of its color, and what lands bursts and is gone", () => {
    const dark = () => ({
        under: { now: 0, ribbons: [], glints: [], dots: [] },
        over: { now: 0, ribbons: [], glints: [], dots: [] },
    });
    const curve = [
        { x: 100, y: 600 },
        { x: 500, y: 300 },
        { x: 1200, y: 400 },
    ];
    const streak = (look, through) => ({
        key: `${look} 1`,
        look,
        curve,
        back: through / 2,
        through,
        wide: 5,
    });
    const flown = (look, through) =>
        flaresOf(dark(), [streak(look, through)], []).over;
    // A ribbon from its tail to its head, in its mana's color, and nothing under the cards.
    const early = flown("blue", 0.2);
    assert.deepEqual(
        early.ribbons.map(({ tone, from, to, wide }) => [tone, from, to, wide]),
        [["blue", 0.1, 0.2, 5 * STREAK.ribbon]],
    );
    assert.equal(flown("damage", 0.2).ribbons[0].tone, "ember");
    assert.deepEqual(
        flaresOf(dark(), [streak("blue", 0.2)], []).under.ribbons,
        [],
    );
    // What it left glows where it lay, less as the head goes on.
    const [orb] = early.glints;
    assert.deepEqual([orb.kind, orb.at], ["flash", curve[0]]);
    assert.ok(flown("blue", 0.9).glints[0].gain < orb.gain / 2);
    // The same streak at the same share of its way is the same light.
    assert.deepEqual(flown("blue", 0.2), early);

    const reach = card(400, 300);
    const landed = (name, age, span = 470) =>
        flaresOf(dark(), [], [{ name, reach, age, span }]).over;
    const burst = (age) => landed("settle", age).glints[0];
    // A permanent's burst is about its middle. It is dark for the two frames the card lies
    // whole, brightest `peak` of its time through and gone past `bloomed` of it, when only
    // its flecks are left.
    assert.deepEqual([burst(0).kind, burst(0).at], ["burst", reach.middle]);
    assert.deepEqual([burst(0).gain, burst(66).gain], [0, 0]);
    const brightest = burst(470 * SETTLE.peak);
    assert.ok(brightest.gain > burst(120).gain && burst(120).gain > 0);
    assert.ok(brightest.gain > burst(250).gain);
    assert.equal(burst(470 * SETTLE.bloomed).gain, 0);
    assert.ok(brightest.size.x > burst(120).size.x, "it swells");
    assert.equal(landed("settle", 100).dots.length, 0);
    assert.ok(landed("settle", 300).dots.length > 0, "it throws flecks");
    // Mana landing on a spell flares at once and fades.
    const flare = (age) => landed("flash", age, 330).glints[0];
    assert.ok(flare(0).gain > flare(150).gain && flare(150).gain > 0);
    assert.ok(flare(150).size.x > flare(0).size.x, "it spreads");
    // A triggered ability come to rest shoots a beam toward the middle of the table, out of
    // its left side a little above its middle, as Arena's does (`2VW6ZzmS8U4 32:15.43`): long
    // after a quarter of its time, then a thin streak, then gone.
    const beam = (age) =>
        landed("beam", age, 300).glints.find((g) => g.kind === "beam");
    assert.deepEqual(beam(30).up, { x: -1, y: 0 }, "toward the middle");
    assert.equal(beam(30).at.x, reach.middle.x - reach.half.x, "from its side");
    assert.ok(beam(30).at.y < reach.middle.y, "above its middle");
    assert.ok(beam(90).size.y >= 3 * reach.half.x, "nearly two cards long");
    assert.ok(beam(240).size.x < beam(60).size.x / 2, "it thins to a streak");
    assert.ok(beam(60).gain > beam(240).gain && beam(299).gain < 0.01);
    assert.ok(landed("beam", 60, 300).dots.length > 0, "with sparks behind it");
});
