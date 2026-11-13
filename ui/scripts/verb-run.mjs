// What the verb suite's cases share (verb-cases.mjs and its parts): reading a run verbs.mjs
// played, and what a pose, a step or a sound of it is held to. A run is what that script's
// `walk` returns: every step the stage showed, with every node's pose by `stage.pose` before
// it, as it began, half way and as it ended; the edges of every step the stage told; and the
// sounds the page was asked for, all in stage time. Nothing here touches a page.
import assert from "node:assert/strict";

/** How far over what rests a moving node is drawn (src/stage/pose.ts ALOFT). */
export const ALOFT = 900;
/** A card's width over its height (src/components/Card.tsx CARD_ASPECT). */
export const CARD_ASPECT = 63 / 88;

export const where = ({ x, y, width, rotation, skew, turn, opacity }) => ({
    x,
    y,
    width,
    rotation,
    skew,
    turn,
    opacity,
});
export const middleOf = (pose) => ({
    x: pose.x + pose.width / 2,
    y: pose.y + pose.width / CARD_ASPECT / 2,
});

/** Holds a flight to leaving from where its card lay: at `start`, its first tick, it is as
 * it was at `lay`. A card back that lands the other way up the table, at `end`, leaves that
 * way up instead, half a turn round over the place it lay, and never spins on its way
 * (tracks.ts `glide`): the opponent's draw and the cards they put back, on the desktop. */
export function leavesFrom(start, lay, end, what) {
    const round =
        end.face === "back" && Math.abs(start.rotation - lay.rotation) > 90;
    if (!round) {
        assert.deepEqual(
            where(start),
            where(lay),
            `${what}: it leaves from where it lay`,
        );
        return;
    }
    const [now, then] = [turnedMiddle(start), turnedMiddle(lay)];
    const off = Math.hypot(now.x - then.x, now.y - then.y);
    assert.ok(
        off < lay.width / 4 && start.width === lay.width,
        `${what}: it leaves ${off} from where it lay`,
    );
    const spun = Math.abs(end.rotation - start.rotation);
    assert.ok(spun < 45, `${what}: it spun ${spun} degrees`);
}

/** The middle of the card drawn at `pose`, turned about its corner as the stage draws it. */
export const turnedMiddle = (pose) => {
    const turn = (pose.rotation * Math.PI) / 180;
    const [across, down] = [pose.width / 2, pose.width / CARD_ASPECT / 2];
    return {
        x: pose.x + across * Math.cos(turn) - down * Math.sin(turn),
        y: pose.y + across * Math.sin(turn) + down * Math.cos(turn),
    };
};
/** Where the card at `pose` is against the straight line from where it is at `start` to
 * where it is at `end`, by their middles: how far `along` it, and how far `off` it toward
 * the bottom of the table, as shares of the line's length. */
export function against(start, pose, end) {
    const [a, p, b] = [start, pose, end].map(turnedMiddle);
    const [across, down] = [b.x - a.x, b.y - a.y];
    const length = across ** 2 + down ** 2;
    const along = ((p.x - a.x) * across + (p.y - a.y) * down) / length;
    const side = Math.sign(across) || 1;
    const off = (side * ((p.y - a.y) * across - (p.x - a.x) * down)) / length;
    return { along, off, level: Math.abs(across) > Math.abs(down) };
}
/** A flight of `seat`'s card half way: on its way from `start` to `end`, and off the
 * straight line between them toward that player's edge of the table (src/stage/tracks.ts
 * BOWS), where the line does not run up the table. */
export function bows(start, half, end, seat, what) {
    const { along, off, level } = against(start, half, end);
    assert.ok(along > 0 && along < 1, `${what}: half way it is on its way`);
    assert.ok(Math.abs(off) > 0.04, `${what}: it flies a straight line`);
    if (level) {
        assert.equal(
            off > 0,
            seat === "yours",
            `${what}: it bows toward the middle of the table`,
        );
    }
}

/** A flight of `seat`'s card half way that goes all but straight: on its way from `start` to
 * `end`, no more than `most` of the line's length off the straight line between them, and,
 * where the line runs across the table, off it toward the middle of the table, away from that
 * player's edge (src/stage/tracks.ts BOWS, `lifts`). */
export function sags(start, half, end, seat, what, most) {
    const { along, off, level } = against(start, half, end);
    assert.ok(along > 0 && along < 1, `${what}: half way it is on its way`);
    assert.ok(
        Math.abs(off) <= most,
        `${what}: it is ${off} off a straight line`,
    );
    if (level && off !== 0) {
        assert.equal(
            off < 0,
            seat === "yours",
            `${what}: it sags toward the middle of the table`,
        );
    }
}

/** A permanent's flight from the stack half way: on its way, and thrown up the screen, well
 * above the straight line from where it is at `start` to where it is at `end`, by their
 * middles, whichever player's it is (src/stage/tracks.ts LIFT). Where that line runs up or
 * down the table, as the phone's does, being thrown up it only holds the card back along
 * the line or carries it past its place before it drops, so there it is only held to be
 * under way. */
export function rises(start, half, end, what) {
    const [a, p, b] = [start, half, end].map(turnedMiddle);
    const { level } = against(start, half, end);
    assert.notDeepEqual(where(half), where(start), `${what}: it has not left`);
    assert.notDeepEqual(
        where(half),
        where(end),
        `${what}: it is there at once`,
    );
    if (!level) return;
    const line = a.y + ((b.y - a.y) * (p.x - a.x)) / (b.x - a.x);
    assert.ok(
        p.y < line - 0.3 * start.width,
        `${what}: half way it is high above its line (${p.y} against ${line})`,
    );
}

export const placeOf = ({ x, y, width }) => ({ x, y, width });
/** Whether two poses lie within a card's width of one another. */
export const beside = (a, b) =>
    Math.abs(a.x - b.x) < a.width && Math.abs(a.y - b.y) < a.width;

/** `run` with the steps its walk did not see: `stage.step` shows one step at a time, the
 * first not over, so a step that plays wholly while an earlier one of its beat does is never
 * the one shown, and one still playing when the step shown before it ends is shown late.
 * Each is put together from what the stage told of it (`edges`) and the samples the walk
 * took at its times (`extra`: the middle and end of every step told), where it took any; one
 * with no sample at its end is left out. With them, every sample the walk took, in order (`samples`). */
export function complete(run) {
    const samples = new Map(run.extra.map((sample) => [sample.at, sample]));
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
        step.before = before(run, step);
        steps.push(step);
    }
    // A step the walk came to late, or never showed, has what was sampled at its times.
    for (const step of steps) {
        for (const [name, time] of [
            ["from", step.from],
            ["middle", (step.from + step.to) / 2],
        ]) {
            if (!step.at[name] && samples.has(time)) {
                step.at = { ...step.at, [name]: samples.get(time) };
            }
        }
    }
    steps.sort((a, b) => a.from - b.from);
    const all = [...samples.values()].sort((a, b) => a.at - b.at);
    return { ...run, steps, samples: all };
}

/** The first step of `run` playing `verb`. */
export function stepOf(run, verb) {
    const step = run.steps.find((s) => s.verb === verb);
    assert.ok(step, `the stage played no ${verb}`);
    return step;
}

/** What `run` held before `step` began: its last sample from an earlier stage time. A step
 * begins in the tick that ends the one before it, so the sample at that time already has it
 * begun; a run's first step has only the sample taken before any tick. */
export function before(run, step) {
    const earlier = run.steps
        .flatMap((s) => [s.before, ...Object.values(s.at)])
        .filter((sample) => sample.at < step.from);
    return earlier.sort((a, b) => a.at - b.at).at(-1) ?? run.steps[0].before;
}

/** Nothing moved over `step` of `run` but what lies in `zones`, which a card leaving or
 * arriving rearranges, and `names`. The battlefield's rows count with the stack: they keep
 * clear of its column, which stands between them (src/game/layout.ts STACK). */
export function onlyMoved(run, step, zones, names = []) {
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
export function onSide(run, pose, seat, what) {
    const far = middleOf(pose).y < run.middle;
    assert.equal(far, seat === "theirs", `${what} lies at y ${pose.y}`);
}

/** The effects `sample` holds on the node `name` (src/stage/effects.ts), each as its name and
 * the layer it is drawn in. */
export const effectsOn = (sample, name) =>
    (sample.effects[name] ?? []).map((e) => `${e.name} ${e.layer}`);

/** `run`'s page was asked for `sound` at the stage time `at`, to sound `delay` ms after. */
export function sounded(run, sound, at, what, delay = 0) {
    assert.ok(
        run.sounds.some(
            (s) =>
                s.sound === sound &&
                s.at === at &&
                Math.abs((s.delay ?? 0) - delay) < 1e-6,
        ),
        `${what}: no ${sound} at ${at} ms among ${JSON.stringify(run.sounds)}`,
    );
}

/** Every sound `run`'s page was asked for is a step's, at the stage time that step begins or
 * ends as the step says, to sound then or the share of the step after it the step says; and
 * every sound a step asks for was asked for then, once however many steps ask at one time. */
export function soundsFollowSteps(run) {
    const asked = [];
    for (const { edge, at, span, cues } of run.edges) {
        for (const cue of cues) {
            const same = (s) => s.sound === cue.sound && s.at === at;
            if (cue.edge === edge && !asked.some(same)) {
                const delay = (cue.into ?? 0) * span;
                asked.push({ sound: cue.sound, at, delay });
            }
        }
    }
    assert.deepEqual(run.sounds, asked, "the sounds against the steps");
}

/** The scripts' steps are played in the script's order: those seen, which take time, are
 * among `script`'s in order. */
export function inOrder(run, script) {
    let at = 0;
    const recorded = run.steps.filter(
        (step) => !run.written || step.from < run.written.from,
    );
    for (const step of recorded) {
        const found = script.findIndex(
            (act, i) =>
                i >= at && JSON.stringify(act) === JSON.stringify(step.act),
        );
        assert.ok(found >= 0, `step ${step.verb} is out of the script's order`);
        at = found + 1;
    }
}
