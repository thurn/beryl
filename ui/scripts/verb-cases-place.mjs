// The verb suite's cases for changes on a card where it lies and to a player (plan K24 and
// K25): what each verb's check asks of the runs verbs.mjs plays for it, as verb-cases.mjs does
// for cards changing zone. The moments are `combat` (the board verbs-combat: three attackers,
// one blocked, one at a planeswalker; a Treasure tapped and sacrificed; life gained) and
// `place` (verbs-place: a seventh land, a permanent turned face up, one taken across the
// table, counters, an Aura). Nothing here touches a page.
import assert from "node:assert/strict";
import {
    ALOFT,
    before,
    beside,
    CARD_ASPECT,
    onSide,
    sounded,
    where,
} from "./verb-cases.mjs";
import { STRIKE } from "../src/stage/tracks.ts";

/** The middle of a permanent's card at `pose`: of its frame, which lies turned a quarter in
 * its slot when it is tapped, leaned as the table leans it (src/stage/desktop.ts middle). */
function cardMiddle(pose, tapped) {
    const long = pose.width / CARD_ASPECT;
    const down = (tapped ? pose.width : long) / 2;
    const across = (tapped ? long : pose.width) / 2;
    return {
        x: pose.x + across + Math.tan((pose.skew * Math.PI) / 180) * down,
        y: pose.y + down,
    };
}

const near = (a, b, what) =>
    assert.ok(
        Math.abs(a.x - b.x) < 0.01 && Math.abs(a.y - b.y) < 0.01,
        `${what}: ${JSON.stringify([a, b])}`,
    );

/** The stage time `run` was told the step of `act` began or ended at. */
function told(run, act, edge) {
    const said = JSON.stringify(act);
    const found = run.edges.find(
        (e) => e.edge === edge && JSON.stringify(e.act) === said,
    );
    assert.ok(found, `the stage told no ${edge} of ${act.verb}`);
    return found.at;
}

/** Every sample `run` took at a stage time from `from` to `to`. */
const samples = (run, from, to) =>
    run.samples.filter((sample) => sample.at >= from && sample.at <= to);

/** Whether `run` was at rest before `step` began: no other step ended as it did, so the last
 * sample before it (`before`) has everything where the step found it. */
const rested = (run, step) =>
    !run.edges.some((e) => e.edge === "end" && e.at === step.from);

export const CASES = {
    // A permanent turns a quarter as it taps or untaps: dressed from the first as its record
    // now says, its card is a quarter back about its own middle, which is where it was, and
    // turns for as long as the step takes at the depth it rests at. Attackers tap together,
    // lands as they pay, and all untap together.
    // A permanent that taps and then leaves (a Treasure, tapped and sacrificed for mana) has
    // done turning as it begins to go: the script orders the two, and the departure begins at
    // the stage time the turn ends, with no wait between (the old board held 450 ms there).
    tap({ combat, place }) {
        const seen = { tap: 0, untap: 0, placed: 0 };
        for (const run of [combat, place]) {
            for (const step of run.steps.filter((s) => s.verb === "tap")) {
                const { slot, tapped } = step.act.object;
                const what = `${tapped ? "tap" : "untap"} of ${slot} at ${step.from} ms`;
                const was = before(run, step);
                assert.equal(was.zones[slot], "battlefield", what);
                assert.equal(was.marks[slot].tapped, !tapped, what);
                assert.equal(was.poses[slot].turn, 0, what);
                seen[tapped ? "tap" : "untap"] += 1;
                if (step.at.from) {
                    const { poses, marks } = step.at.from;
                    assert.equal(marks[slot].tapped, tapped, what);
                    assert.equal(poses[slot].turn, tapped ? -90 : 90, what);
                    if (rested(run, step)) {
                        seen.placed += 1;
                        near(
                            cardMiddle(poses[slot], tapped),
                            cardMiddle(was.poses[slot], !tapped),
                            `${what}: its card begins where it was`,
                        );
                    }
                }
                if (step.at.middle) {
                    const { turn, depth } = step.at.middle.poses[slot];
                    assert.ok(
                        Math.abs(turn) > 0 && Math.abs(turn) < 90,
                        `${what}: half way it is turned ${turn}`,
                    );
                    assert.ok(depth < ALOFT, `${what}: it never flies`);
                }
                const end = step.at.to.poses[slot];
                assert.equal(end.turn, 0, what);
                assert.equal(end.opacity, 1, what);
                assert.ok(end.depth < ALOFT, what);
                assert.equal(step.at.to.marks[slot].tapped, tapped, what);
                sounded(run, tapped ? "tap" : "untap", step.from, what);
            }
        }
        assert.ok(seen.tap >= 5, `${seen.tap} taps were seen`);
        assert.ok(seen.untap >= 2, `${seen.untap} untaps were seen`);
        assert.ok(
            seen.placed >= 2,
            `${seen.placed} were seen to begin in place`,
        );

        const left = combat.steps.filter(
            (s) => s.verb === "vanish" && s.act.why === "sacrifice",
        );
        assert.equal(left.length, 1, "one permanent is sacrificed");
        const [gone] = left;
        const { slot } = gone.act.subject;
        const turned = combat.steps.find(
            (s) => s.verb === "tap" && s.act.object.slot === slot,
        );
        assert.ok(turned, "it tapped before it left");
        const [from, to] = ["start", "end"].map((edge) =>
            told(combat, turned.act, edge),
        );
        assert.ok(to > from, "its turn takes time");
        // It goes after its turn is done, and nothing is played between the two but what its
        // own beat holds (the mana it was sacrificed for): its delay is the verb's own.
        const going = told(combat, gone.act, "start");
        assert.ok(going >= to, `it begins to go at ${going}, turned at ${to}`);
        assert.deepEqual(
            combat.edges
                .filter((e) => e.edge === "start" && e.at >= to && e.at < going)
                .map((e) => e.verb)
                .filter((verb) => verb !== "streak"),
            [],
            "nothing waits between its turn and its going",
        );
        const lying = samples(combat, to, going);
        assert.ok(lying.length > 0, "it was seen turned");
        for (const sample of lying) {
            assert.equal(sample.poses[slot].turn, 0, "it has done turning");
            assert.equal(sample.marks[slot].tapped, true);
            assert.equal(
                sample.poses[slot].opacity,
                1,
                "and has not begun to go",
            );
        }
    },

    // A creature lunges at what it deals combat damage to and drifts back to where it lay:
    // from its own place, over everything, toward the player's half of the table or its
    // blocker's card, and never turned or sized. The strikes of one combat begin one after
    // another, and creatures that strike one another are both on their way at once. A player
    // struck has their old life total as the strike begins and the new one by its end. Nothing
    // moves but what strikes.
    strike({ combat }) {
        const strikes = combat.steps.filter((s) => s.verb === "strike");
        assert.equal(strikes.length, 4, "the combat's four strikes");
        const strikers = strikes.map((s) => `${s.act.source}`);
        const starts = strikes.map((s) => s.from);
        assert.deepEqual(
            starts,
            [...new Set(starts)].sort((a, b) => a - b),
            "they begin one after another",
        );
        const seen = { player: 0, object: 0 };
        for (const strike of strikes) {
            const { source, to, amount } = strike.act;
            const what = `the strike of ${source} at ${strike.from} ms`;
            const was = before(combat, strike);
            const lay = was.poses[source];
            const tapped = was.marks[source].tapped;
            seen[to.kind] += 1;
            if (strike.at.from) {
                assert.deepEqual(
                    where(strike.at.from.poses[source]),
                    where(lay),
                    `${what}: it begins where it lay`,
                );
            }
            const half = strike.at.middle.poses[source];
            const moved = { x: half.x - lay.x, y: half.y - lay.y };
            const far = Math.hypot(moved.x, moved.y);
            assert.ok(half.depth >= ALOFT, `${what}: it flies over what rests`);
            assert.deepEqual(
                [half.width, half.turn, half.rotation, half.opacity],
                [lay.width, lay.turn, lay.rotation, lay.opacity],
                `${what}: it is not turned or sized`,
            );
            if (to.kind === "player") {
                // Toward the half of the player it strikes: the opponent's is the far one.
                const up = to.player === "opponent";
                assert.ok(
                    up === moved.y < 0 && far > lay.width,
                    `${what}: half way it has gone ${JSON.stringify(moved)}`,
                );
                const side = strike.at.from ?? was;
                assert.equal(
                    strike.at.to.life[to.player],
                    side.life[to.player] - amount,
                    `${what}: the life it takes`,
                );
            } else {
                // Along the line from its card's middle to the middle of what it strikes.
                const target = was.poses[to.slot];
                const from = cardMiddle(lay, tapped);
                const at = cardMiddle(target, was.marks[to.slot].tapped);
                const way = { x: at.x - from.x, y: at.y - from.y };
                const whole = Math.hypot(way.x, way.y);
                const along = (moved.x * way.x + moved.y * way.y) / whole;
                assert.ok(
                    Math.abs(far - along) < 0.01 &&
                        along > 0.3 * whole &&
                        along < whole,
                    `${what}: half way it has gone ${along} of ${whole}`,
                );
            }
            assert.deepEqual(
                where(strike.at.to.poses[source]),
                where(lay),
                `${what}: it is back where it lay`,
            );
            assert.ok(strike.at.to.poses[source].depth < ALOFT, what);
            // As its lunge lands, within the step.
            const lands = STRIKE.hit * (strike.to - strike.from);
            sounded(combat, "hit", strike.from, what, lands);
            for (const [name, pose] of Object.entries(was.poses)) {
                if (strikers.includes(name)) continue;
                for (const sample of Object.values(strike.at)) {
                    assert.deepEqual(
                        sample.poses[name],
                        pose,
                        `${what}: ${name} moved at ${sample.at} ms`,
                    );
                }
            }
        }
        assert.deepEqual(seen, { player: 1, object: 3 });
        // The blocked attacker and its blocker strike each other, each on its way at once.
        const pairs = strikes.filter((a) =>
            strikes.some(
                (b) =>
                    b.act.to.slot === a.act.source &&
                    a.act.to.slot === b.act.source &&
                    a.from < b.to &&
                    b.from < a.to,
            ),
        );
        assert.equal(
            pairs.length,
            2,
            "an attacker and its blocker strike at once",
        );
    },

    // A number changes at once, as its step begins, and says that it did for as long as the
    // step plays. A life total is the new one from the first, and the change stands beside it
    // (on the page, too) until the step is over: a loss to a strike and a gain. A counter's
    // badge, a planeswalker's loyalty and a power and toughness are their card's record, which
    // is the new one from the first; a counter leaves its card where it is.
    count({ combat, place }) {
        const seen = { loss: 0, gain: 0, loyalty: 0, counter: 0, stats: 0 };
        let shown = 0;
        for (const run of [combat, place]) {
            for (const step of run.steps.filter((s) => s.verb === "count")) {
                const { tally } = step.act;
                const what = `the count of ${tally.kind} at ${step.from} ms`;
                const was = before(run, step);
                const first = step.at.from ?? step.at.middle ?? step.at.to;
                if (tally.kind === "life") {
                    const { player, life } = tally;
                    const by = life - was.life[player];
                    if (by === 0) continue;
                    seen[by > 0 ? "gain" : "loss"] += 1;
                    assert.equal(first.life[player], life, what);
                    const during = samples(run, step.from, step.to - 1);
                    assert.ok(during.length > 0, `${what}: it was not sampled`);
                    for (const sample of during) {
                        const all = sample.counting.filter(
                            (c) => c.player === player,
                        );
                        assert.ok(all.length > 0, `${what}: no change says so`);
                        assert.equal(all.at(-1).life, life, what);
                    }
                    if (step.at.middle?.changes) {
                        shown += 1;
                        const { counting, changes } = step.at.middle;
                        const mine = counting.filter(
                            (c) => c.player === player,
                        );
                        assert.deepEqual(
                            changes.filter(([side]) => side === player),
                            [[player, mine.at(-1).life - mine[0].was]],
                            `${what}: the page shows the change beside the total`,
                        );
                    }
                    assert.deepEqual(
                        step.at.to.counting.filter(
                            (c) => c.player === player && c.life === life,
                        ),
                        [],
                        `${what}: the change is gone as the step ends`,
                    );
                    if (by > 0) sounded(run, "gain", step.from, what);
                    continue;
                }
                if (tally.kind === "pool") continue;
                const { slot, counters, pt } = tally.object;
                const kind =
                    tally.kind === "stats"
                        ? "stats"
                        : tally.counter === "loyalty"
                          ? "loyalty"
                          : "counter";
                seen[kind] += 1;
                assert.notDeepEqual(
                    [was.marks[slot].counters, was.marks[slot].pt],
                    [counters, pt ?? null],
                    `${what}: it changed nothing`,
                );
                for (const sample of Object.values(step.at)) {
                    assert.deepEqual(
                        [sample.marks[slot].counters, sample.marks[slot].pt],
                        [counters, pt ?? null],
                        `${what}: the card shows it at ${sample.at} ms`,
                    );
                    // A characteristic that changes may lay its card elsewhere (a creature
                    // made a Vehicle leaves the creatures' row); a counter moves nothing.
                    if (kind === "stats") continue;
                    assert.deepEqual(
                        where(sample.poses[slot]),
                        where(step.at.to.poses[slot]),
                        `${what}: its card stays as it is`,
                    );
                }
            }
        }
        assert.ok(seen.loss >= 1 && seen.gain >= 2, JSON.stringify(seen));
        assert.ok(shown >= 2, `the page was read at ${shown} life counts`);
        assert.ok(
            seen.loyalty >= 1 && seen.counter >= 1 && seen.stats >= 1,
            JSON.stringify(seen),
        );
    },

    // A face-down permanent turns face up where it lies (UI Conventions, "A card turning face
    // up"): it grows about its card's middle, over what rests, shows its back as it was until
    // half way and its face from there, and settles back where it lay; nothing else moves. A
    // card whose name you learn where it lies in a library (your scry) shows nothing and takes
    // no time: the look's prompt shows it.
    flip({ place, scry, seat }) {
        const flips = place.steps.filter((s) => s.verb === "flip");
        assert.equal(flips.length, 1, "one permanent turns face up");
        const [flip] = flips;
        const { slot } = flip.act.object;
        // As it begins it is as it was: whatever ended as it began has come to rest.
        const { from, middle, to } = flip.at;
        const lay = from.poses[slot];
        assert.equal(before(place, flip).zones[slot], "battlefield");
        assert.equal(before(place, flip).marks[slot].named, false);
        assert.equal(lay.face, "back", "it lies face down");
        assert.equal(from.marks[slot].named, false, "its back shows at first");
        const half = middle.poses[slot];
        assert.ok(half.width > lay.width * 1.1, "half way it has grown");
        near(
            cardMiddle(half, false),
            cardMiddle(lay, false),
            "it grows about its middle",
        );
        assert.ok(half.depth >= ALOFT, "over what rests");
        assert.equal(half.face, "front");
        assert.equal(
            middle.marks[slot].named,
            true,
            "its face shows from there",
        );
        assert.deepEqual(where(to.poses[slot]), where(lay), "it settles back");
        assert.ok(to.poses[slot].depth < ALOFT);
        assert.equal(to.marks[slot].named, true);
        for (const [name, pose] of Object.entries(from.poses)) {
            if (name === `${slot}`) continue;
            // As it ends, the step after it has begun in the same tick.
            assert.deepEqual(middle.poses[name], pose, `${name} moved`);
        }
        const learned = scry.edges.filter((e) => e.verb === "flip");
        assert.equal(learned.length, seat === "yours" ? 4 : 0);
        for (const edge of learned) {
            assert.equal(edge.act.object.zone, "library");
            assert.equal(
                told(scry, edge.act, "start"),
                told(scry, edge.act, "end"),
                "a name learned in a library takes no time",
            );
        }
    },

    // Flights that leave a card on the battlefield (`place`). A permanent that changes sides
    // crosses the table, over what rests, to its new controller's half. An Aura lands tucked
    // behind the permanent it is attached to, where the layout tucks it (a phone gives it a
    // place of its own in the row). And the cards at rest make room by one rule, on every row
    // and layout: as a land is played, each card it moves is on its way for as long as the
    // land flies, at the depth it rests at, and is where it then rests as the land lands:
    // nothing jumps and nothing but the land flies. On the desktop a seventh land piles its
    // controller's lands by name (UI Conventions, exception 5).
    fly({ place, seat, reflows }) {
        const flights = place.steps.filter((s) => s.verb === "fly");
        const taken = flights.filter(
            (s) =>
                s.act.from === "battlefield" &&
                s.act.object.zone === s.act.from,
        );
        assert.equal(taken.length, 1, "one permanent changes sides");
        for (const flight of taken) {
            const { slot, controller } = flight.act.object;
            const was = before(place, flight);
            assert.notEqual(was.marks[slot].controller, controller);
            onSide(
                place,
                was.poses[slot],
                seat === "yours" ? "theirs" : "yours",
                "where it lay",
            );
            assert.ok(flight.at.middle.poses[slot].depth >= ALOFT, "it flies");
            const end = flight.at.to.poses[slot];
            onSide(place, end, seat, "where it lands");
            assert.ok(end.depth < ALOFT);
            sounded(place, "control", flight.from, "the permanent taken");
        }
        const attached = flights.filter(
            (s) => (s.act.object.attachedTo ?? null) !== null,
        );
        assert.equal(attached.length, 1, "one Aura becomes attached");
        for (const flight of attached) {
            const { slot, attachedTo } = flight.act.object;
            const end = flight.at.to;
            assert.equal(end.zones[slot], "battlefield");
            assert.equal(end.marks[slot].attachedTo, attachedTo);
            if (reflows) continue;
            assert.ok(
                beside(end.poses[slot], end.poses[attachedTo]),
                "it lands on the permanent it is attached to",
            );
            assert.ok(
                end.poses[slot].depth < end.poses[attachedTo].depth,
                "tucked behind it",
            );
        }
        const [land] = flights.filter(
            (s) => s.act.from === "hand" && s.act.object.zone === "battlefield",
        );
        assert.ok(land, "a land is played");
        const was = before(place, land);
        const { middle, to } = land.at;
        let room = 0;
        for (const [name, pose] of Object.entries(was.poses)) {
            if (name === `${land.act.object.slot}`) continue;
            const end = to.poses[name];
            const half = middle.poses[name];
            if (JSON.stringify(where(end)) === JSON.stringify(where(pose))) {
                assert.deepEqual(where(half), where(pose), `${name} strayed`);
                continue;
            }
            room += 1;
            assert.equal(was.zones[name], to.zones[name], `${name} left`);
            assert.ok(half.depth < ALOFT, `${name} makes room at its depth`);
            assert.notDeepEqual(
                where(half),
                where(end),
                `${name} was at its new place at once`,
            );
        }
        assert.ok(room > 0, "the land's row made room for it");
        if (reflows) return;
        // Seven lands of two names: each name's lie on one pile, and the piles apart.
        const lands = Object.keys(to.poses).filter(
            (name) =>
                to.zones[name] === "battlefield" &&
                beside(
                    { ...to.poses[name], width: 4 * to.poses[name].width },
                    to.poses[land.act.object.slot],
                ) &&
                to.marks[name].pt === null &&
                to.marks[name].counters.length === 0,
        );
        assert.equal(lands.length, 7, `the lands: ${lands}`);
        const piles = [];
        for (const name of lands) {
            const pile = piles.find((p) =>
                beside(to.poses[p[0]], to.poses[name]),
            );
            if (pile) pile.push(name);
            else piles.push([name]);
        }
        assert.deepEqual(
            piles.map((p) => p.length).sort(),
            [3, 4],
            "seven lands pile by name",
        );
    },
};
