// The verb suite's cases for cards entering and leaving piles beyond `zones` (verbs.mjs plays
// them beside verb-cases.mjs's, in the same check): `opening`, the hands dealt before the
// first turn and the mulligans that send them back (the seeded fixtures opening-bottom and
// verbs-opening-theirs), and `piles`, the board verbs-piles: cards milled, a card cast from a
// graveyard and one from exile, a card exiled under its host and back as the host leaves, and
// a token that enters attacking. Nothing here touches a page.
import assert from "node:assert/strict";
import {
    ALOFT,
    before,
    beside,
    CARD_ASPECT,
    onSide,
    where,
} from "./verb-cases.mjs";

const routeOf = (step) => `${step.act.from}>${step.act.object.zone}`;
const placeOf = ({ x, y, width }) => ({ x, y, width });
/** Which half of the table a card of `owner` (as the recording's seat names them) lies in,
 * as `onSide` names it: the near half is `yours`. */
const sideOf = (owner) => (owner === "you" ? "yours" : "theirs");

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
        for (const pile of ["graveyard", "exile"]) {
            const cast = flights.filter((s) => routeOf(s) === `${pile}>stack`);
            assert.equal(cast.length, 1, `cards cast from ${pile}`);
            const [flight] = cast;
            const name = flight.act.object.slot;
            const what = `the cast from ${pile} of ${name}`;
            const was = before(piles, flight);
            assert.equal(was.zones[name], pile, what);
            if (flight.at.from) {
                assert.deepEqual(
                    where(flight.at.from.poses[name]),
                    where(was.poses[name]),
                    `${what}: it leaves from where it lay`,
                );
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
    // (UI Conventions, exception 6).
    vanish({ piles }) {
        const tucked = piles.steps.filter(
            (s) => s.verb === "vanish" && s.act.object?.exiledBy != null,
        );
        assert.equal(tucked.length, 1, "cards exiled under a host");
        const [gone] = tucked;
        const name = gone.act.subject.slot;
        const host = gone.act.object.exiledBy;
        const { zones, poses } = gone.at.to;
        assert.equal(zones[name], "exile");
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
};
