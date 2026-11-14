// What a card that goes to a pile is drawn as on its way, with no browser (`npm test`): a
// card that will lie under others on a pile, where only its paper edge shows
// (`Dress.rim`), shows its face until it is there (src/stage/enter.ts `coverOf`, tracks.ts
// `shownUntil`).
import assert from "node:assert/strict";
import { test } from "node:test";
import { row, object } from "./stage-table.mjs";
import { card, prompted, split, table } from "./stage-prompted.mjs";

/** The tests' rows, with your graveyard and exile each a pile as the table's, at x 4000 and
 * 5000: a card lies 10 to the right of the one over it, counted from the top, and only the
 * top one shows more than its paper edge (`Dress.rim`). A card the stage says has yet to
 * arrive (`leaving`) is off its pile, which lies as it did, as desktop.ts lays it. */
const graves = {
    ...row,
    rest(table, leaving = new Set()) {
        const out = row.rest(table);
        const { objects, players } = table.view;
        // So is a card a prompt holds up.
        const away = new Set([
            ...leaving,
            ...(table.tray?.piles ?? []).flatMap((pile) => pile.cards),
        ]);
        const exile = objects.filter((o) => o.zone === "exile");
        const piles = [
            ["graveyard", 4000, players.you.graveyard],
            ["exile", 5000, exile.map((o) => o.slot)],
        ];
        for (const [zone, x, pile] of piles) {
            const arrived = pile.filter((slot) => !away.has(slot));
            pile.forEach((slot, i) => {
                if (out.get(slot)?.dress.look === "shown") return;
                const depth = away.has(slot)
                    ? pile.length - 1 - i
                    : arrived.length - 1 - arrived.indexOf(slot);
                out.set(slot, {
                    object: objects.find((o) => o.slot === slot),
                    pose: {
                        x: x + depth * 10,
                        y: 500,
                        width: 100,
                        rotation: 0,
                        skew: 0,
                        turn: 0,
                        opacity: 1,
                        depth: i,
                        face: "front",
                    },
                    dress: {
                        zone,
                        look: "pile",
                        frame: 100,
                        back: false,
                        rim: depth > 0,
                    },
                });
            });
        }
        return out;
    },
};

/** A cost that exiles three cards of a graveyard, picked from a tray, as Abhorrent
 * Oculus's: the graveyard holds 30 under 12, 11 and 10, and those three are picked. The
 * answer's script exiles 10, then 11, then 12, in a beat each or all in `one` beat, whose
 * flights `flight` times. (Exile lies in the order of slots here, as the test's rows read
 * it, which is the order the three arrive in.) Gives where each card is and whether each
 * of 10, 11, 12 and 30 is drawn as only its edge (`Dress.rim`). */
async function exileThree(one, flight) {
    const { made, surface, prompts, reply } = prompted(graves, flight);
    const picked = [10, 11, 12];
    const asked = {
        ...split([], []),
        purpose: "cost",
        marks: picked.map((slot) => ({ on: card(slot), mark: "eligible" })),
        body: {
            shape: "pick",
            tray: picked,
            least: 3,
            most: 3,
            picked: picked.map(card),
            warned: [],
        },
    };
    const others = [object(1), object(3, "hand"), object(30, "graveyard")];
    const players = (graveyard) => ({
        you: { life: 20, library: [], graveyard },
        opponent: { life: 20, library: [], graveyard: [] },
    });
    prompts.play(
        table(asked, {
            objects: [...others, ...picked.map((s) => object(s, "graveyard"))],
            players: players([30, 12, 11, 10]),
        }),
    );
    made.clock.advance(300);
    prompts.press({ kind: "confirm" });
    await reply({ kind: "answered" });
    const exiled = picked.map((slot) => object(slot, "exile"));
    const steps = exiled.map((o) => ({
        act: { verb: "fly", from: "graveyard", why: "cost", object: o },
    }));
    prompts.play(
        table(undefined, {
            objects: [...others, ...exiled],
            players: players([30]),
        }),
        { beats: one ? [steps] : steps.map((step) => [step]) },
    );
    made.clock.advance(0);
    return {
        made,
        at: (slot) => [made.pose(slot).x, made.pose(slot).y],
        rims: () =>
            [10, 11, 12, 30].map((slot) => surface.dresses.get(slot).rim),
    };
}

test("cards a tray lets go show their faces until they lie under others", async () => {
    // 11 and 12 wait where they were held up while 10 flies. Let go, 12 is a card under the
    // graveyard's top until it flies. Were each dressed as it rests from the first, 12
    // would be drawn as a bare edge, an empty dark card, where it was held up, and 30 would
    // be one on the graveyard while the three had yet to leave it.
    const { made, at, rims } = await exileThree(false);
    // 10 flies to exile over 400 ms while 11 and 12 wait, and 30 is the graveyard's top:
    // each shows its face.
    made.clock.advance(200);
    assert.deepEqual(
        [at(11), at(12), at(30)],
        [
            [200, -1000],
            [400, -1000],
            [4000, 500],
        ],
    );
    assert.deepEqual(rims(), [false, false, false, false]);
    // 11 flies, and then 12, which lies over it; 10 shows only its edge once 11 lies over
    // it, and 11 once 12 does. 30 is all that is left in the graveyard.
    made.clock.advance(400);
    assert.deepEqual(at(10), [5000, 500]);
    assert.deepEqual(rims(), [false, false, false, false]);
    made.clock.advance(400);
    assert.deepEqual(rims(), [true, false, false, false]);
    made.clock.advance(400);
    assert.deepEqual(
        [at(10), at(11), at(12), at(30)],
        [
            [5020, 500],
            [5010, 500],
            [5000, 500],
            [4000, 500],
        ],
    );
    assert.deepEqual(rims(), [true, true, false, false]);
    assert.equal(made.shown.busy, false);
});

test("a card flying to a pile shows its face until it is there, as others follow it", async () => {
    // The three flights are one beat, each beginning 200 ms after the one before, so 11
    // sets off for exile while 10 is still on its way there. Under 11 on the pile, 10 rests
    // as only its edge from then on, and were it dressed so at once it would finish its
    // flight as an empty dark card.
    const { made, at, rims } = await exileThree(true, { stagger: 200 });
    // 10 is still on its way at 300 ms, and 11 at 500.
    made.clock.advance(300);
    assert.equal(at(10)[1], 125);
    assert.deepEqual(rims(), [false, false, false, false]);
    made.clock.advance(200);
    assert.equal(at(11)[1], 125);
    assert.deepEqual(rims(), [false, false, false, false]);
    // Once 11 is there over it, 10 shows only its edge, and 11 once 12 is.
    made.clock.advance(100);
    assert.deepEqual(rims(), [true, false, false, false]);
    made.clock.advance(200);
    assert.deepEqual(
        [at(10), at(11), at(12)],
        [
            [5020, 500],
            [5010, 500],
            [5000, 500],
        ],
    );
    assert.deepEqual(rims(), [true, true, false, false]);
});

test("a card that flies to lie under another shows its face until it is there", async () => {
    // Exile holds 40, and the permanent 1 is exiled to lie under it (exile lies in the
    // order of slots here). Were 1 dressed as it rests from the first, it would cross the
    // table as a bare edge, an empty dark card.
    const { made, surface, prompts } = prompted(graves);
    const players = {
        you: { life: 20, library: [], graveyard: [] },
        opponent: { life: 20, library: [], graveyard: [] },
    };
    prompts.play(
        table(undefined, {
            objects: [object(1), object(3, "hand"), object(40, "exile")],
            players,
        }),
    );
    made.clock.advance(0);
    const exiled = object(1, "exile");
    prompts.play(
        table(undefined, {
            objects: [exiled, object(3, "hand"), object(40, "exile")],
            players,
        }),
        {
            beats: [
                [
                    {
                        act: {
                            verb: "fly",
                            from: "battlefield",
                            why: "effect",
                            object: exiled,
                        },
                    },
                ],
            ],
        },
    );
    made.clock.advance(0);
    made.clock.advance(200);
    assert.equal(made.pose(1).y, 250);
    assert.equal(surface.dresses.get(1).rim, false);
    made.clock.advance(200);
    assert.deepEqual([made.pose(1).x, made.pose(1).y], [5010, 500]);
    assert.equal(surface.dresses.get(1).rim, true);
});
