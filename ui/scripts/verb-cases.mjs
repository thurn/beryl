// The verb suite's cases (plan K24 and K25): what each verb's check asks of the runs verbs.mjs
// plays for it, from one seat in one layout. A run is what that script's `walk` returns: every
// step the stage showed, with every node's pose by `stage.pose` before it, as it began, half
// way and as it ended; the edges of every step the stage told; and the sounds the page was
// asked for, all in stage time. Nothing here touches a page.
import assert from "node:assert/strict";
import { isDeepStrictEqual } from "node:util";

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
const middleOf = (pose) => ({
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
const turnedMiddle = (pose) => {
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
function bows(start, half, end, seat, what) {
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

const placeOf = ({ x, y, width }) => ({ x, y, width });
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

/** What each verb's check asks of the moment as `seat` saw it: `whole`, a run of the fixture
 * that holds it in one view, and `cut`, one of the fixture that cuts it at each priority. */
/** Whether a card dressed with `look` shows: under its pile's top it is hidden, or only its
 * paper edge is drawn. A card of a graveyard you may play shows beside your hand instead. */
const shows = (look) => look !== "hidden" && look !== "rim";

/** How a card that is not broken up leaves, by where it goes and why (`vanish`). */
const LEAVES = {
    exile: "blanch over",
    returned: "glow under",
    resolve: "dim over",
};

export const CASES = {
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
    // still face down half way and face up as it lands, turning over on the way and never
    // spinning round. Your own spell returned to your hand flies there.
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
            // A spell thrown onto the stack and a permanent set down from it leave the
            // straight line, toward their controller's edge of the table.
            bows(start, half, end, seat, `${flight.act.from} flight`);
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
        // A card that lands on the battlefield settles in a wash of light, from the tick it
        // lands; one that lands on the stack does not.
        assert.deepEqual(effectsOn(resolve.at.middle, card), []);
        assert.deepEqual(effectsOn(resolve.at.to, card), ["settle over"]);
        assert.equal(resolve.at.to.effects[card][0].progress, 0);
        assert.deepEqual(effectsOn(cast.at.to, card), []);
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
                leavesFrom(start, was.poses[name], end, what);
            }
            if (half) assert.ok(half.depth >= ALOFT, `${what}: it flies`);
            // Whether it is larger than where it left and where it lands: told only of a
            // flight sampled as it began.
            const larger = (pose) =>
                start && pose.width > Math.max(start.width, end.width) + 0.01;
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
            // A drawn card turns over as it leaves the library, held up or not, and has
            // its face up by half way; a spell turns over a little past half way.
            const drawn = route === "library>hand" && flight.act.why === "draw";
            const read = drawn && object.name !== null && others.length === 0;
            if (turns) {
                assert.equal(
                    half.face,
                    drawn ? "front" : "back",
                    `${what}: its face half way`,
                );
                // It turns over, top over bottom: flat as it leaves and as it lands. A
                // spell is narrowing toward edge-on by half way, its back still showing,
                // and a drawn card lies flat again by then.
                assert.equal(start.flat ?? 1, 1, `${what}: it leaves flat`);
                assert.equal(end.flat ?? 1, 1, `${what}: it lands flat`);
                if (drawn) {
                    assert.equal(
                        half.flat ?? 1,
                        1,
                        `${what}: it has turned over`,
                    );
                } else {
                    assert.ok(
                        half.flat < 1,
                        `${what}: it has not begun to turn`,
                    );
                }
                if (!read) {
                    // And it never spins round in the table's plane besides.
                    const spun = Math.abs(half.rotation - start.rotation);
                    assert.ok(spun < 45, `${what}: it spun ${spun} degrees`);
                }
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
                    // One of several sweeps past the place a card drawn alone is held at
                    // (drawn.ts `swept`): how large it is there is the layout's.
                    seen.together += 1;
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
    // source rests when the step begins, and ends on the stack, along an arc that bows
    // toward its controller's edge of the table; the source stays as it is.
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
            const mine = lift.act.item.controller === "you";
            bows(start, half, end, mine ? "yours" : "theirs", "the lift");
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
    // nothing moves but the battlefield making room, which it has done before the first
    // token of the batch shows. The tokens of a batch begin one after
    // another, and an earlier one makes room for the next as the row does: sideways, and
    // where identical permanents stack and a row shrinks to fit, by less than a card any way.
    // Each is placed, by its sound, as it appears, in a halo under it that is fading by the
    // time it is whole.
    appear({ whole, seat, reflows }) {
        const tokens = whole.steps.filter(
            (s) => s.verb === "appear" && s.act.from === "nowhere",
        );
        assert.ok(tokens.length > 0, "the stage played no token appearing");
        const last = tokens.at(-1);
        for (const token of tokens) {
            const name = token.act.object.slot;
            // The table may hold it before its step begins, opening its row: unseen.
            const early = before(whole, token).poses[name];
            assert.ok(
                !early || early.opacity === 0,
                "it shows before its step",
            );
            assert.deepEqual(effectsOn(token.at.to, name), ["glow under"]);
            const { progress } = token.at.to.effects[name][0];
            assert.ok(progress > 0 && progress < 1, `its glow at ${progress}`);
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
        // The table has opened for the batch before its first token shows: from then on
        // nothing but those tokens is anywhere but where the batch leaves it.
        const batch = tokens.map((token) => `${token.act.object.slot}`);
        const settled = last.at.to.poses;
        for (const sample of tokens.flatMap((s) => Object.values(s.at))) {
            for (const [other, pose] of Object.entries(sample.poses)) {
                if (batch.includes(other)) continue;
                assert.deepEqual(
                    where(pose),
                    where(settled[other]),
                    `at ${sample.at} ms ${other} is still making room`,
                );
            }
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
    // new zone, with no flight; a token is then no more. How it goes says what became of it:
    // what is destroyed, countered or sacrificed darkens and breaks up for as long as it takes
    // to leave, which is its whole step, a spell that resolved only darkens, what goes to
    // exile is lost in light, and what goes back to a hand leaves in a halo; nothing of any
    // stays on a card at its new place. The graveyard it goes to shows the top it had until
    // it lands, and then it (the motion review of vanish, by-w8gv.21.48: a phone's pile showed
    // none, and so did the desktop's when two cards left for it together), and an empty one
    // shows nothing until then (a phone's showed its edge, a dark slab). Every way of leaving
    // is seen: a
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
                    assert.deepEqual(
                        effectsOn(sample, name),
                        [LEAVES[kind] ?? "dissolve over"],
                        `${what}: how it goes`,
                    );
                    const top = sample.tops[object?.owner];
                    if (object?.zone === "graveyard" && top !== undefined) {
                        assert.ok(
                            shows(sample.looks[top]),
                            `${what}: its graveyard's top, ${top}, does not show`,
                        );
                    }
                    // A phone's strip draws a pile's edge, and only behind a top.
                    if (object?.zone === "graveyard" && sample.edges) {
                        assert.equal(
                            sample.edges[object.owner],
                            top !== undefined,
                            `${what}: its graveyard's edge with no card on it`,
                        );
                    }
                }
                // What lasts while it leaves lasts as long as its step, which is over as it
                // is gone; a halo has its own time.
                if (gone.at.middle && kind !== "returned") {
                    const [{ progress }] = gone.at.middle.effects[name];
                    assert.ok(Math.abs(progress - 0.5) < 1e-9, what);
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
                    assert.deepEqual(effectsOn(after, name), [], what);
                    if (object.zone === "graveyard") {
                        assert.equal(after.tops[object.owner], name, what);
                        assert.ok(shows(after.looks[name]), what);
                    }
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

    // The cards the opponent looks at come off their library face down, one after another,
    // and lie beside it for as long as the look lasts, and are back on it when it ends;
    // nothing else moves.
    // Your own look lifts nothing: its cards are shown by its prompt, and none leaves your
    // library before or after you answer (the scry flight, one of the five repeated themes:
    // by-8qq, by-2wn7, by-kyig, by-1mrh, by-pv6p, by-yfb2, by-nva).
    //
    // A look that takes no card from where it lies moves nothing at all, as it begins or as
    // it ends: the opponent's handling of their own hand, which they mulligan here, and their
    // search of a library, which counts no cards. The old board's checks of these watched a
    // page ("opponent hand stays parked during a private choice", "an opponent searching
    // keeps their library parked": by-w8gv.21.65).
    lift({ scry, opening, reveal, seat }) {
        const parked = { "look at their hand": 0, search: 0 };
        for (const run of [opening, reveal]) {
            for (const step of run.steps.filter((s) => s.verb === "lift")) {
                const { handling, span } = step.act;
                const kind =
                    handling.zone === "hand"
                        ? handling.side === "opponent" && "look at their hand"
                        : handling.count == null && "search";
                if (!kind) continue;
                // As it begins, which is as the step before it in its script ends, everything
                // lies as that step left it.
                const lay = step.at.from?.poses;
                if (!lay) continue;
                parked[kind] += 1;
                for (const sample of Object.values(step.at)) {
                    for (const [name, pose] of Object.entries(sample.poses)) {
                        if (!lay[name]) continue;
                        assert.deepEqual(
                            where(pose),
                            where(lay[name]),
                            `${name} moved at ${sample.at} ms, as their ${kind} ${span}s`,
                        );
                    }
                }
            }
        }
        assert.ok(
            parked["look at their hand"] >= 2,
            "the opponent handles their hand",
        );
        // Your own search lifts nothing either: its prompt shows its cards.
        assert.equal(parked.search, seat === "yours" ? 0 : 2, "a search");
        const lifts = scry.steps.filter((s) => s.verb === "lift");
        if (seat === "yours") {
            assert.deepEqual(lifts, [], "your own look lifts nothing");
            // Learning their names takes no time, so the stage only tells of it.
            const looked = scry.edges
                .filter((e) => e.verb === "flip" && e.edge === "start")
                .map((e) => e.act.object.slot);
            assert.equal(looked.length, 2, "the scry looks at two cards");
            // Until the scry is announced: its pill then passes over what follows.
            const told = scry.edges.find((e) => e.verb === "announce");
            assert.ok(told, "the stage announced no scry");
            const lay = before(scry, scry.steps[0]).poses;
            for (const step of scry.steps.filter((s) => s.to <= told.at)) {
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
            // Every sample taken between the two steps: none where the look's end follows
            // at once, the opponent's answer being already in (a library is ordered in no
            // time).
            for (const sample of scry.samples) {
                if (sample.at < begin.to || sample.at >= end.from) continue;
                assert.deepEqual(
                    sample.poses[name],
                    up[name],
                    "it lies there while the look lasts",
                );
            }
            assert.ok(
                beside(end.at.to.poses[name], lay[name]),
                `${name} is back on the library as the look ends`,
            );
        }
        const [a, b] = lifted.map((name) => up[name]);
        assert.notDeepEqual(placeOf(a), placeOf(b), "they lie side by side");
        // They come off one after another: half way, one has and the other waits on the pile.
        const half = begin.at.middle?.poses;
        if (half) {
            const waiting = lifted.filter((name) =>
                beside(half[name], lay[name]),
            );
            assert.equal(waiting.length, 1, `waiting half way: ${waiting}`);
        }
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
