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
    effectsOn,
    onSide,
    sounded,
    where,
} from "./verb-run.mjs";
import { NEAR, STRIKE } from "../src/stage/tracks.ts";

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
    // turns there, at the depth it rests at; only once it has turned do it and its row go to
    // where they now rest. Attackers tap together, lands as they pay, and all untap together.
    // A permanent that taps and then leaves (a Treasure, tapped and sacrificed for mana) has
    // done turning as it begins to go: the two are one beat, its payment being one event of
    // the rules, and the departure waits in it for the turn (`timingOf`), beginning at the
    // stage time the turn ends, with no wait between (the old board held 450 ms there).
    tap({ combat, place }) {
        const seen = { tap: 0, untap: 0, placed: 0, waited: 0 };
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
                    // Half way it is still turning where it lay, or onto a place hardly any
                    // way off (an attacker's, a step forward), and its row has yet to close up
                    // or make room: nothing else on the battlefield has left its place.
                    if (rested(run, step)) {
                        const [now, lay] = [
                            cardMiddle(step.at.middle.poses[slot], tapped),
                            cardMiddle(was.poses[slot], !tapped),
                        ];
                        const off = Math.hypot(now.x - lay.x, now.y - lay.y);
                        assert.ok(
                            off <= NEAR * step.at.to.poses[slot].width,
                            `${what}: it turns ${off} from where it lay`,
                        );
                        for (const [name, pose] of Object.entries(was.poses)) {
                            const now = step.at.middle.poses[name];
                            if (was.zones[name] !== "battlefield") continue;
                            if (now === undefined || now.turn !== 0) continue;
                            near(
                                now,
                                pose,
                                `${what}: ${name} waits for the turn`,
                            );
                            seen.waited += 1;
                        }
                    }
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
        assert.ok(seen.waited >= 4, `${seen.waited} were seen to wait`);

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

    // A creature lunges at what it deals combat damage to and draws back to where it lay:
    // from its own place, over everything, toward the player's half of the table or its
    // blocker's card, and never turned or sized. It stops where its card first overlaps the
    // card it strikes, or at the rim of a player's life total, and a burst of light is on what
    // it strikes as the blow lands, when a player's total changes too. What is much smaller
    // than it, it shrinks toward the size of and lands on. The strikes of one combat begin one after another,
    // but that creatures that strike one another do so as one, at one time, and meet half
    // way. A player struck has their old life total as the strike begins and the new one by
    // its end. Nothing moves but what strikes.
    strike({ combat }) {
        const strikes = combat.steps.filter((s) => s.verb === "strike");
        assert.equal(strikes.length, 4, "the combat's four strikes");
        const strikers = strikes.map((s) => `${s.act.source}`);
        const starts = strikes.map((s) => s.from);
        assert.deepEqual(
            starts,
            starts.toSorted((a, b) => a - b),
            "they begin one after another",
        );
        assert.equal(new Set(starts).size, 3, "but for the two of the fight");
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
            // How far its card's middle has gone: it may be smaller there (below).
            const half = strike.at.middle.poses[source];
            const [here, there] = [lay, half].map((p) => cardMiddle(p, tapped));
            const moved = { x: there.x - here.x, y: there.y - here.y };
            const far = Math.hypot(moved.x, moved.y);
            assert.ok(half.depth >= ALOFT, `${what}: it flies over what rests`);
            assert.deepEqual(
                [half.turn, half.rotation, half.opacity],
                [lay.turn, lay.rotation, lay.opacity],
                `${what}: it is not turned`,
            );
            // Its own size, unless it dives onto something much smaller than itself (a
            // phone's small permanents and life totals), which it shrinks to the size of.
            const small =
                to.kind === "object"
                    ? was.poses[to.slot].width / lay.width
                    : undefined;
            assert.ok(
                small === undefined || small < STRIKE.small.from
                    ? half.width <= lay.width && half.width > 0.4 * lay.width
                    : half.width === lay.width,
                `${what}: half way it is ${half.width} wide of ${lay.width}`,
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
                // The blow bursts on their life total, which changes as it lands: the count
                // of the life it takes begins there, on the stage time of the blow.
                assert.ok(
                    effectsOn(strike.at.middle, to.player).includes(
                        "impact over",
                    ),
                    `${what}: the blow shows on the player's total`,
                );
                const lands =
                    strike.from + STRIKE.hit * (strike.to - strike.from);
                const taken = combat.steps.find(
                    (s) =>
                        s.verb === "count" &&
                        s.act.tally.player === to.player &&
                        s.from >= strike.from,
                );
                assert.ok(
                    taken && Math.abs(taken.from - lands) < 1e-6,
                    `${what}: its life goes at ${taken?.from}, the blow lands at ${lands}`,
                );
            } else {
                // Along the line from its card's middle to the middle of what it strikes.
                const lies = was.marks[to.slot].tapped;
                const from = cardMiddle(lay, tapped);
                const at = cardMiddle(was.poses[to.slot], lies);
                const way = { x: at.x - from.x, y: at.y - from.y };
                const whole = Math.hypot(way.x, way.y);
                const along = (moved.x * way.x + moved.y * way.y) / whole;
                assert.ok(
                    Math.abs(far - along) < 0.01 && along > 0,
                    `${what}: half way it has gone ${along} of ${whole}`,
                );
                // Half way is just past the blow: the two cards overlap by a little less
                // than `bite` of its width on the side they meet, however far it came,
                // unless they lay closer.
                const reach = (pose, lying) => {
                    const long = pose.width / CARD_ASPECT;
                    return lying
                        ? { x: long / 2, y: pose.width / 2 }
                        : { x: pose.width / 2, y: long / 2 };
                };
                const struck = strike.at.middle.poses[to.slot];
                const now = [there, cardMiddle(struck, lies)];
                const [mine, theirs] = [
                    reach(half, tapped),
                    reach(struck, lies),
                ];
                const over = Math.min(
                    mine.x + theirs.x - Math.abs(now[1].x - now[0].x),
                    mine.y + theirs.y - Math.abs(now[1].y - now[0].y),
                );
                const bite = STRIKE.bite * lay.width;
                if (small >= STRIKE.small.from) {
                    assert.ok(
                        over > 0.4 * bite &&
                            (over < bite + 0.5 ||
                                along < STRIKE.least * lay.width + 0.5),
                        `${what}: at the blow they overlap ${over} of a width ${lay.width}`,
                    );
                } else if (small <= STRIKE.small.to) {
                    // It has landed on it: its middle is over the card it strikes.
                    const long = struck.width / CARD_ASPECT;
                    assert.ok(
                        Math.abs(now[1].x - now[0].x) < long / 2 &&
                            Math.abs(now[1].y - now[0].y) < long / 2,
                        `${what}: it has landed ${JSON.stringify(now)}`,
                    );
                }
                assert.deepEqual(
                    effectsOn(strike.at.middle, to.slot),
                    ["impact over"],
                    `${what}: the blow shows on what it strikes`,
                );
                if (strike.at.from) {
                    assert.deepEqual(effectsOn(strike.at.from, to.slot), []);
                }
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
        // The blocked attacker and its blocker strike each other as one.
        const pairs = strikes.filter((a) =>
            strikes.some(
                (b) =>
                    b.act.to.slot === a.act.source &&
                    a.act.to.slot === b.act.source &&
                    a.from === b.from &&
                    a.to === b.to,
            ),
        );
        assert.equal(
            pairs.length,
            2,
            "an attacker and its blocker strike at one time",
        );
    },

    // A number changes at once, as its step begins, and says that it did for as long as the
    // step plays. A life total is the new one from the first, it flares for the whole step,
    // red for a loss and green for a gain, and the change stands beside it (on the page, too)
    // and drifts away, down for a loss and up for a gain, until the step is over: a loss to a
    // strike and a gain. A counter's badge, a planeswalker's loyalty and a power and
    // toughness are their card's record, which is the new one from the first, and a ring of
    // light pulses round the card; a counter leaves its card where it is.
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
                    const flare = by > 0 ? "gain" : "loss";
                    for (const sample of during) {
                        const lit = (sample.effects[player] ?? []).find(
                            (e) => e.name === flare,
                        );
                        const through =
                            (sample.at - step.from) / (step.to - step.from);
                        assert.ok(
                            lit && Math.abs(lit.progress - through) < 1e-6,
                            `${what}: its ${flare} at ${sample.at} ms is ${JSON.stringify(lit)}`,
                        );
                    }
                    if (step.at.middle?.changes) {
                        shown += 1;
                        const { counting, changes } = step.at.middle;
                        const mine = counting.filter(
                            (c) => c.player === player,
                        );
                        const said = changes.filter(
                            ([side]) => side === player,
                        );
                        assert.deepEqual(
                            said.map(([side, change]) => [side, change]),
                            [[player, mine.at(-1).life - mine[0].was]],
                            `${what}: the page shows the change beside the total`,
                        );
                        // Alone, it has drifted half way: down for a loss, up for a gain.
                        if (mine.length === 1) {
                            const [, , drift] = said[0];
                            assert.ok(
                                drift !== 0 && drift < 0 === by > 0,
                                `${what}: the change has drifted ${drift}`,
                            );
                        }
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
                assert.ok(
                    effectsOn(first, slot).includes("pulse over"),
                    `${what}: its card pulses`,
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
    // A veil of light is over it for the whole step, drawn over its face, so that the face is
    // not seen to change. A card whose name you learn where it lies in a library (your scry)
    // shows nothing and takes no time: the look's prompt shows it.
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
        assert.deepEqual(effectsOn(from, slot), ["cover over"]);
        assert.deepEqual(effectsOn(middle, slot), ["cover over"]);
        assert.equal(middle.effects[slot][0].progress, 0.5);
        assert.deepEqual(effectsOn(to, slot), [], "the veil has cleared");
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
        assert.deepEqual(
            effectsOn(land.at.to, land.act.object.slot),
            [],
            "it lands with no flash, as Arena's land does",
        );
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
