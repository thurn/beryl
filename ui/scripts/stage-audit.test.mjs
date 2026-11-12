// The stage's audit judging a run (stage-audit.mjs `judge`), with no browser: runs written by
// hand as `walk` returns them, one view whose script has the steps under test. What the page
// measures and how the stage is stepped are held in a browser (verb-audit.mjs).
import assert from "node:assert/strict";
import { test } from "node:test";
import { judge, TICK, TWICE } from "./stage-audit.mjs";

/** A card as the page measures it: painted, on screen, on top, at (`x`, `y`). */
const card = (node, x, y, more = {}) => ({
    node: String(node),
    slot: /^\d+$/.test(String(node)) ? String(node) : undefined,
    stage: 1,
    name: more.name ?? null,
    where: more.zone ?? "battlefield",
    zone: more.zone ?? "battlefield",
    id: more.id ?? Number(String(node).replace(/\D/g, "")) + 100,
    x,
    y,
    w: 100,
    h: 140,
    o: 1,
    on: true,
    top: true,
    ...more,
});

/** Frames a tick apart from 0 to `until` ms, each with the instances `at(t)` gives. */
const frames = (until, at, more = () => ({})) =>
    Array.from({ length: Math.round(until / TICK) + 1 }, (_, n) => ({
        t: n * TICK,
        instances: at(n * TICK),
        lines: [],
        prompt: null,
        ...more(n * TICK),
    }));

const TOKEN = { slot: 7, zone: "battlefield", name: "Spirit", tapped: false };

/** A run of one view whose script is `acts`, each step played over its `[from, to]` if it
 * has one. */
const run = (acts, measured) => ({
    since: 0,
    frames: measured,
    views: [
        {
            at: measured.at(-1).t,
            turn: "turn 1 you precombatMain",
            plays: true,
            names: { 7: "Spirit", 9: "Bear" },
            steps: acts.map(([act]) => ({ act, beat: 0 })),
        },
    ],
    edges: acts.flatMap(([, span], index) =>
        span
            ? ["start", "end"].map((edge, n) => ({
                  view: 0,
                  index,
                  edge,
                  at: span[n],
                  from: span[0],
                  to: span[1],
              }))
            : [],
    ),
});

const verdicts = (findings) =>
    findings.map((f) => `${f.verdict} ${f.kind}: ${f.why}`);

test("a token that appears at its place in a moment is no jump", () => {
    const appear = {
        verb: "appear",
        from: "nowhere",
        why: "created",
        object: TOKEN,
    };
    const found = judge(
        run(
            [[appear, [100, 133]]],
            frames(400, (t) => (t >= 100 ? [card(7, 300, 300)] : [])),
        ),
    );
    assert.deepEqual(verdicts(found), ["ok appear: appeared over 33 ms"]);
    assert.equal(found[0].what, "Spirit nowhere->battlefield (created)");
    assert.equal(found[0].at, 100);
});

test("a finished ability gone between two ticks is no jump", () => {
    const vanish = {
        verb: "vanish",
        subject: { kind: "ability", id: 3 },
        from: "stack",
        why: "resolve",
    };
    const found = judge(
        run(
            [[vanish, [200, 200]]],
            frames(400, (t) => (t < 200 ? [card("a3", 900, 300)] : [])),
        ),
    );
    assert.deepEqual(verdicts(found), ["ok vanish: gone over 0 ms"]);
});

test("a step the stage never played is reported", () => {
    const appear = {
        verb: "appear",
        from: "nowhere",
        why: "created",
        object: TOKEN,
    };
    const tap = { verb: "tap", object: { ...TOKEN, slot: 9, tapped: true } };
    const found = judge(
        run(
            [[appear], [tap, [0, 170]]],
            frames(400, (t) => [
                card(9, 500, 300, { w: 100 + 40 * Math.min(1, t / 170) }),
            ]),
        ),
    );
    assert.equal(
        verdicts(found)[0],
        "still appear: the stage played no such step",
    );
    assert.match(verdicts(found)[1], /^ok tap: moved 40px over [12]\d\d ms$/);
    assert.equal(found[1].minor, true);
});

test("a card that flies is seen to travel, and one that jumps is not", () => {
    const fly = {
        verb: "fly",
        from: "hand",
        why: "play",
        object: { slot: 9, zone: "battlefield", name: "Bear" },
    };
    const glide = (t) => [
        card(9, 100 + Math.min(1, Math.max(0, (t - 50) / 400)) * 600, 300),
    ];
    assert.match(
        verdicts(judge(run([[fly, [50, 450]]], frames(600, glide))))[0],
        /^ok fly: moved 600px over 4\d\d ms$/,
    );
    const jump = (t) => [card(9, t < 450 ? 100 : 700, 300)];
    assert.match(
        verdicts(judge(run([[fly, [50, 450]]], frames(600, jump))))[0],
        /^instant fly: moved 600px in \d\d ms of its 400$/,
    );
    const covered = (t) => glide(t).map((c) => ({ ...c, top: false }));
    assert.match(
        verdicts(judge(run([[fly, [50, 450]]], frames(600, covered))))[0],
        /^unseen fly: visible in 0 of \d+ frames, mostly covered$/,
    );
    // Under the turn's banner it is seen: play goes on beneath an announcement.
    const announced = (t) =>
        glide(t).map((c) => ({ ...c, top: false, cover: ".stage-notice" }));
    assert.match(
        verdicts(judge(run([[fly, [50, 450]]], frames(600, announced))))[0],
        /^ok fly: /,
    );
    const stuck = () => [card(9, 100, 300)];
    assert.deepEqual(
        verdicts(judge(run([[fly, [50, 450]]], frames(600, stuck)))),
        ["still fly: it did not move"],
    );
});

test("an ability whose source is gone appears on the stack", () => {
    const emerge = {
        verb: "emerge",
        id: 3,
        item: { kind: "ability", source: 9, name: "Map" },
    };
    const ability = (t) =>
        t < 100 ? [] : [card("a3", t < 130 ? 300 : 900, t < 130 ? 600 : 300)];
    // Its source sacrificed for its cost, it is on the stack at once.
    assert.deepEqual(
        verdicts(judge(run([[emerge, [100, 400]]], frames(600, ability)))),
        ["ok emerge: appeared over 300 ms"],
    );
    // With its source there to lift out of, the same is a jump.
    const sourced = (t) => [card(9, 300, 600), ...ability(t)];
    assert.match(
        verdicts(judge(run([[emerge, [100, 400]]], frames(600, sourced))))[0],
        /^instant emerge: moved 671px in \d\d ms of its 300$/,
    );
});

test("only the cards of a prompt the data shows twice may show again", () => {
    assert.deepEqual([...TWICE.shapes], ["mode"]);
    const twice = (shape) =>
        judge(
            run(
                [],
                frames(
                    100,
                    () => [
                        card(7, 300, 700, { name: "Spirit", zone: "hand" }),
                        // The chooser's copies: one with the card's slot, one with its
                        // name alone.
                        card(7, 600, 400, { name: "Spirit", id: 2 }),
                        {
                            ...card(7, 800, 400, { name: "Spirit", id: 3 }),
                            node: undefined,
                            slot: undefined,
                        },
                    ],
                    () => ({
                        prompt: shape && {
                            shape,
                            about: [7],
                            names: ["Spirit"],
                        },
                    }),
                ),
            ),
        ).map((f) => f.verdict);
    assert.deepEqual(twice("mode"), []);
    assert.deepEqual(twice("pick"), ["duplicate", "duplicate"]);
    assert.deepEqual(twice(null), ["duplicate", "duplicate"]);
});

test("the face a verb holds up beside its card is no duplicate", () => {
    assert.deepEqual([...TWICE.verbs], ["showcase"]);
    // The card in a hand and its face in the Revealed Cards pile, a node of the stage's own.
    const both = () => [
        card(7, 300, 700, { name: "Spirit", zone: "hand" }),
        {
            ...card("r7", 150, 200, { name: "Spirit", id: 2 }),
            slot: undefined,
        },
    ];
    const found = (twice) =>
        judge(run([], frames(100, both)), twice).map((f) => f.verdict);
    assert.deepEqual(found(TWICE), []);
    assert.deepEqual(found({ shapes: new Set(), verbs: new Set() }), [
        "duplicate",
    ]);
});

test("an arrow that changes shape with its ends still jumped", () => {
    const line = (d, ends) => ({
        key: "s9>7",
        arrow: "hostile-target",
        whole: true,
        d,
        ends,
    });
    const drawn = (lines) =>
        judge(
            run(
                [],
                frames(
                    100,
                    () => [],
                    (t) => ({ lines: [lines(t)] }),
                ),
            ),
        ).map((f) => `${f.verdict}: ${f.why}`);
    assert.deepEqual(
        drawn(() => line("M0 0", "1,1|2,2")),
        [],
    );
    assert.deepEqual(
        drawn((t) => line(t < 50 ? "M0 0" : "M0 9", "1,1|2,2")),
        ["jump: changed shape 1 time with its ends still"],
    );
    // Under what the eye would catch, it did not.
    assert.deepEqual(
        drawn((t) => line(t < 50 ? "M0 0" : "M0 2", "1,1|2,2")),
        [],
    );
    // A line is drawn every moment from where its ends are: one that follows an end is none.
    assert.deepEqual(
        drawn((t) => line(`M0 ${Math.round(t)}`, `1,${Math.round(t)}|2,2`)),
        [],
    );
});
