// The prompt on the stage, with no browser (`npm test`): what ui/src/stage/prompt.ts draws of
// the session's prompt and what it sends of the pointer. The session here is a test's own:
// each click waits for the reply the test gives it, so every state between is looked at.
import assert from "node:assert/strict";
import { test } from "node:test";
import { Feed } from "../src/stage/feed.ts";
import { Prompts } from "../src/stage/prompt.ts";
import { move, stage, view } from "./stage-table.mjs";

const card = (slot) => ({ kind: "object", slot });
const marked = (mark, ...slots) =>
    slots.map((slot) => ({ on: card(slot), mark }));

/** A prompt of the pick shape as the session sends it, picked on the table. */
const pick = (marks, more = {}) => ({
    revision: 0,
    shape: "pick",
    purpose: "priority",
    title: "",
    marks,
    buttons: [],
    state: {
        shape: "pick",
        tray: [],
        least: 0,
        most: 1,
        picked: [],
        warned: [],
    },
    ...more,
});

/** A view of permanents 1 and 2 and the hand's card 3 that asks `prompt` as decision `seq`. */
const asking = (prompt, seq = 1, more = {}) =>
    view([1, 2], [3], {
        status: {
            kind: "deciding",
            decision: {
                id: { session: 1, seq },
                source: null,
                prompt,
                ...more,
            },
        },
    });

/** A stage with its prompts, and a session that records each click sent and answers it when
 * the test says: `reply(value)` gives the oldest click waiting its reply. */
function prompted(durations = {}) {
    const { stage: made, surface } = stage(durations);
    const sent = [];
    const waiting = [];
    const prompts = new Prompts(made, (decision, revision, click) => {
        sent.push({ decision, revision, click });
        return new Promise((resolve, reject) =>
            waiting.push({ resolve, reject }),
        );
    });
    const reply = async (value) => {
        waiting.shift().resolve(value);
        await new Promise(setImmediate);
    };
    const refuse = async (error) => {
        waiting.shift().reject(error);
        await new Promise(setImmediate);
    };
    /** A click on the node `name`. */
    const click = (name) => {
        made.pointer.press(name, { x: 1, y: 1 });
        made.pointer.release({ x: 1, y: 1 });
    };
    return { made, surface, prompts, sent, reply, refuse, click };
}

const drawn = (surface) => [...surface.marks.keys()].sort();

test("a prompt opens as its view has played out, with the marks the session sent", () => {
    const { made, surface, prompts } = prompted({ mark: { duration: 100 } });
    const asked = pick(marked("eligible", 1, 3));
    prompts.play(asking(asked));
    assert.equal(prompts.asked().prompt, asked);
    // The marks come up as a step of their own, on the stage's time.
    made.clock.advance(50);
    assert.equal(surface.marks.get("1 eligible"), 0.5);
    made.clock.advance(50);
    assert.deepEqual(drawn(surface), ["1 eligible", "3 eligible"]);
    assert.equal(surface.marks.get("3 eligible"), 1);
    assert.equal(made.shown.busy, false);
});

test("a click sends what was clicked on the state shown, and the reply is drawn as it is", async () => {
    const { made, surface, prompts, sent, reply, click } = prompted();
    prompts.play(asking(pick(marked("eligible", 1, 2))));
    made.clock.advance(0);
    click(2);
    assert.deepEqual(sent, [
        {
            decision: { session: 1, seq: 1 },
            revision: 0,
            click: { kind: "on", on: card(2) },
        },
    ]);
    // One click is with the session at a time.
    assert.equal(prompts.asked().sending, true);
    click(1);
    prompts.press({ kind: "decline" });
    assert.equal(sent.length, 1);
    const picked = pick([...marked("eligible", 1), ...marked("selected", 2)], {
        revision: 1,
    });
    await reply({ kind: "open", prompt: picked });
    made.clock.advance(0);
    assert.equal(prompts.asked().prompt, picked);
    assert.equal(prompts.asked().sending, false);
    assert.deepEqual(drawn(surface), ["1 eligible", "2 selected"]);
    // Only what changed was touched: the mark that stayed was never taken away.
    assert.deepEqual(surface.spans, [
        "+ 1 eligible",
        "+ 2 eligible",
        "- 2 eligible",
        "+ 2 selected",
    ]);
    prompts.press({ kind: "confirm" });
    assert.deepEqual(sent[1], {
        decision: { session: 1, seq: 1 },
        revision: 1,
        click: { kind: "button", button: { kind: "confirm" } },
    });
});

test("a click that answers leaves the marks to the next view, which ends them first", async () => {
    const { made, surface, prompts, reply, click } = prompted({
        fly: { duration: 100 },
    });
    const first = asking(pick(marked("eligible", 1, 2)));
    prompts.play(first);
    made.clock.advance(0);
    click(1);
    await reply({ kind: "answered" });
    assert.deepEqual(prompts.asked(), { sending: false, answered: true });
    assert.deepEqual(drawn(surface), ["1 eligible", "2 eligible"]);
    // The view it answered into: no mark is on the table as its script begins.
    prompts.play(view([2, 1], [3]), { beats: [[move(1, 1)]] });
    assert.equal(prompts.asked().answered, false);
    made.clock.advance(50);
    assert.deepEqual(drawn(surface), []);
    assert.equal(made.step.verb, "fly");
    assert.equal(
        made.step.index,
        0,
        "the script's steps are numbered as it has them",
    );
});

test("the reply to a click that a later view overtook is of no prompt shown", async () => {
    const { made, surface, prompts, reply, click } = prompted();
    prompts.play(asking(pick(marked("eligible", 1))));
    made.clock.advance(0);
    click(1);
    // The views of the game playing on arrive before the click's reply does.
    const next = pick(marked("eligible", 2));
    prompts.play(asking(next, 2), { beats: [] });
    made.clock.advance(0);
    assert.equal(prompts.asked().prompt, next);
    await reply({ kind: "answered" });
    made.clock.advance(0);
    assert.equal(prompts.asked().prompt, next);
    assert.equal(prompts.asked().sending, false);
    assert.deepEqual(drawn(surface), ["2 eligible"]);
});

test("a click the prompt had no use for goes on to who listens, and so does one with no prompt", async () => {
    const { made, prompts, sent, reply, click } = prompted();
    const told = [];
    prompts.listen({
        click: (name) => told.push(["click", name]),
        hover: (name) => told.push(["hover", name]),
    });
    made.pointer.over(2);
    const asked = pick(marked("eligible", 1));
    prompts.play(asking(asked));
    made.clock.advance(0);
    click(2);
    assert.deepEqual(told, [["hover", 2]], "the session is asked first");
    await reply({ kind: "open", prompt: asked });
    assert.deepEqual(told.at(-1), ["click", 2]);
    // The game plays on: nothing is asked, and nothing is sent.
    prompts.play(view([1, 2], [3]), { beats: [] });
    made.clock.advance(0);
    click(1);
    assert.equal(sent.length, 1);
    assert.deepEqual(told.at(-1), ["click", 1]);
    // Nor is a click a board keeps for itself, as a phone keeps a touch on the hand.
    prompts.play(asking(asked, 2), { beats: [] });
    made.clock.advance(0);
    prompts.clicks = (name) => name !== 3;
    click(3);
    assert.equal(sent.length, 1);
    assert.deepEqual(told.at(-1), ["click", 3]);
});

test("a prompt answered elsewhere is let be", () => {
    const { made, surface, prompts, sent, click } = prompted();
    // A shape the stage does not draw yet, and one the session does not yet hold.
    const paid = pick(marked("eligible", 1), {
        shape: "pay",
        state: { shape: "pay", cost: "{3}", left: "{3}" },
    });
    for (const [seq, prompt] of [
        paid,
        { ...pick([]), state: null },
    ].entries()) {
        prompts.play(asking(prompt, seq + 1), { beats: [] });
        made.clock.advance(0);
        assert.deepEqual(prompts.asked(), { sending: false, answered: false });
        click(1);
    }
    assert.deepEqual(drawn(surface), []);
    assert.equal(sent.length, 0);
});

test("the choice a click opens over a priority is held, whatever its shape", async () => {
    const { made, surface, prompts, sent, reply, click } = prompted();
    prompts.play(asking(pick(marked("eligible", 1))));
    made.clock.advance(0);
    click(2);
    const ways = {
        ...pick([], { revision: 1 }),
        shape: "mode",
        state: { shape: "mode", about: [2] },
        buttons: [{ id: { kind: "decline" }, label: "Cancel" }],
    };
    await reply({ kind: "open", prompt: ways });
    made.clock.advance(0);
    assert.equal(prompts.asked().prompt, ways);
    assert.deepEqual(drawn(surface), []);
    // The same decision shown again, as a change of stops shows it, keeps its prompt.
    prompts.play(asking(ways, 1), { beats: [] });
    made.clock.advance(0);
    assert.equal(prompts.asked().prompt, ways);
    prompts.press({ kind: "decline" });
    assert.equal(sent[1].revision, 1);
});

test("a card of the hand the prompt marks is carried out of it as a click on it", async () => {
    const { made, prompts, sent, reply } = prompted();
    made.release = { duration: 100, delay: 0, stagger: 0, ease: (p) => p };
    const asked = pick(marked("eligible", 1, 3));
    prompts.play(asking(asked));
    made.clock.advance(0);
    // A permanent is clicked, never carried; nor is a card the prompt does not mark.
    assert.equal(made.pointer.holdable(1), false);
    assert.equal(made.pointer.holdable(2), false);
    assert.equal(made.pointer.holdable(3), true);
    // Let go over the hand, where it rests at y 500, it goes back and nothing is sent.
    made.pointer.press(3, { x: 30, y: 540 });
    made.pointer.move({ x: 130, y: 560 });
    made.pointer.release({ x: 130, y: 560 });
    assert.equal(sent.length, 0);
    made.clock.advance(100);
    assert.equal(made.pose(3).y, 500);
    // Let go over the table, it is a click on the card, which waits where it was let go.
    made.pointer.press(3, { x: 30, y: 540 });
    made.pointer.move({ x: 230, y: 240 });
    made.pointer.release({ x: 230, y: 240 });
    assert.deepEqual(sent[0].click, { kind: "on", on: card(3) });
    made.clock.advance(100);
    assert.deepEqual([made.pose(3).x, made.pose(3).y], [200, 200]);
    // The session had no use for it: the card goes home.
    await reply({ kind: "open", prompt: asked });
    made.clock.advance(100);
    assert.deepEqual([made.pose(3).x, made.pose(3).y], [0, 500]);
});

test("an arrow runs from what asks to a target picked, and the script that casts at it takes it over", async () => {
    const { made, surface, prompts, reply, click } = prompted();
    const spell = { kind: "spell", slot: 3, targets: [] };
    const table = (prompt, seq) => ({
        ...asking(prompt, seq, { source: 3 }),
        objects: asking(prompt).objects.map((o) =>
            o.slot === 3 ? { ...o, zone: "stack" } : o,
        ),
        stack: [spell],
    });
    prompts.play(table(pick(marked("eligible", 1, 2), { purpose: "target" })));
    made.clock.advance(0);
    assert.equal(surface.lines.size, 0);
    click(1);
    await reply({
        kind: "open",
        prompt: pick(
            [...marked("friendly-target", 1), ...marked("eligible", 2)],
            { purpose: "target", revision: 1 },
        ),
    });
    made.clock.advance(0);
    assert.deepEqual([...surface.lines.keys()], ["friendly-target 3 1"]);
    assert.deepEqual(drawn(surface), ["1 friendly-target", "2 eligible"]);
    // Confirmed: the view it answers into draws the same arrow, which is never taken away.
    surface.spans.length = 0;
    const cast = {
        act: {
            verb: "point",
            span: "begin",
            arrow: "friendly-target",
            from: card(3),
            to: card(1),
        },
    };
    prompts.play(table(pick([]), 2), { beats: [[cast]] });
    made.clock.advance(0);
    assert.deepEqual(surface.spans, ["- 1 friendly-target", "- 2 eligible"]);
    assert.deepEqual([...surface.lines.keys()], ["friendly-target 3 1"]);
});

test("the marks coming up hold up neither the screen nor the next view", () => {
    // A mark comes up over a moment and is cut as it ends, as the verbs' data has it.
    const { made, surface, prompts } = prompted((act) =>
        act.verb === "fly" || act.span === "begin" ? { duration: 100 } : {},
    );
    const feed = new Feed(prompts);
    const update = (view, script) => ({
        session: 1,
        view: { ...view, script, events: [] },
    });
    feed.arrive(update(asking(pick(marked("eligible", 1, 2)))), false);
    made.clock.advance(10);
    // The stage plays the marks; the screen has the view, and waits for nothing.
    assert.equal(made.shown.busy, true);
    assert.equal(feed.fed().behind, false);
    assert.equal(feed.fed().shown.view.status.kind, "deciding");
    // The next view's script begins at once, under no mark.
    feed.arrive(update(view([2, 1], [3]), { beats: [[move(1, 1)]] }), false);
    assert.equal(feed.fed().behind, true);
    made.clock.advance(50);
    assert.equal(made.step.verb, "fly");
    assert.equal(made.pose(1).x, 200);
    assert.deepEqual(drawn(surface), []);
    made.clock.advance(50);
    assert.equal(feed.fed().behind, false);
});

test("a click that fails leaves the prompt open, and says why", async () => {
    const { made, prompts, refuse, click } = prompted();
    const asked = pick(marked("eligible", 1));
    prompts.play(asking(asked));
    made.clock.advance(0);
    click(1);
    const error = new Error("refused");
    await refuse(error);
    assert.deepEqual(prompts.asked().error, error);
    assert.equal(prompts.asked().prompt, asked);
    assert.equal(prompts.asked().sending, false);
});
