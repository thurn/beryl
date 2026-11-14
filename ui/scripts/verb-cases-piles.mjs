// The verb suite's cases for cards entering and leaving piles beyond `zones` (verbs.mjs plays
// them beside verb-cases.mjs's, in the same check): `opening`, the hands dealt before the
// first turn and the mulligans that send them back (the seeded fixtures opening-bottom and
// verbs-opening-theirs), and `piles`, the board verbs-piles: cards milled, a card cast from a
// graveyard and one from exile, a card exiled under its host and back as the host leaves, and
// a token that enters attacking; and `reveal`, the board verbs-reveal: Trinket Mage finds Mana
// Crypt, revealed before it goes to the hand, and an Oust then puts a face-down Snapcaster
// Mage into its library, revealed as it goes. Nothing here touches a page.
import assert from "node:assert/strict";
import {
    ALOFT,
    before,
    beside,
    CARD_ASPECT,
    onSide,
    leavesFrom,
    where,
} from "./verb-run.mjs";

const routeOf = (step) => `${step.act.from}>${step.act.object.zone}`;
const placeOf = ({ x, y, width }) => ({ x, y, width });
/** Which half of the table a card of `owner` (as the recording's seat names them) lies in,
 * as `onSide` names it: the near half is `yours`. */
const sideOf = (owner) => (owner === "you" ? "yours" : "theirs");

/** The name of the face the Revealed Cards pile holds of the card in `slot`
 * (src/stage/layout.ts `revealedName`). */
const faceOf = (slot) => `r${slot}`;

/** The `lift` case's part for the board verbs-dig, a Stock Up that takes two of the five
 * cards it looks at to the hand and puts the other three on the bottom one choice at a time.
 * Neither seat lifts anything: Arena holds no card of the opponent's Stock Up off their
 * library while they choose (`3sEoP_s4DQE 46:04.0`, `0VsYGs88NXA 1:13:06.3`,
 * `WCknTZNbraE 1:22:28.2`). The two cards taken fly from the library to the hand once each,
 * and no other card of the library leaves it. (Before, the five came out, two went to the
 * hand, three came out again, went back, and moved again.) */
export function dug(dig, seat) {
    const lifts = dig.steps.filter((s) => s.verb === "lift");
    assert.deepEqual(lifts, [], `${seat}: a Stock Up lifts nothing`);
    const taken = dig.steps.filter(
        (s) =>
            s.verb === "fly" &&
            s.act.from === "library" &&
            s.act.object.zone === "hand" &&
            s.act.why === "effect",
    );
    assert.equal(taken.length, 2, "two of the five go to the hand");
    const first = taken[0];
    const lay = before(dig, first);
    const names = taken.map((s) => String(s.act.object.slot));
    assert.equal(new Set(names).size, 2, `each leaves once: ${names}`);
    for (const flight of taken) {
        const name = String(flight.act.object.slot);
        const [start, end] = ["from", "to"].map(
            (at) => flight.at[at]?.poses[name],
        );
        if (start && end) {
            leavesFrom(start, lay.poses[name], end, `${name} to the hand`);
        }
        // It drops into the table on its way, as Arena's does: half way, it is more than
        // two card widths further into the table than where it left (Arena's is about 0.38
        // of the table's height, `3sEoP_s4DQE 46:04.37`).
        const half = flight.at.middle?.poses[name];
        if (half && seat === "theirs") {
            const into = Math.abs(half.y - lay.poses[name].y);
            assert.ok(
                into > 2 * lay.poses[name].width,
                `${name} is ${into} px into the table half way`,
            );
        }
    }
    // Nothing else of that library leaves it while the look resolves.
    const library = Object.keys(lay.zones).filter(
        (name) => lay.zones[name] === "library" && !names.includes(name),
    );
    const until = taken.at(-1).to;
    for (const sample of dig.samples) {
        if (sample.at < first.from || sample.at > until) continue;
        for (const name of library) {
            if (!sample.poses[name] || !lay.poses[name]) continue;
            assert.ok(
                beside(sample.poses[name], lay.poses[name]),
                `${name} left its library at ${sample.at} ms`,
            );
        }
    }
}

export const CASES = {
    // Milled cards fly from the library to the graveyard face up, one after another, and
    // none is held up on the way (UI Conventions, not seen in Arena: "Milled cards"). A card
    // cast from a graveyard (flashback) or from exile flies to the stack from where it lay,
    // as one cast from a hand does. A card exiled
    // under its host flies back to the battlefield from under it as the host leaves (not
    // seen: "A linked exile returning").
    fly({ piles }) {
        const flights = piles.steps.filter((s) => s.verb === "fly");
        const milled = flights.filter(
            (s) => routeOf(s) === "library>graveyard",
        );
        assert.ok(milled.length >= 2, `${milled.length} cards milled`);
        for (const [i, flight] of milled.entries()) {
            const name = flight.act.object.slot;
            const what = `the mill of ${name} at ${flight.from} ms`;
            const [start, half, end] = ["from", "middle", "to"].map(
                (at) => flight.at[at]?.poses[name],
            );
            assert.equal(flight.at.to.zones[name], "graveyard", what);
            assert.equal(end.face, "front", `${what}: it lands face up`);
            assert.ok(end.depth < ALOFT, `${what}: it lands among what rests`);
            if (half) assert.ok(half.depth >= ALOFT, `${what}: it flies`);
            if (start && half) {
                assert.ok(
                    half.width <= Math.max(start.width, end.width) + 0.01,
                    `${what}: it is not held up`,
                );
            }
            if (i) {
                assert.ok(
                    milled[i - 1].from < flight.from,
                    `${what}: begins after the one before it`,
                );
            }
        }
        // Cards put back from a hand fly straight to the library (not seen: "Cards a look
        // returns"). The engine moves them as one group (CR 401.4), so each leaves a beat
        // after the one before it, overlapping its flight, as cards exiled together do.
        const returned = flights.filter((s) => routeOf(s) === "hand>library");
        assert.ok(returned.length >= 2, `${returned.length} cards put back`);
        for (const [i, flight] of returned.entries()) {
            const name = flight.act.object.slot;
            const what = `the put-back of ${name} at ${flight.from} ms`;
            const was = before(piles, flight);
            const [start, half, end] = ["from", "middle", "to"].map(
                (at) => flight.at[at]?.poses[name],
            );
            assert.equal(was.zones[name], "hand", what);
            if (start) leavesFrom(start, was.poses[name], end, what);
            assert.ok(half.depth >= ALOFT, `${what}: it flies`);
            assert.ok(
                half.width <= Math.max(was.poses[name].width, end.width) + 0.01,
                `${what}: straight, not held up`,
            );
            assert.equal(flight.at.to.zones[name], "library", what);
            if (i) {
                assert.ok(
                    returned[i - 1].from < flight.from,
                    `${what}: begins after the one before it`,
                );
            }
        }
        for (const pile of ["graveyard", "exile"]) {
            const cast = flights.filter((s) => routeOf(s) === `${pile}>stack`);
            assert.equal(cast.length, 1, `cards cast from ${pile}`);
            const [flight] = cast;
            const name = flight.act.object.slot;
            const what = `the cast from ${pile} of ${name}`;
            const was = before(piles, flight);
            assert.equal(was.zones[name], pile, what);
            if (flight.at.from) {
                const start = flight.at.from.poses[name];
                const lay = was.poses[name];
                // Priority has no motion step. A playable card can move from its pile into
                // the hand fan after the preceding sample, before its cast begins.
                if (start.width > lay.width * 2) {
                    assert.ok(
                        start.y >= lay.y,
                        `${what}: it leaves the hand fan`,
                    );
                } else {
                    assert.deepEqual(
                        where(start),
                        where(lay),
                        `${what}: it leaves from where it lay`,
                    );
                }
            }
            assert.ok(
                flight.at.middle.poses[name].depth >= ALOFT,
                `${what}: it flies`,
            );
            assert.equal(flight.at.to.zones[name], "stack", what);
        }
        const exiled = piles.steps.find(
            (s) => s.verb === "vanish" && s.act.object?.exiledBy != null,
        );
        assert.ok(exiled, "no card was exiled under its host");
        const name = exiled.act.subject.slot;
        const host = exiled.act.object.exiledBy;
        const back = flights.find(
            (s) =>
                s.act.object.slot === name &&
                routeOf(s) === "exile>battlefield",
        );
        assert.ok(back, `${name} never flew back from exile`);
        const leaves = piles.steps.find(
            (s) => s.verb === "vanish" && s.act.subject.slot === host,
        );
        assert.ok(leaves, `its host ${host} never left`);
        assert.equal(back.from, leaves.from, "it returns as its host leaves");
        const was = before(piles, back);
        assert.ok(
            beside(was.poses[name], was.poses[host]),
            "it leaves from under its host",
        );
        assert.ok(back.at.middle.poses[name].depth >= ALOFT, "it flies back");
        const end = back.at.to.poses[name];
        assert.ok(end.depth < ALOFT, "it lands among what rests");
        onSide(
            piles,
            end,
            sideOf(back.act.object.controller),
            "the permanent back",
        );
    },

    // A card exiled under its host rests tucked under it once it has gone from where it lay
    // (UI Conventions, exception 6). Nothing makes room for a card that leaves until it has
    // gone: half way through the step of a card that leaves alone, and as that step ends, all
    // else is where it lay, as deep as it lay, but for what lies where it lands (Arena's row
    // closes up after, 3sEoP_s4DQE 24:49.6).
    vanish({ piles, zones }) {
        for (const run of [zones, piles]) {
            const left = run.steps.filter(
                (s) =>
                    s.verb === "vanish" &&
                    s.act.subject.kind === "object" &&
                    s.to > s.from,
            );
            // One that leaves while another card moves, each making room for whichever has
            // gone or arrived, is left out.
            const moving = run.steps.filter((s) =>
                ["fly", "appear", "vanish", "emerge"].includes(s.verb),
            );
            const alone = left.filter((gone) =>
                moving.every(
                    (other) =>
                        other === gone ||
                        other.to <= gone.from ||
                        gone.to <= other.from,
                ),
            );
            assert.ok(alone.length > 0, "no card left alone");
            for (const gone of alone) {
                // As it began: a step that ended as it began has by then put its own card
                // on its pile.
                const was = (gone.at.from ?? before(run, gone)).poses;
                const into = gone.act.object?.zone;
                for (const when of ["middle", "to"]) {
                    const sample = gone.at[when];
                    if (!sample) continue;
                    for (const [name, pose] of Object.entries(was)) {
                        if (Number(name) === gone.act.subject.slot) continue;
                        if (!sample.poses[name]) continue;
                        if (when === "to" && sample.zones[name] === into)
                            continue;
                        assert.deepEqual(
                            where(sample.poses[name]),
                            where(pose),
                            `${name} made room by the ${when} of the vanish at ${gone.from} ms`,
                        );
                        // Nor does it change depth while the card goes: a spell over one
                        // that leaves the stack stays over it. As the step ends, what
                        // flies next is already aloft.
                        if (when !== "middle") continue;
                        assert.equal(
                            sample.poses[name].depth,
                            pose.depth,
                            `${name} changed depth half way through the vanish at ${gone.from} ms`,
                        );
                    }
                }
            }
        }
        const tucked = piles.steps.filter(
            (s) => s.verb === "vanish" && s.act.object?.exiledBy != null,
        );
        assert.equal(tucked.length, 1, "cards exiled under a host");
        const [gone] = tucked;
        const name = gone.act.subject.slot;
        const host = gone.act.object.exiledBy;
        const { poses } = gone.at.to;
        assert.equal(gone.at.to.zones[name], "exile");
        assert.ok(beside(poses[name], poses[host]), "it rests by its host");
        assert.ok(poses[name].depth < poses[host].depth, "it rests under it");
    },

    // An opening hand is dealt as cards that come to be seen in the hand, none flying: yours
    // face up and fanned, the opponent's face down on their side (Arena's case "opening
    // hand"). A mulligan's hand goes from sight where it lay before the next is dealt. A
    // token that enters attacking appears where the attackers are, beside the creature that
    // made it attack (not seen: "A creature that enters attacking").
    appear({ opening, piles }) {
        const dealt = opening.steps.filter(
            (s) => s.verb === "appear" && s.act.why === "draw",
        );
        assert.ok(dealt.length >= 14, `${dealt.length} cards dealt`);
        for (const step of dealt) {
            const { slot: name, owner } = step.act.object;
            const what = `the deal of ${name} at ${step.from} ms`;
            assert.equal(before(opening, step).zones[name], "library", what);
            if (step.at.from) {
                assert.equal(step.at.from.poses[name].opacity, 0, what);
            }
            for (const sample of Object.values(step.at)) {
                assert.ok(sample.poses[name].depth < ALOFT, `${what}: flies`);
            }
            const end = step.at.to.poses[name];
            assert.equal(step.at.to.zones[name], "hand", what);
            assert.equal(end.opacity, 1, what);
            assert.equal(
                end.face,
                step.act.object.name === null ? "back" : "front",
                what,
            );
            onSide(opening, end, sideOf(owner), what);
        }
        const mulligans = opening.steps.filter(
            (s) =>
                s.verb === "vanish" &&
                s.act.from === "hand" &&
                s.act.object?.zone === "library",
        );
        assert.ok(mulligans.length >= 7, "no hand went back");
        for (const gone of mulligans) {
            const next = dealt.find((s) => s.from >= gone.from);
            if (!next) continue;
            assert.ok(
                gone.to <= next.from,
                `a hand going back at ${gone.from} ms is gone as the next is dealt at ${next.from}`,
            );
        }

        // Each hand dealt is fanned: its cards side by side as its deal ends.
        const deals = Map.groupBy(
            dealt,
            (s) =>
                `${s.act.object.owner} ${mulligans.filter((m) => m.from < s.from).length}`,
        );
        for (const [deal, steps] of deals) {
            const { poses } = steps.at(-1).at.to;
            const xs = steps
                .map((s) => poses[s.act.object.slot].x)
                .sort((a, b) => a - b);
            assert.ok(
                xs.length >= 5 && xs.every((x, i) => i === 0 || x > xs[i - 1]),
                `the hand dealt (${deal}) lies at x ${xs}`,
            );
        }
        const token = piles.steps.find(
            (s) =>
                s.verb === "appear" &&
                s.act.object.combat?.kind === "attacking",
        );
        assert.ok(token, "no token entered attacking");
        const name = token.act.object.slot;
        const end = token.at.to.poses[name];
        for (const sample of Object.values(token.at)) {
            const pose = sample.poses[name];
            assert.deepEqual(placeOf(pose), placeOf(end), "it is at its place");
            assert.ok(pose.depth < ALOFT, "it never flies");
        }
        const declared = piles.edges.findLast(
            (e) =>
                e.edge === "start" &&
                e.at <= token.from &&
                e.act.verb === "set" &&
                e.act.fact.object?.combat?.kind === "attacking",
        );
        assert.ok(declared, "no attacker was declared before it");
        // In the attacker's row: their middles less than half a card apart up or down.
        const attacker = token.at.to.poses[declared.act.fact.object.slot];
        const [a, b] = [attacker, end].map(
            (pose) => pose.y + pose.width / CARD_ASPECT / 2,
        );
        assert.ok(
            Math.abs(a - b) <
                Math.min(attacker.width, end.width) / CARD_ASPECT / 2,
            `its middle is at y ${b}, the attacker's at ${a}`,
        );
        onSide(piles, end, sideOf(token.act.object.controller), "it");
    },

    // A card held up to be read joins the Revealed Cards pile, at the top left of either
    // seat's table: a face of it comes to be seen there, where the layout rests it, and
    // travels nowhere, while the card itself lies where it was, so the step that then moves
    // the card finds it there. What another spell or ability reveals takes the pile's place:
    // the face-down permanent's face is there alone as it leaves. The card Trinket Mage finds
    // is its controller's own reveal, so only the opponent's seat holds it up: yours sees its
    // flight to the hand alone, and never a pile for a card you found yourself.
    showcase({ reveal, seat }) {
        const shown = reveal.steps.filter((s) => s.verb === "showcase");
        assert.equal(
            shown.length,
            seat === "yours" ? 1 : 2,
            "the cards revealed to the seat",
        );
        const found = seat === "yours" ? null : shown[0];
        const unveiled = shown[shown.length - 1];
        for (const step of shown) {
            const { slot, name } = step.act.object;
            const face = faceOf(slot);
            const { from, middle, to } = step.at;
            assert.ok(name, "the seat reads the card it is shown");
            assert.equal(step.to - step.from, 200, "a showcase's 200 ms");
            assert.ok(
                !(face in before(reveal, step).poses),
                "it was not there",
            );
            const lies = to.poses[face];
            const [first, half] = [from.poses[face], middle.poses[face]];
            assert.equal(first.opacity, 0, "it is unseen at first");
            assert.ok(first.width < half.width, "it grows");
            assert.ok(half.width < lies.width, "half way it is not whole");
            assert.ok(
                half.opacity > 0.5,
                `half way it is ${half.opacity} seen`,
            );
            assert.equal(lies.opacity, 1);
            for (const pose of [first, half]) {
                const [here, there] = [pose, lies].map(({ x, y, width }) => [
                    x + width / 2,
                    y + width / CARD_ASPECT / 2,
                ]);
                assert.ok(
                    Math.abs(here[0] - there[0]) < 0.01 &&
                        Math.abs(here[1] - there[1]) < 0.01,
                    `it grows about its middle: ${here} to ${there}`,
                );
            }
            assert.equal(lies.face, "front");
            assert.ok(lies.depth < ALOFT, "at rest, under what flies");
            assert.ok(
                lies.x + lies.width < reveal.middle &&
                    lies.y < reveal.middle / 2,
                `at the top left: ${JSON.stringify(where(lies))}`,
            );
            assert.equal(to.zones[face], "nowhere", "its face is in no zone");
        }
        if (found) {
            // The card found lies in its library while its face comes up, and then flies to
            // the hand; nothing else moves meanwhile.
            const { slot } = found.act.object;
            for (const [name, pose] of Object.entries(found.at.from.poses)) {
                if (name === faceOf(slot)) continue;
                // As it ends, the step after it has begun in the same tick.
                assert.deepEqual(
                    found.at.middle.poses[name],
                    pose,
                    `${name} moved`,
                );
            }
            assert.equal(found.at.middle.zones[slot], "library");
            const taken = reveal.steps.find(
                (s) => s.verb !== "showcase" && s.act.object?.slot === slot,
            );
            assert.equal(
                taken?.act.object.zone,
                "hand",
                "it then goes to the hand",
            );
            assert.ok(taken.from >= found.to, "once it has been held up");
            // Its face stays in the pile until another reveal takes its place.
            const until = before(reveal, unveiled);
            assert.equal(
                until.poses[faceOf(slot)]?.opacity,
                1,
                "the pile stays",
            );
            assert.ok(
                !(faceOf(slot) in unveiled.at.to.poses),
                "another's replaces it",
            );
        }
        const gone = unveiled.act.object.slot;
        // The face-down permanent is held up in the beat that takes it: its face comes up
        // while it goes from sight where it lay, face down for the seat that did not know it.
        const leaves = reveal.steps.find(
            (s) => s.verb === "vanish" && s.act.object?.slot === gone,
        );
        assert.ok(leaves, "the permanent leaves");
        assert.ok(
            unveiled.from >= leaves.from && unveiled.from < leaves.to,
            "it is held up as it goes",
        );
        assert.equal(before(reveal, leaves).zones[gone], "battlefield");
        assert.equal(
            before(reveal, leaves).poses[gone].face,
            "back",
            `it lay face down for ${seat}`,
        );
    },
};
