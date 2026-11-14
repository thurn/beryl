// A stage with its prompts, for the tests of what a prompt holds up and lets go, with no
// browser (stage-tray.test.mjs, stage-piles.test.mjs): the session's prompts and views as
// the tests write them, on the tests' rows (stage-table.mjs).
import { Prompts } from "../src/stage/prompt.ts";
import { object, row, stage, view } from "./stage-table.mjs";

export const card = (slot) => ({ kind: "object", slot });

/** A split of the library's cards as the session sends it: nothing in the first pile, which
 * must come to hold `exactly`, and `cards` in the second. */
export const split = (first, second, more = {}) => ({
    revision: 0,
    purpose: "scry",
    title: "Scry",
    marks: [...first, ...second].map((slot) => ({
        on: card(slot),
        mark: "eligible",
    })),
    buttons: [],
    body: {
        shape: "split",
        piles: [
            { place: "bottom", label: "Bottom", cards: first, exactly: 1 },
            { place: "top", label: "Top", cards: second, exactly: null },
        ],
    },
    ...more,
});

/** A view of the permanent 1, the hand's card 3 and a library of 10 under 11, whose cards
 * the viewer knows; it asks `prompt`, if given one. */
export const table = (prompt, more = {}) => {
    const base = view([1], [3]);
    return {
        ...base,
        objects: [
            ...base.objects,
            object(10, "library"),
            object(11, "library"),
        ],
        players: {
            ...base.players,
            you: { ...base.players.you, library: [10, 11] },
        },
        status: prompt
            ? {
                  kind: "deciding",
                  decision: {
                      id: { session: 1, seq: 1 },
                      source: null,
                      prompt,
                  },
              }
            : { kind: "playing" },
        ...more,
    };
};

/** A stage whose lifts take 300 ms, or 200 to sort, and whose flights take 400, or as
 * `flight` says; with its prompts, and a session that answers each click when the test
 * says. Its `layout` is the tests' rows unless given. */
export function prompted(layout = row, flight = {}) {
    const timed = (act) => {
        if (act.verb === "lift") return { duration: act.sorted ? 200 : 300 };
        return act.verb === "fly"
            ? { duration: 400, ...flight }
            : { duration: 0 };
    };
    const { stage: made, surface } = stage(timed, undefined, layout);
    const sent = [];
    const waiting = [];
    const prompts = new Prompts(made, (decision, revision, click) => {
        sent.push({ decision, revision, click });
        return new Promise((resolve) => waiting.push(resolve));
    });
    made.pointer.listen(prompts.pointerClient);
    const reply = async (value) => {
        waiting.shift()(value);
        await new Promise(setImmediate);
    };
    const at = (slot) => {
        const { x, y } = made.pose(slot);
        return [x, y];
    };
    return { made, surface, prompts, sent, reply, at };
}
