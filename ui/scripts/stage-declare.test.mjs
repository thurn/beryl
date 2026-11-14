// A declaration of attackers on the stage, with no browser (`npm test`): each creature the
// player picks to attack wears an attacker's chevron at once, as Arena's does the moment it
// is clicked (`0VsYGs88NXA 33:56.9`), not only once the view that has it attacking comes.
// The chevron is light the stage paints (src/stage/light.ts, a `wedge` glint), so these read
// the light the surface was last given.
import assert from "node:assert/strict";
import { test } from "node:test";
import { Prompts } from "../src/stage/prompt.ts";
import { object, stage, view } from "./stage-table.mjs";

/** A declaration as the session sends it: of attackers, or with `arrow` "block" of blockers. */
const declare = (pairs, aiming, revision = 0, arrow = "attack") => ({
    revision,
    purpose: arrow,
    title: arrow === "attack" ? "Choose attackers." : "Choose blockers.",
    marks: [],
    buttons: [],
    body: { shape: "declare", arrow, pairs, lined: false, aiming },
});

const opponent = { kind: "player", player: "opponent" };

/** A view of creatures 1 and 2 that asks `prompt` as decision `seq`, with `objects` in place
 * of the view's own records where given. */
const asking = (prompt, seq = 1, objects = undefined) => {
    const shown = view([1, 2], [], {
        status: {
            kind: "deciding",
            decision: { id: { session: 1, seq }, source: null, prompt },
        },
    });
    return objects ? { ...shown, objects } : shown;
};

/** A stage with its prompts, whose session answers each click when the test says. */
function prompted() {
    const { stage: made, surface } = stage();
    const waiting = [];
    const prompts = new Prompts(
        made,
        () => new Promise((resolve) => waiting.push(resolve)),
    );
    made.pointer.listen(prompts.pointerClient);
    const click = (name) => {
        made.pointer.press(name, { x: 1, y: 1 });
        made.pointer.release({ x: 1, y: 1 });
    };
    /** Answers the oldest click with the prompt `prompt` opens, and draws a tick. */
    const reply = async (value) => {
        waiting.shift()(value);
        await new Promise(setImmediate);
        made.clock.advance(16);
    };
    return { made, surface, prompts, click, reply };
}

/** The chevrons the stage paints now: the attackers' wedges of light. */
const chevrons = (surface) =>
    [
        ...(surface.lit?.under.glints ?? []),
        ...(surface.lit?.over.glints ?? []),
    ].filter((glint) => glint.kind === "wedge").length;

test("a creature picked to attack wears its chevron at once, and loses it when taken back", async () => {
    const { made, surface, prompts, click, reply } = prompted();
    prompts.play(asking(declare([], [])));
    made.clock.advance(16);
    assert.equal(chevrons(surface), 0, "none picked");
    // Picked, and still to be aimed at a player or a planeswalker.
    click(1);
    await reply({ kind: "open", prompt: declare([], [1], 1) });
    assert.equal(chevrons(surface), 1, "picked, not yet aimed");
    click(2);
    await reply({
        kind: "open",
        prompt: declare([{ from: 1, to: opponent }], [2], 2),
    });
    assert.equal(chevrons(surface), 2, "one aimed and one picked");
    // Taken back.
    click(2);
    await reply({
        kind: "open",
        prompt: declare([{ from: 1, to: opponent }], [], 3),
    });
    assert.equal(chevrons(surface), 1, "one taken back");
});

test("an attacker keeps its chevron as the view that has it attacking comes", async () => {
    const { made, surface, prompts, click, reply } = prompted();
    prompts.play(asking(declare([], [])));
    made.clock.advance(16);
    click(1);
    await reply({
        kind: "open",
        prompt: declare([{ from: 1, to: opponent }], [], 1),
    });
    prompts.press({ kind: "confirm" });
    await reply({ kind: "answered" });
    const attacking = object(1, "battlefield", {
        tapped: true,
        combat: { kind: "attacking", planeswalker: null, blocked: false },
    });
    prompts.play(
        asking(declare([], [], 0, "block"), 2, [attacking, object(2)]),
    );
    made.clock.advance(16);
    assert.equal(chevrons(surface), 1);
});

test("a creature picked to block wears no attacker's chevron", async () => {
    const { made, surface, prompts, click, reply } = prompted();
    prompts.play(asking(declare([], [], 0, "block")));
    made.clock.advance(16);
    click(1);
    await reply({ kind: "open", prompt: declare([], [1], 1, "block") });
    assert.equal(chevrons(surface), 0);
});
