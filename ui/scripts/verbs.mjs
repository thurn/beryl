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
// asked for at the stage time its step begins or ends, and none otherwise. Exits non-zero if a
// check fails or a page logs an error.
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { basename, join } from "node:path";
import { isDeepStrictEqual } from "node:util";
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
/** How far over what rests a moving node is drawn (src/stage/pose.ts ALOFT). */
const ALOFT = 900;
/** A card's width over its height (src/components/Card.tsx CARD_ASPECT). */
const CARD_ASPECT = 63 / 88;

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
            act: edge.step.act,
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
        // A node dressed with its card's name shows its face (src/stage/dom.ts).
        if (pose.face === "back" && node.dataset.card) {
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
        done ||= !whole && lifted && !abilities;
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
    const shown = stage.shown.view;
    const band = document
        .querySelector(".phone-middle")
        ?.getBoundingClientRect();
    return {
        steps,
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
 * many answers of its seat lead there: one for each view that waits on one. */
async function ending(fixture) {
    const shown = await views(fixture);
    const { turn, status } = shown.at(-1);
    return {
        shows: [turn.number, turn.step, status],
        answers:
            1 + shown.filter((view) => view.status.kind === "deciding").length,
    };
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

const placeOf = ({ x, y, width }) => ({ x, y, width });
/** Whether two poses lie within a card's width of one another. */
const beside = (a, b) =>
    Math.abs(a.x - b.x) < a.width && Math.abs(a.y - b.y) < a.width;

/** `run` with the steps its walk did not see: `stage.step` shows one step at a time, the
 * first not over, so a step that plays wholly while an earlier one of its beat does is never
 * the one shown. Each is put together from what the stage told of it (`edges`) and the
 * samples the walk took at its times, where it took any; one with no sample at its end is
 * left out. */
function complete(run) {
    const samples = new Map();
    for (const step of run.steps) {
        for (const sample of [step.before, ...Object.values(step.at)]) {
            samples.set(sample.at, sample);
        }
    }
    const seen = new Set(
        run.steps.map((s) => `${s.from} ${JSON.stringify(s.act)}`),
    );
    const open = [];
    const steps = [...run.steps];
    for (const { edge, at, act, verb } of run.edges) {
        if (edge === "start") {
            open.push({ from: at, act, verb });
            continue;
        }
        const told = JSON.stringify(act);
        const at0 = open.findIndex((o) => JSON.stringify(o.act) === told);
        if (at0 < 0) continue;
        const [{ from }] = open.splice(at0, 1);
        const sampled = samples.get(at);
        if (at === from || seen.has(`${from} ${told}`) || !sampled) continue;
        const step = { verb, act, from, to: at, at: { to: sampled } };
        for (const [name, time] of [
            ["from", from],
            ["middle", (from + at) / 2],
        ]) {
            if (samples.has(time)) step.at[name] = samples.get(time);
        }
        step.before = before(run, step);
        steps.push(step);
    }
    steps.sort((a, b) => a.from - b.from);
    return { ...run, steps };
}

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
    // Then the cards that change zone otherwise (`zones`): every flight leaves from where its
    // card lay and lands where it then lies. A card you draw alone is held up large over its
    // place in your hand on the way, and cards drawn together begin one after another and
    // are not; the opponent's draw stays face down at its size. A card that leaves face down
    // for a place where its face shows (your draw, the opponent's spell, their discard) is
    // still face down half way and face up as it lands. Your own spell returned to your hand
    // flies there.
    fly({ whole, zones, seat }) {
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

        const flew = zones.steps.filter((s) => s.verb === "fly");
        const draws = flew.filter((s) => s.act.why === "draw");
        const seen = {
            held: 0,
            together: 0,
            hidden: 0,
            discarded: 0,
            turned: 0,
            returned: 0,
        };
        for (const flight of flew) {
            const { from, object } = flight.act;
            const name = object.slot;
            const route = `${from}>${object.zone}`;
            const what = `${route} of ${name} at ${flight.from} ms`;
            const start = flight.at.from?.poses[name];
            const half = flight.at.middle?.poses[name];
            const end = flight.at.to.poses[name];
            assert.equal(flight.at.to.zones[name], object.zone, what);
            assert.ok(end.depth < ALOFT, `${what}: it lands among what rests`);
            if (start) {
                const was = before(zones, flight);
                assert.equal(was.zones[name], from, what);
                assert.deepEqual(
                    where(start),
                    where(was.poses[name]),
                    `${what}: it leaves from where it lay`,
                );
            }
            if (half) assert.ok(half.depth >= ALOFT, `${what}: it flies`);
            const larger = (pose) =>
                pose.width > Math.max(start.width, end.width) + 0.01;
            // A step a tick found already under way was sampled only as it ended.
            const turns = start?.face === "back" && end.face === "front";
            if (turns) seen.turned += 1;
            const others = draws.filter(
                (other) =>
                    other !== flight &&
                    other.act.object.name !== null &&
                    other.from < flight.to &&
                    flight.from < other.to,
            );
            // A card held up to be read has turned over by the time it is.
            const read =
                route === "library>hand" &&
                object.name !== null &&
                others.length === 0;
            if (turns) {
                assert.equal(
                    half.face,
                    read ? "front" : "back",
                    `${what}: its face half way`,
                );
            }
            if (route === "library>hand" && object.name === null) {
                seen.hidden += 1;
                assert.equal(end.face, "back", what);
                if (half) {
                    assert.equal(half.face, "back", what);
                    assert.ok(!larger(half), `${what}: it is not held up`);
                }
            } else if (route === "library>hand") {
                assert.equal(end.face, "front", what);
                if (!read) {
                    seen.together += 1;
                    if (half) assert.ok(!larger(half), `${what}: not held up`);
                } else {
                    seen.held += 1;
                    assert.ok(larger(half), `${what}: it is held up large`);
                    assert.ok(
                        middleOf(half).y < middleOf(end).y,
                        `${what}: over its place in the hand`,
                    );
                    onSide(zones, end, "yours", "your hand");
                }
            } else if (route === "hand>graveyard") {
                seen.discarded += 1;
            } else if (route === "stack>hand") {
                seen.returned += 1;
            }
        }
        // Cards drawn together begin one after another.
        const batch = draws.filter(
            (s) => s.act.object.name !== null || seat === "theirs",
        );
        const starts = batch.slice(0, 3).map((s) => s.from);
        assert.ok(
            starts[0] < starts[1] && starts[1] < starts[2],
            `three draws begin at ${starts}`,
        );
        const { held, together, hidden, discarded, turned, returned } = seen;
        assert.ok(held >= 1, "no draw was held up");
        assert.equal(discarded, 1);
        if (seat === "yours") {
            assert.equal(together, 3, "your three draws together");
            assert.ok(hidden >= 1, "the opponent's draw");
            // Your draws, and the opponent's two spells.
            assert.ok(turned >= held + 2, `${turned} cards turned over`);
        } else {
            assert.ok(hidden >= 3, "the opponent's three draws");
            // Your draws, and the card the opponent discards.
            assert.ok(turned >= held + 1, `${turned} cards turned over`);
            assert.equal(returned, 1, "your spell flies back to your hand");
        }
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
    // A card that leaves (`zones`, and the permanent sacrificed in `scry`) goes from sight
    // where it lay, dressed as it was and never aloft, and only as its step ends is it in its
    // new zone, with no flight; a token is then no more. Every way of leaving is seen: a
    // spell countered and spells resolved, a creature and a token destroyed, a permanent
    // exiled and one sacrificed, and the opponent's spell returned to their hand.
    vanish({ whole, script, zones, scry, seat }) {
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

        const seen = {};
        for (const run of [zones, scry]) {
            const left = run.steps.filter(
                (s) => s.verb === "vanish" && s.act.subject.kind === "object",
            );
            for (const gone of left) {
                const { subject, from, why, object } = gone.act;
                const name = subject.slot;
                const what = `${why} of ${name} at ${gone.from} ms`;
                const lay = before(run, gone).poses[name];
                const kind =
                    object?.zone === "hand"
                        ? "returned"
                        : object
                          ? why
                          : `${why}Token`;
                seen[kind] = (seen[kind] ?? 0) + 1;
                // A spell whose own question was open as it resolved has no node to go: the
                // view of a decision leaves the spell that asks it off its stack, and the
                // prompt shows it (the prompt shapes' beads, by-w8gv.21.26 and .27).
                if (!lay && from === "stack" && why === "resolve") continue;
                assert.ok(lay, `${what}: it was nowhere`);
                for (const at of ["from", "middle"]) {
                    const sample = gone.at[at];
                    if (!sample) continue;
                    const pose = sample.poses[name];
                    assert.deepEqual(
                        placeOf(pose),
                        placeOf(lay),
                        `${what}: it stays where it lay`,
                    );
                    assert.equal(sample.zones[name], from, what);
                    assert.ok(pose.depth < ALOFT, `${what}: it never flies`);
                }
                if (gone.at.from) {
                    assert.equal(gone.at.from.poses[name].opacity, lay.opacity);
                }
                if (gone.at.middle) {
                    const { opacity } = gone.at.middle.poses[name];
                    assert.ok(
                        opacity > 0 && opacity < lay.opacity,
                        `${what}: half way its opacity is ${opacity}`,
                    );
                }
                const after = gone.at.to;
                if (object) {
                    assert.equal(after.zones[name], object.zone, what);
                    assert.notDeepEqual(
                        placeOf(after.poses[name]),
                        placeOf(lay),
                        `${what}: it is at its new place`,
                    );
                } else {
                    assert.equal(after.poses[name], undefined, what);
                }
            }
        }
        // A spell that resolves in the beat of the one it countered, and is gone before it,
        // is over between two samples of the walk: it is not among those seen.
        assert.deepEqual(
            {
                ...seen,
                resolve: Math.min(seen.resolve, 3),
            },
            {
                counter: 1,
                resolve: 3,
                destroy: 1,
                destroyToken: 1,
                exile: 1,
                sacrifice: 1,
                ...(seat === "yours" ? { returned: 1 } : {}),
            },
        );
    },

    // The cards the opponent looks at come off their library face down and lie beside it
    // for as long as the look lasts, and are back on it when it ends; nothing else moves.
    // Your own look lifts nothing: its cards are shown by its prompt, and none leaves your
    // library before or after you answer (the scry flight, one of the five repeated themes:
    // by-8qq, by-2wn7, by-kyig, by-1mrh, by-pv6p, by-yfb2, by-nva).
    lift({ scry, seat }) {
        const lifts = scry.steps.filter((s) => s.verb === "lift");
        if (seat === "yours") {
            assert.deepEqual(lifts, [], "your own look lifts nothing");
            const looked = scry.steps
                .filter((s) => s.verb === "flip")
                .map((s) => s.act.object.slot);
            assert.equal(looked.length, 2, "the scry looks at two cards");
            const told = stepOf(scry, "announce");
            const until = scry.steps.indexOf(told);
            const lay = before(scry, scry.steps[0]).poses;
            for (const step of scry.steps.slice(0, until + 1)) {
                for (const sample of [step.before, ...Object.values(step.at)]) {
                    for (const name of looked) {
                        assert.equal(sample.zones[name], "library");
                        assert.ok(
                            beside(sample.poses[name], lay[name]),
                            `at ${sample.at} ms ${name} has left your library`,
                        );
                        assert.ok(sample.poses[name].depth < ALOFT);
                    }
                }
            }
            return;
        }
        const [begin, end] = ["begin", "end"].map((span) =>
            lifts.find((s) => s.act.span === span),
        );
        assert.ok(begin && end, "the look begins and ends");
        const { handling } = begin.act;
        assert.deepEqual(end.act.handling, handling);
        const lay = before(scry, begin).poses;
        const up = begin.at.to.poses;
        const lifted = Object.keys(up).filter(
            (name) =>
                begin.at.from.zones[name] === "library" &&
                !beside(up[name], lay[name]),
        );
        assert.equal(lifted.length, handling.count, `lifted: ${lifted}`);
        for (const name of lifted) {
            assert.equal(begin.at.to.zones[name], "library");
            for (const sample of Object.values(begin.at)) {
                assert.equal(sample.poses[name].face, "back");
            }
            assert.deepEqual(
                before(scry, end).poses[name],
                up[name],
                "it lies there while the look lasts",
            );
            assert.ok(
                beside(end.at.to.poses[name], lay[name]),
                `${name} is back on the library as the look ends`,
            );
        }
        const [a, b] = lifted.map((name) => up[name]);
        assert.notDeepEqual(placeOf(a), placeOf(b), "they lie side by side");
        // The pile they left settles under them; nothing else moves.
        for (const [name, pose] of Object.entries(begin.at.from.poses)) {
            if (lifted.includes(name) || !up[name]) continue;
            assert.ok(
                begin.at.to.zones[name] === "library"
                    ? beside(up[name], pose)
                    : isDeepStrictEqual(placeOf(up[name]), placeOf(pose)),
                `${name} moved as the cards lifted`,
            );
        }
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
