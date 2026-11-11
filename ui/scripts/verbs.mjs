// The verb suite (plan K24 and K25): every verb the stage plays, checked from both seats on
// the stage's own clock. `by ui-test` serves the UI and runs this beside interactions.mjs,
// which the cut-over retires. Usage:
//   node scripts/verbs.mjs <base-url> <fixtures-dir> [check...]
// A check is `verb:<verb>/<seat>/<layout>` (ui/verbs.json lists them), named whole or in
// part to run it alone, and logged by that name. Each opens the moment's fixture with `?stage`
// in its layout's windows (the desktop's, or each phone size ui/shots.json has with `?phone`),
// holds the stage's clock, plays the recording on as its seat did, and steps the clock inside
// one `page.evaluate`, reading `stage.step` and `stage.pose`: no check waits on the page's
// time, so none depends on the machine's speed. It plays the moment in ticks of 16 ms and of
// 100 ms, which must read the same. Every run also holds the page's sounds to its steps: each
// asked for at the stage time its step begins or ends, and none otherwise. Exits non-zero if a
// check fails or a page logs an error.
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
import { CHECK_SECONDS, HangError, hangGuard } from "./hang.mjs";

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
 * takes no time shows only in what the samples around it hold. Also every edge of a step the
 * stage told (`stage.follow`) with the sounds that step asks for (src/audio/steps.ts), and
 * the sounds the page was asked for meanwhile (`window.berylSounds`), all in stage time; and
 * where the two halves of the table meet. */
async function walk({ tick, gap, answers, middle }) {
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
    const heard = window.berylSounds.length;
    const edges = [];
    const unfollow = stage.follow((edge) =>
        edges.push({
            edge: edge.edge,
            at: edge.at - origin,
            verb: edge.step.act.verb,
            cues: window.berylStepSounds(edge),
        }),
    );
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
    unfollow();
    const band = document
        .querySelector(".phone-middle")
        ?.getBoundingClientRect();
    return {
        steps,
        faults,
        done,
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

/** Which half of `run`'s table `pose` lies in is the half of the seat that plays the moment. */
function onSide(run, pose, seat, what) {
    const far = middleOf(pose).y < run.middle;
    assert.equal(far, seat === "theirs", `${what} lies at y ${pose.y}`);
}

/** `run`'s page was asked for `sound` at the stage time `at`, with no delay. */
function sounded(run, sound, at, what) {
    assert.ok(
        run.sounds.some((s) => s.sound === sound && s.at === at && !s.delay),
        `${what}: no ${sound} at ${at} ms among ${JSON.stringify(run.sounds)}`,
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

/** What each verb's check asks of the moment as `seat` saw it: `whole`, a run of the fixture
 * that holds it in one view, and `cut`, one of the fixture that cuts it at each priority. */
const CASES = {
    // A card flies from where it lay to where it then lies, over everything at rest, and
    // nothing moves but the zones it left and entered: the spell from its hand to the stack,
    // and the permanent from the stack to its side of the battlefield.
    // The spell sounds as it is cast, and the permanent is placed as it lands: at the end of
    // its flight, whenever that is.
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
        onSide(
            whole,
            before(whole, cast).poses[card],
            seat,
            "the hand it left",
        );
        onSide(whole, resolve.at.to.poses[card], seat, "the permanent");
        sounded(whole, "cast", cast.from, "the spell as it is cast");
        sounded(whole, "place", resolve.to, "the permanent as it lands");
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
    // (the most re-filed motion: 12 reports on by-w8gv.21, each another batching). Where the
    // battlefield makes room for the stack, the source glides to its new place meanwhile.
    emerge({ whole, cut, reflows }) {
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
                reflows ? where(from) : from,
                reflows ? where(was.poses[source]) : was.poses[source],
                "its source is where it rested as the lift begins",
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
            const after = lift.at.to.poses[source];
            const next = run.steps[run.steps.indexOf(lift) + 1];
            if (reflows) {
                assert.equal(lift.at.to.zones[source], "battlefield");
                assert.deepEqual(
                    next.before.poses[source],
                    after,
                    "its source has come to rest as the lift ends",
                );
            } else {
                assert.deepEqual(after, from, "its source stays as it is");
            }
            sounded(run, "ability", lift.from, "the ability as it lifts");
            onlyMoved(run, lift, ["stack"], [ability]);
            // It is on the stack until its ability is done.
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
    // another, and an earlier one makes room for the next as the row does: sideways, and
    // where identical permanents stack and a row shrinks to fit, by less than a card any way.
    // Each is placed, by its sound, as it appears.
    appear({ whole, seat, reflows }) {
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
            onSide(whole, end, seat, "the token");
            sounded(whole, "place", token.from, "the token as it appears");
            const seen = tokens
                .slice(tokens.indexOf(token))
                .flatMap((s) => Object.values(s.at));
            for (const sample of seen) {
                const pose = sample.poses[name];
                const sideways = pose.y === end.y && pose.width === end.width;
                assert.ok(
                    (reflows
                        ? Math.abs(pose.y - end.y) < end.width
                        : sideways) && Math.abs(pose.x - end.x) < end.width,
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
    // the step before its `vanish`, and nowhere from that one on, with no fade between, and
    // no sound of its own.
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
        const told = whole.edges.filter((edge) => edge.verb === "vanish");
        assert.ok(told.length > 0, "the stage told of no vanish");
        assert.deepEqual(
            told.flatMap((edge) => edge.cues),
            [],
        );
    },
};
assert.deepEqual(Object.keys(CASES), verbs, "ui/verbs.json and the cases");
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
 * in ticks of `tick`; and checks what holds of every run: the page drew what the model said,
 * nothing but the stage's clock moved a node, its sounds were its steps', and the old
 * playback and card layer took no part. */
async function play(context, watch, query, fixture, tick) {
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
            answers: ANSWERS,
            middle: MIDDLE,
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
                    // Only the lift is asked how its views were batched.
                    const names =
                        verb === "emerge" ? ["whole", "cut"] : ["whole"];
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
                                        fixture[name],
                                        tick,
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
                            inOrder(each[0], await recorded(fixture[name]));
                            runs[name] = each[0];
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
