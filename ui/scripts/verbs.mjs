// The verb suite (plan K24 and K25): every verb the stage plays, checked from both seats on
// the stage's own clock. `by ui-test` serves the UI and runs this beside interactions.mjs,
// which the cut-over retires. Usage:
//   node scripts/verbs.mjs <base-url> <fixtures-dir> [check...]
// A check is `verb:<verb>/<seat>/<layout>` (ui/verbs.json lists them), named whole or in
// part to run it alone, and logged by that name. Each opens its moments' fixtures with `?stage`
// in its layout's windows (the desktop's, or each phone size ui/shots.json has with `?phone`),
// holds the stage's clock, plays the recording on as its seat did, and steps the clock inside
// one `page.evaluate`, reading `stage.step` and `stage.pose`: no check waits on the page's
// time, so none depends on the machine's speed. It plays the moment in ticks of 16 ms and of
// 100 ms, which must read the same. Every run also holds the page's sounds to its steps: each
// asked for at the stage time its step begins or ends, and none otherwise. What each verb's
// check then asks of its runs is its cases: verb-cases.mjs for cards changing zone, and
// verb-cases-place.mjs for changes on a card where it lies and to a player. Exits non-zero if
// a check fails or a page logs an error.
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { basename, join } from "node:path";
import {
    coverage,
    launch,
    recordResult,
    stageFaults,
    workers,
} from "./capture.mjs";
import { ending as endOf } from "./fixture-views.mjs";
import { CHECK_SECONDS, HangError, hangGuard } from "./hang.mjs";
import { CASES as ZONES, complete } from "./verb-cases.mjs";
import { CASES as PLACE } from "./verb-cases-place.mjs";
import { CASES as PILES } from "./verb-cases-piles.mjs";

const [base, fixtures, ...filters] = process.argv.slice(2);
if (!base || !fixtures) {
    console.error(
        "usage: node scripts/verbs.mjs <base-url> <fixtures-dir> [check...]",
    );
    process.exit(2);
}
const { verbs, layouts, moment, more, watches } = JSON.parse(
    await readFile(new URL("../verbs.json", import.meta.url), "utf8"),
);
/** The checks the fast check chose (`by ui-test --select`, tools' selection.py), from the file
 * `BERYL_CHECKS` names, one a line; null to run every check the filters reach. */
const chosen = process.env.BERYL_CHECKS
    ? new Set((await readFile(process.env.BERYL_CHECKS, "utf8")).split("\n"))
    : null;
const wanted = (check) =>
    (filters.length === 0 || filters.some((f) => check.includes(f))) &&
    (!chosen || chosen.has(check));

/** The sizes of tick every check plays the moment in, at its layout's first window; at the
 * others the last alone, the stage's time being no layout's. */
const TICKS = [16, 100];
const { shots } = JSON.parse(
    await readFile(new URL("../shots.json", import.meta.url), "utf8"),
);
/** Each window size the shots take of a route `asks` picks out, once. */
const windows = (asks) => [
    ...new Map(
        shots
            .filter((shot) => asks(new URL(shot.route, base).searchParams))
            .map(({ width, height }) => [
                `${width}x${height}`,
                { width, height },
            ]),
    ).values(),
];
/** What each layout of the stage is checked at and by: the query that asks for it, its
 * windows, and whether its battlefield is laid out again as the stack comes and goes and as a
 * row fills. A phone's is (src/mobile/layout.ts): its middle band grows for the stack, a row's
 * cards shrink to fit, and identical permanents stack as one. So there a lift's source and a
 * new token's neighbours glide to their new places meanwhile, where the desktop's stay. */
const LAYOUTS = {
    desktop: {
        query: "stage",
        windows: [{ width: 1600, height: 900 }],
        reflows: false,
    },
    phone: {
        query: "phone&stage",
        windows: windows((query) => query.has("phone")),
        reflows: true,
    },
};
/** How long the stage stands idle between a seat's answers, in stage ms. */
const GAP = 250;
/** The most answers a moment takes: the cast, the resolution and the trigger's, with room. */
const ANSWERS = 6;
/** The desktop stage's height, whose upper half is the opponent's (src/game/layout.ts STAGE).
 * A phone's halves meet in its middle band. */
const MIDDLE = 450;
/** Plays the moment on the page's stage, in the page: holds the stage's clock, answers as the
 * recording's seat did (the fixture transport plays the recorded views on at each `choose`,
 * src/ipc.ts) and walks the clock from step to step in ticks of `tick`, until an ability has
 * come onto the stack and gone, or with `whole`, for all of its `answers`: to the recording's
 * end, which the view the stage then shows tells (`shows`). Returns each step `stage.step` showed, in order, with every
 * node's pose by `stage.pose` before it, as it began, half way and as it ended, where a tick
 * landed there; and how the page's nodes differed from the model at any of those. A step that
 * takes no time shows only in what the samples around it hold. Also every edge of a step the
 * stage told (`stage.follow`) with the sounds that step asks for (src/audio/steps.ts), and
 * the sounds the page was asked for meanwhile (`window.berylSounds`), all in stage time; and
 * where the two halves of the table meet. */
async function walk({ tick, gap, answers, middle, whole }) {
    const stage = window.berylStage;
    const { clock } = stage;
    clock.hold();
    // Whole milliseconds from here on, so that two runs give the same numbers to the bit.
    while (!Number.isInteger(clock.now)) {
        clock.advance(Math.ceil(clock.now) - clock.now);
    }
    const origin = clock.now;
    // The times to sample besides: the middle and the end of every step the stage tells of as
    // it begins, so that a step that plays while an earlier one of its beat does is seen too.
    const wanted = new Set();
    const extra = [];
    let sample;
    let changes;
    const reach = async (time) => {
        while (clock.now < time) {
            const next = Math.min(
                time,
                clock.now + tick,
                ...[...wanted].filter((at) => at > clock.now),
            );
            clock.advance(next - clock.now);
            if (wanted.delete(clock.now) && clock.now < time) {
                const taken = sample();
                if (taken.counting.length) taken.changes = await changes();
                extra.push(taken);
            }
        }
    };
    const layer = document.querySelector(".card-layer");
    const faults = [];
    const heard = window.berylSounds.length;
    const edges = [];
    const unfollow = stage.follow((edge) => {
        edges.push({
            edge: edge.edge,
            at: edge.at - origin,
            verb: edge.step.act.verb,
            act: edge.step.act,
            cues: window.berylStepSounds(edge),
        });
        if (edge.edge !== "start") return;
        const { act } = edge.step;
        const { duration } = stage.timings(act, edge.table.view);
        if (duration > 0) {
            wanted.add(edge.at + duration / 2).add(edge.at + duration);
        }
    });
    const near = (a, b, by) => Math.abs(a - b) <= by;
    /** Why the page's node for `name` is not where the model has it, if it is not. */
    const differs = (name, pose, frame, down) => {
        const node = layer.querySelector(`[data-stage="${name}"]`);
        if (!node) return "has no node";
        const written = node.style.transform.match(/-?\d+(\.\d+)?(e-?\d+)?/g);
        const [x, y, scale, rotation, skew] = (written ?? []).map(Number);
        if (
            !near(x, pose.x, 0.01) ||
            !near(y, pose.y, 0.01) ||
            !near(scale, pose.width / frame, 0.0001) ||
            !near(rotation, pose.rotation, 0.01) ||
            !near(skew, pose.skew, 0.01)
        ) {
            return `is drawn at ${node.style.transform}`;
        }
        if (!near(Number(node.style.opacity), pose.opacity, 0.0001)) {
            return `is drawn at opacity ${node.style.opacity}`;
        }
        if (Number(node.style.zIndex) !== Math.round(pose.depth)) {
            return `is drawn at depth ${node.style.zIndex}`;
        }
        // Its card's own turn inside it (src/stage/pose.ts turnOf).
        const turn = node.style.getPropertyValue("--turn");
        if (turn !== (pose.turn ? `${pose.turn}deg` : "")) {
            return `has its card turned ${turn || "not at all"}`;
        }
        // And the browser draws it so: the stage's rule reaches the card (stage.css).
        const card = pose.turn && node.querySelector(".card-slot > .card");
        if (card) {
            const drawn = parseFloat(getComputedStyle(card).rotate);
            if (!near(drawn, pose.turn, 0.01)) {
                return `has its card drawn turned ${drawn} of ${pose.turn}`;
            }
        }
        // A node dressed with its card's name shows its face (src/stage/dom.ts), but for a
        // face-down permanent its controller knows, which is named and shows its back.
        if (pose.face === "back" && node.dataset.card && !down) {
            return `shows the face of ${node.dataset.card}`;
        }
        // Where the browser put it: an upright node's corner is the pose's.
        if (pose.rotation === 0 && pose.skew === 0) {
            const table = layer.getBoundingClientRect();
            const box = node.getBoundingClientRect();
            const by = table.width / layer.offsetWidth;
            if (
                !near(box.left, table.left + pose.x * by, 0.5) ||
                !near(box.top, table.top + pose.y * by, 0.5)
            ) {
                return `lies at ${box.left}, ${box.top}`;
            }
        }
        return null;
    };
    let lifted = false;
    let done = false;
    sample = () => {
        const poses = {};
        const zones = {};
        const marks = {};
        const { resting, table, counts } = stage.shown;
        for (const [name, { dress, object }] of resting) {
            const pose = stage.pose(name);
            poses[name] = pose;
            zones[name] = dress.zone;
            // What a permanent's face is drawn from, where a step changes it in place.
            if (object && dress.zone === "battlefield") {
                marks[name] = {
                    tapped: object.tapped,
                    named: object.name !== null && object.facing !== "down",
                    controller: object.controller,
                    attachedTo: object.attachedTo ?? null,
                    counters: object.counters,
                    pt: object.pt ?? null,
                };
            }
            const down = object?.facing === "down";
            const why = differs(name, pose, dress.frame, down);
            if (why) faults.push(`at ${clock.now - origin} ms ${name} ${why}`);
        }
        const nodes = layer.querySelectorAll(".stage-node").length;
        if (nodes !== resting.size) {
            faults.push(
                `at ${clock.now - origin} ms the page has ${nodes} nodes for ${resting.size}`,
            );
        }
        const abilities = [...resting.keys()].some(
            (n) => typeof n === "string",
        );
        done ||= !whole && lifted && !abilities;
        lifted ||= abilities;
        // The life totals the table has, and the life counts under way with what each found.
        const life = {
            you: table.players.you.life,
            opponent: table.players.opponent.life,
        };
        const counting = counts.flatMap(({ step, found }) => {
            const { tally } = step.act;
            if (tally.kind !== "life") return [];
            const was = found.view.players[tally.player].life;
            return [{ player: tally.player, life: tally.life, was }];
        });
        return { at: clock.now - origin, poses, zones, marks, life, counting };
    };
    /** What the page shows beside each life total of how it is changing, once React has drawn
     * what the stage told it. */
    changes = async () => {
        await new Promise((resolve) => setTimeout(resolve));
        return [...document.querySelectorAll("[data-life-change]")].map(
            (shown) => [
                shown.closest("[data-life]")?.dataset.life,
                Number(shown.dataset.lifeChange),
            ],
        );
    };
    const steps = [];
    for (let answer = 0; answer < answers && !done; answer++) {
        await window.berylFixture.invoke("choose", { request: {} });
        // The transport hands the views on in a microtask.
        await new Promise((resolve) => setTimeout(resolve));
        while (!done) {
            const before = sample();
            clock.advance(0);
            const step = stage.step;
            if (!step) break;
            const { index, verb, from, to } = step;
            const at = {};
            if (clock.now < from) await reach(from);
            if (clock.now === from) at.from = sample();
            const middle = (from + to) / 2;
            if (clock.now < middle) {
                await reach(middle);
                at.middle = sample();
                if (at.middle.counting.length) {
                    at.middle.changes = await changes();
                }
            }
            await reach(to);
            at.to = sample();
            steps.push({
                answer,
                index,
                verb,
                act: step.step.act,
                from: from - origin,
                to: to - origin,
                before,
                at,
            });
        }
        if (!done) await reach(clock.now + gap);
    }
    unfollow();
    const shown = stage.shown.view;
    const band = document
        .querySelector(".phone-middle")
        ?.getBoundingClientRect();
    return {
        steps,
        extra,
        faults,
        done: done || (whole && !stage.shown.busy),
        shows: shown && [shown.turn.number, shown.turn.step, shown.status],
        edges,
        sounds: window.berylSounds
            .slice(heard)
            .map(({ sound, at, delay }) => ({ sound, at: at - origin, delay })),
        middle: band
            ? (band.top + band.bottom) / 2 - layer.getBoundingClientRect().top
            : middle,
        // What the old playback and card layer leave on a page they run on.
        old: {
            shown: window.berylShown?.length ?? 0,
            cards: document.querySelectorAll(".card-el:not(.stage-node)")
                .length,
        },
    };
}

/** The views a recorded fixture shows, in order. */
async function views(fixture) {
    const { messages } = JSON.parse(
        await readFile(join(fixtures, `${fixture}.json`), "utf8"),
    );
    return messages
        .filter((m) => m.kind === "event")
        .map((m) => m.payload.view);
}

/** The steps of a recorded fixture's scripts after its first view, in order. */
async function recorded(fixture) {
    return (await views(fixture))
        .slice(1)
        .flatMap((view) => view.script.beats.flat())
        .map((step) => step.act);
}

/** What a recorded fixture ends on, as `walk` tells what the stage shows (`shows`), and how
 * many answers of its seat lead there (fixture-views.mjs). */
async function ending(fixture) {
    return endOf(
        JSON.parse(await readFile(join(fixtures, `${fixture}.json`), "utf8")),
    );
}

/** Every sound `run`'s page was asked for is a step's, at the stage time that step begins or
 * ends as the step says, with no delay; and every sound a step asks for was asked for then,
 * once however many steps ask at one time. */
function soundsFollowSteps(run) {
    const asked = [];
    for (const { edge, at, cues } of run.edges) {
        for (const cue of cues) {
            const same = (s) => s.sound === cue.sound && s.at === at;
            if (cue.edge === edge && !asked.some(same)) {
                asked.push({ sound: cue.sound, at, delay: 0 });
            }
        }
    }
    assert.deepEqual(run.sounds, asked, "the sounds against the steps");
}

/** What each verb's check asks of its runs: each of its cases there is. */
const CASES = Object.fromEntries(
    verbs.map((verb) => [
        verb,
        (runs) =>
            [ZONES, PLACE, PILES].forEach((cases) => cases[verb]?.(runs)),
    ]),
);
assert.deepEqual(
    [...new Set([...Object.keys(ZONES), ...Object.keys(PLACE)])].sort(),
    [...verbs].sort(),
    "ui/verbs.json and the cases",
);
assert.deepEqual(
    layouts.filter((layout) => !LAYOUTS[layout]?.windows.length),
    [],
    "ui/verbs.json's layouts and their windows",
);

const browser = await launch();
const queue = workers(6);
const errors = [];
let failed = false;
let ran = 0;

/** A context of windows of `size`, whose pages fetch the fixtures this run recorded, not what
 * Vite serves. */
async function open(size) {
    const context = await browser.newContext({
        viewport: size,
        deviceScaleFactor: 1,
    });
    await context.route("**/fixtures/*.json", (route) =>
        route.fulfill({
            path: join(
                fixtures,
                decodeURIComponent(
                    basename(new URL(route.request().url()).pathname),
                ),
            ),
        }),
    );
    return context;
}

/** Plays `fixture` from its first view on the stage of a page of its own in `context`, asked
 * for by `query`, which `watch` is shown first and returns what to do before the page closes,
 * in ticks of `tick`, to its `end` if given one (`ending`); and checks what holds of every run: the page drew what the model said,
 * nothing but the stage's clock moved a node, its sounds were its steps', and the old
 * playback and card layer took no part. */
async function play(context, watch, query, fixture, tick, end) {
    const page = await context.newPage();
    const done = await watch(page);
    let hung = false;
    const playOut = async () => {
        await page.goto(
            new URL(`/?${query}&fixture=${fixture}&at=1#/game`, base).href,
        );
        await page.waitForFunction(
            () => window.berylStage && document.querySelector(".stage-node"),
        );
        await page.evaluate(() => document.fonts.ready.then(() => {}));
        const run = await page.evaluate(walk, {
            tick,
            gap: GAP,
            answers: end?.answers ?? ANSWERS,
            middle: MIDDLE,
            whole: end !== undefined,
        });
        assert.deepEqual(
            run.faults.slice(0, 5),
            [],
            "the page against the model",
        );
        assert.deepEqual(await stageFaults(page), []);
        assert.deepEqual(
            run.old,
            { shown: 0, cards: 0 },
            "the old playback or card layer ran",
        );
        assert.equal(run.done, true, "the moment did not play out");
        if (end) {
            assert.deepEqual(
                run.shows,
                end.shows,
                "the stage did not come to the recording's end",
            );
        }
        soundsFollowSteps(run);
        return run;
    };
    try {
        return await hangGuard(playOut(), CHECK_SECONDS, fixture).catch((e) => {
            hung = e instanceof HangError;
            throw e;
        });
    } finally {
        // A page that hung would hold its coverage too: closing the context ends it.
        if (!hung) await done();
        await page.close();
    }
}

/** The scripts' steps are played in the script's order: those seen, which take time, are
 * among `script`'s in order. */
function inOrder(run, script) {
    let at = 0;
    for (const step of run.steps) {
        const found = script.findIndex(
            (act, i) =>
                i >= at && JSON.stringify(act) === JSON.stringify(step.act),
        );
        assert.ok(found >= 0, `step ${step.verb} is out of the script's order`);
        at = found + 1;
    }
}

for (const verb of verbs) {
    for (const [seat, fixture] of Object.entries(moment)) {
        for (const layout of layouts) {
            const check = `verb:${verb}/${seat}/${layout}`;
            if (!wanted(check)) continue;
            ran += 1;
            await queue(async () => {
                const started = performance.now();
                const { query, windows, reflows } = LAYOUTS[layout];
                const contexts = [];
                const seen = [];
                let threw = false;
                let covered;
                // Every page's errors are the check's; what a page ran is taken of the first,
                // since each plays the same code.
                const watch = async (page) => {
                    page.on("pageerror", (e) =>
                        seen.push(`${check}: ${e.message}`),
                    );
                    page.on(
                        "console",
                        (m) =>
                            m.type() === "error" &&
                            seen.push(`${check}: ${m.text()}`),
                    );
                    if (covered !== undefined) return async () => {};
                    covered = null;
                    const finish = await coverage(page);
                    return async () => {
                        covered = await finish();
                    };
                };
                try {
                    // Only the lift is asked how its views were batched; the moments a
                    // verb watches besides are each played to their recordings' ends.
                    const names = [
                        ...(verb === "emerge" ? ["whole", "cut"] : ["whole"]),
                        ...(watches[verb] ?? []),
                    ];
                    const played = {
                        ...fixture,
                        ...Object.fromEntries(
                            Object.entries(more).map(([name, by]) => [
                                name,
                                by[seat],
                            ]),
                        ),
                    };
                    const script = await recorded(fixture.whole);
                    for (const [i, size] of windows.entries()) {
                        const at = `${size.width}x${size.height}`;
                        const context = await open(size);
                        contexts.push(context);
                        const runs = { seat, script, reflows };
                        for (const name of names) {
                            const each = [];
                            for (const tick of i ? TICKS.slice(-1) : TICKS) {
                                each.push(
                                    await play(
                                        context,
                                        watch,
                                        query,
                                        played[name],
                                        tick,
                                        name in more
                                            ? await ending(played[name])
                                            : undefined,
                                    ),
                                );
                            }
                            for (const read of ["steps", "sounds"]) {
                                assert.deepEqual(
                                    each.at(-1)[read],
                                    each[0][read],
                                    `${name} at ${at}: ticks of ${TICKS[1]} ms read otherwise than ${TICKS[0]}`,
                                );
                            }
                            inOrder(each[0], await recorded(played[name]));
                            runs[name] =
                                name in more ? complete(each[0]) : each[0];
                        }
                        try {
                            CASES[verb](runs);
                        } catch (e) {
                            e.message = `at ${at}: ${e.message}`;
                            throw e;
                        }
                    }
                    console.log(`ok: ${check}`);
                } catch (e) {
                    failed = true;
                    threw = true;
                    console.error(`FAILED: ${check}\n${e.stack ?? e}`);
                } finally {
                    for (const context of contexts) await context.close();
                }
                errors.push(...seen);
                recordResult(
                    check,
                    threw || seen.length ? "fail" : "pass",
                    (performance.now() - started) / 1000,
                    covered ?? null,
                );
            });
        }
    }
}
await queue.drain();
await browser.close();

if (ran === 0 && !chosen) {
    failed = true;
    console.error(`no verb check's name contains ${filters.join(" or ")}`);
}
if (errors.length) {
    failed = true;
    console.error(`page errors:\n  ${errors.join("\n  ")}`);
}
process.exit(failed ? 1 : 0);
