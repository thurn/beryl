// The verb suite (plan K24 and K25): every verb the stage plays, checked from both seats on
// the stage's own clock. `by ui-test` serves the UI and runs this beside interactions.mjs,
// which the cut-over retires. Usage:
//   node scripts/verbs.mjs <base-url> <fixtures-dir> [check...]
// A check is `verb:<verb>/<seat>/<layout>` (ui/verbs.json lists them), named whole or in
// part to run it alone, and logged by that name. Each opens the moment's fixture with `?stage`,
// holds the stage's clock, plays the recording on as its seat did, and steps the clock inside
// one `page.evaluate`, reading `stage.step` and `stage.pose`: no check waits on the page's
// time, so none depends on the machine's speed. It plays the moment in ticks of 16 ms and of
// 100 ms, which must read the same. Exits non-zero if a check fails or a page logs an error.
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

const [base, fixtures, ...filters] = process.argv.slice(2);
if (!base || !fixtures) {
    console.error(
        "usage: node scripts/verbs.mjs <base-url> <fixtures-dir> [check...]",
    );
    process.exit(2);
}
const { verbs, layouts, moment } = JSON.parse(
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

/** The sizes of tick every check plays the moment in. */
const TICKS = [16, 100];
/** How long the stage stands idle between a seat's answers, in stage ms. */
const GAP = 250;
/** The most answers a moment takes: the cast, the resolution and the trigger's, with room. */
const ANSWERS = 6;
/** The stage's height, whose upper half is the opponent's (src/game/layout.ts STAGE). */
const MIDDLE = 450;
/** How far over what rests a moving node is drawn (src/stage/pose.ts ALOFT). */
const ALOFT = 900;
/** A card's width over its height (src/components/Card.tsx CARD_ASPECT). */
const CARD_ASPECT = 63 / 88;

/** Plays the moment on the page's stage, in the page: holds the stage's clock, answers as the
 * recording's seat did (the fixture transport plays the recorded views on at each `choose`,
 * src/ipc.ts) and walks the clock from step to step in ticks of `tick`, until an ability has
 * come onto the stack and gone. Returns each step `stage.step` showed, in order, with every
 * node's pose by `stage.pose` before it, as it began, half way and as it ended, where a tick
 * landed there; and how the page's nodes differed from the model at any of those. A step that
 * takes no time shows only in what the samples around it hold. */
async function walk({ tick, gap, answers }) {
    const stage = window.berylStage;
    const { clock } = stage;
    clock.hold();
    // Whole milliseconds from here on, so that two runs give the same numbers to the bit.
    while (!Number.isInteger(clock.now)) {
        clock.advance(Math.ceil(clock.now) - clock.now);
    }
    const origin = clock.now;
    const reach = (time) => {
        while (clock.now < time) {
            clock.advance(Math.min(tick, time - clock.now));
        }
    };
    const layer = document.querySelector(".card-layer");
    const faults = [];
    const near = (a, b, by) => Math.abs(a - b) <= by;
    /** Why the page's node for `name` is not where the model has it, if it is not. */
    const differs = (name, pose, frame) => {
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
    const sample = () => {
        const poses = {};
        const zones = {};
        const { resting } = stage.shown;
        for (const [name, { dress }] of resting) {
            const pose = stage.pose(name);
            poses[name] = pose;
            zones[name] = dress.zone;
            const why = differs(name, pose, dress.frame);
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
        done ||= lifted && !abilities;
        lifted ||= abilities;
        return { at: clock.now - origin, poses, zones };
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
            if (clock.now < from) reach(from);
            if (clock.now === from) at.from = sample();
            const middle = (from + to) / 2;
            if (clock.now < middle) {
                reach(middle);
                at.middle = sample();
            }
            reach(to);
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
        if (!done) reach(clock.now + gap);
    }
    return {
        steps,
        faults,
        done,
        // What the old playback and card layer leave on a page they run on.
        old: {
            shown: window.berylShown?.length ?? 0,
            cards: document.querySelectorAll(".card-el:not(.stage-node)")
                .length,
        },
    };
}

/** The steps of a recorded fixture's scripts after its first view, in order. */
async function recorded(fixture) {
    const { messages } = JSON.parse(
        await readFile(join(fixtures, `${fixture}.json`), "utf8"),
    );
    return messages
        .filter((m) => m.kind === "event")
        .slice(1)
        .flatMap((m) => m.payload.view.script.beats.flat())
        .map((step) => step.act);
}

const where = ({ x, y, width, rotation, skew, opacity }) => ({
    x,
    y,
    width,
    rotation,
    skew,
    opacity,
});
const middleOf = (pose) => ({
    x: pose.x + pose.width / 2,
    y: pose.y + pose.width / CARD_ASPECT / 2,
});

/** The first step of `run` playing `verb`. */
function stepOf(run, verb) {
    const step = run.steps.find((s) => s.verb === verb);
    assert.ok(step, `the stage played no ${verb}`);
    return step;
}

/** What `run` held before `step` began: its last sample from an earlier stage time. A step
 * begins in the tick that ends the one before it, so the sample at that time already has it
 * begun; a run's first step has only the sample taken before any tick. */
function before(run, step) {
    const earlier = run.steps
        .flatMap((s) => [s.before, ...Object.values(s.at)])
        .filter((sample) => sample.at < step.from);
    return earlier.sort((a, b) => a.at - b.at).at(-1) ?? run.steps[0].before;
}

/** Nothing moved over `step` of `run` but what lies in `zones`, which a card leaving or
 * arriving rearranges, and `names`. The battlefield's rows count with the stack: they keep
 * clear of its column, which stands between them (src/game/layout.ts STACK). */
function onlyMoved(run, step, zones, names = []) {
    const free = zones.includes("stack") ? [...zones, "battlefield"] : zones;
    const was = before(run, step);
    for (const [name, pose] of Object.entries(was.poses)) {
        const zone = [was.zones[name], step.at.to.zones[name]];
        if (names.includes(name) || zone.some((z) => free.includes(z))) {
            continue;
        }
        assert.deepEqual(
            step.at.to.poses[name],
            pose,
            `${name} moved during the ${step.verb}`,
        );
    }
}

/** Which half of the table `pose` lies in is the half of the seat that plays the moment. */
function onSide(pose, seat, what) {
    const far = middleOf(pose).y < MIDDLE;
    assert.equal(far, seat === "theirs", `${what} lies at y ${pose.y}`);
}

/** What each verb's check asks of the moment as `seat` saw it: `whole`, a run of the fixture
 * that holds it in one view, and `cut`, one of the fixture that cuts it at each priority. */
const CASES = {
    // A card flies from where it lay to where it then lies, over everything at rest, and
    // nothing moves but the zones it left and entered: the spell from its hand to the stack,
    // and the permanent from the stack to its side of the battlefield.
    fly({ whole, seat }) {
        const flights = whole.steps.filter((s) => s.verb === "fly");
        const [cast, resolve] = flights;
        assert.deepEqual(
            flights.slice(0, 2).map((s) => [s.act.from, s.act.object.zone]),
            [
                ["hand", "stack"],
                ["stack", "battlefield"],
            ],
        );
        assert.equal(cast.act.object.slot, resolve.act.object.slot);
        const card = cast.act.object.slot;
        for (const flight of [cast, resolve]) {
            const was = before(whole, flight);
            const start = was.poses[card];
            const half = flight.at.middle.poses[card];
            const end = flight.at.to.poses[card];
            assert.equal(was.zones[card], flight.act.from);
            assert.deepEqual(
                where(flight.at.from.poses[card]),
                where(start),
                "it leaves from where it lay",
            );
            assert.equal(flight.at.to.zones[card], flight.act.object.zone);
            const between = (a, b, c) => (a - b) * (c - b) < 0;
            assert.ok(
                between(start.y, half.y, end.y) ||
                    between(start.x, half.x, end.x),
                "half way through it is on its way",
            );
            assert.ok(half.depth >= ALOFT, "it flies over what rests");
            assert.ok(end.depth < ALOFT, "it lands among what rests");
            assert.equal(end.opacity, 1);
            onlyMoved(whole, flight, [flight.act.from, flight.act.object.zone]);
        }
        onSide(before(whole, cast).poses[card], seat, "the hand it left");
        onSide(resolve.at.to.poses[card], seat, "the permanent");
        // It stays where it landed: the trigger lifts out of it there.
        const lift = stepOf(whole, "emerge");
        assert.deepEqual(
            before(whole, lift).poses[card],
            resolve.at.to.poses[card],
        );
    },

    // A trigger lifts out of its source: it begins small in the source's middle, wherever the
    // source rests when the step begins, and ends on the stack; the source stays as it is.
    // The lift is the same whether the session sent the moment whole or cut it into views
    // (the most re-filed motion: 12 reports on by-w8gv.21, each another batching).
    emerge({ whole, cut }) {
        const lifts = [whole, cut].map((run) => {
            const lift = stepOf(run, "emerge");
            const ability = `a${lift.act.id}`;
            const source = lift.act.item.source;
            const was = before(run, lift);
            assert.equal(was.poses[ability], undefined);
            assert.equal(was.zones[source], "battlefield");
            const [start, half, end] = ["from", "middle", "to"].map(
                (at) => lift.at[at].poses[ability],
            );
            const from = lift.at.from.poses[source];
            assert.deepEqual(
                from,
                was.poses[source],
                "its source rests as the lift begins",
            );
            const [a, b] = [middleOf(start), middleOf(from)];
            assert.ok(
                Math.abs(a.x - b.x) < 0.01 && Math.abs(a.y - b.y) < 0.01,
                `it begins in its source's middle: ${JSON.stringify([a, b])}`,
            );
            assert.ok(start.width < from.width, "it begins smaller than it");
            assert.equal(start.opacity, 1);
            assert.ok(
                start.width < half.width && half.width < end.width,
                "it grows on its way",
            );
            assert.ok(half.depth >= ALOFT, "it flies over what rests");
            assert.equal(lift.at.to.zones[ability], "stack");
            assert.deepEqual(
                lift.at.to.poses[source],
                from,
                "its source stays as it is",
            );
            onlyMoved(run, lift, ["stack"], [ability]);
            // It is on the stack until its ability is done.
            const next = run.steps[run.steps.indexOf(lift) + 1];
            assert.deepEqual(next.before.poses[ability], end);
            return {
                start,
                half,
                end,
                source: from,
                took: lift.to - lift.from,
            };
        });
        assert.deepEqual(
            lifts[1],
            lifts[0],
            "the lift differs with how its views were batched",
        );
    },

    // A token is at its place from the first and comes to be seen there: no flight, and
    // nothing moves but the battlefield making room. The tokens of a batch begin one after
    // another, and an earlier one makes room for the next as the row does.
    appear({ whole, seat }) {
        const tokens = whole.steps.filter(
            (s) => s.verb === "appear" && s.act.from === "nowhere",
        );
        assert.ok(tokens.length > 0, "the stage played no token appearing");
        const last = tokens.at(-1);
        for (const token of tokens) {
            const name = token.act.object.slot;
            assert.equal(before(whole, token).poses[name], undefined);
            if (token.at.from) {
                assert.equal(token.at.from.poses[name].opacity, 0);
            }
            const end = last.at.to.poses[name];
            assert.equal(end.opacity, 1);
            assert.ok(end.depth < ALOFT);
            assert.equal(last.at.to.zones[name], "battlefield");
            onSide(end, seat, "the token");
            const seen = tokens
                .slice(tokens.indexOf(token))
                .flatMap((s) => Object.values(s.at));
            for (const sample of seen) {
                const pose = sample.poses[name];
                assert.ok(
                    pose.y === end.y &&
                        pose.width === end.width &&
                        Math.abs(pose.x - end.x) < end.width,
                    `at ${sample.at} ms it is not at its place`,
                );
            }
            onlyMoved(whole, token, ["battlefield"]);
        }
        // The last of them moves for none.
        const name = last.act.object.slot;
        for (const sample of Object.values(last.at)) {
            const pose = sample.poses[name];
            assert.deepEqual(
                where({ ...pose, opacity: 1 }),
                where(last.at.to.poses[name]),
                "it is at its place throughout",
            );
            assert.ok(pose.depth < ALOFT, "it never flies");
        }
    },

    // A finished ability is gone at once: whole on the stack in every sample up to the end of
    // the step before its `vanish`, and nowhere from that one on, with no fade between.
    vanish({ whole, script }) {
        const lift = stepOf(whole, "emerge");
        const ability = `a${lift.act.id}`;
        const end = lift.at.to.poses[ability];
        const samples = whole.steps
            .slice(whole.steps.indexOf(lift) + 1)
            .flatMap((s) => [s.before, ...Object.values(s.at)]);
        const last = samples.pop();
        for (const sample of samples) {
            assert.deepEqual(
                sample.poses[ability],
                end,
                `at ${sample.at} ms it is whole on the stack`,
            );
        }
        assert.equal(last.poses[ability], undefined);
        // The step whose end took it is the one the script has before its `vanish`.
        const gone = script.findIndex(
            (act) => act.verb === "vanish" && act.subject.id === lift.act.id,
        );
        assert.ok(gone > 0, "the script has no vanish for the ability");
        assert.deepEqual(whole.steps.at(-1).act, script[gone - 1]);
        assert.equal(last.at, whole.steps.at(-1).to);
    },
};
assert.deepEqual(Object.keys(CASES), verbs, "ui/verbs.json and the cases");

const browser = await launch();
const queue = workers(6);
const errors = [];
let failed = false;
let ran = 0;

/** A context whose pages fetch the fixtures this run recorded, not what Vite serves. */
async function open() {
    const context = await browser.newContext({
        viewport: { width: 1600, height: 900 },
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

/** Plays `fixture` from its first view on the stage of a page of its own in `context`, which
 * `watch` is shown first and returns what to do before the page closes, in ticks of `tick`;
 * and checks what holds of every run: the page drew what the model said, nothing but the
 * stage's clock moved a node, and the old playback and card layer took no part. */
async function play(context, watch, fixture, tick) {
    const page = await context.newPage();
    const done = await watch(page);
    try {
        await page.goto(
            new URL(`/?stage&fixture=${fixture}&at=1#/game`, base).href,
        );
        await page.waitForFunction(
            () => window.berylStage && document.querySelector(".stage-node"),
        );
        await page.evaluate(() => document.fonts.ready.then(() => {}));
        const run = await page.evaluate(walk, {
            tick,
            gap: GAP,
            answers: ANSWERS,
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
        return run;
    } finally {
        await done();
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
                const context = await open();
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
                    // Only the lift is asked how its views were batched.
                    const names =
                        verb === "emerge" ? ["whole", "cut"] : ["whole"];
                    const runs = { seat };
                    for (const name of names) {
                        const each = [];
                        for (const tick of TICKS) {
                            each.push(
                                await play(context, watch, fixture[name], tick),
                            );
                        }
                        assert.deepEqual(
                            each[1].steps,
                            each[0].steps,
                            `${name}: ticks of ${TICKS[1]} ms read otherwise than ${TICKS[0]}`,
                        );
                        inOrder(each[0], await recorded(fixture[name]));
                        runs[name] = each[0];
                    }
                    runs.script = await recorded(fixture.whole);
                    CASES[verb](runs);
                    console.log(`ok: ${check}`);
                } catch (e) {
                    failed = true;
                    threw = true;
                    console.error(`FAILED: ${check}\n${e.stack ?? e}`);
                } finally {
                    await context.close();
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
