// The cards a prompt holds up, with no browser (`npm test`): what ui/src/stage/prompt.ts
// lifts into a tray from the session's state and lets go as the answer plays, what it sends
// of a card carried between piles, and where ui/src/stage/tray.ts lays a tray out. The
// layout is the tests' own rows (stage-table.mjs), so every pose is a number worked out by
// hand.
import assert from "node:assert/strict";
import { test } from "node:test";
import { Prompts, trayOf } from "../src/stage/prompt.ts";
import { laidOut, landing } from "../src/stage/tray.ts";
import { object, stage, view } from "./stage-table.mjs";

const card = (slot) => ({ kind: "object", slot });

/** A split of the library's cards as the session sends it: nothing in the first pile, which
 * must come to hold `exactly`, and `cards` in the second. */
const split = (first, second, more = {}) => ({
    revision: 0,
    purpose: "arrange",
    title: "Put one of them into your hand.",
    marks: [...first, ...second].map((slot) => ({
        on: card(slot),
        mark: "eligible",
    })),
    buttons: [],
    body: {
        shape: "split",
        piles: [
            { place: "hand", label: "Hand", cards: first, exactly: 1 },
            { place: "bottom", label: "Bottom", cards: second, exactly: null },
        ],
    },
    ...more,
});

/** A view of the permanent 1, the hand's card 3 and a library of 10 under 11, whose cards
 * the viewer knows; it asks `prompt`, if given one. */
const table = (prompt, more = {}) => {
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

/** A stage whose lifts take 300 ms, or 200 to sort, and whose flights take 400; with its
 * prompts, and a session that answers each click when the test says. */
function prompted() {
    const { stage: made, surface } = stage((act) => {
        if (act.verb === "lift") return { duration: act.sorted ? 200 : 300 };
        return { duration: act.verb === "fly" ? 400 : 0 };
    });
    const sent = [];
    const waiting = [];
    const prompts = new Prompts(made, (decision, revision, click) => {
        sent.push({ decision, revision, click });
        return new Promise((resolve) => waiting.push(resolve));
    });
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

test("a prompt lifts the cards it holds up as it opens, and each reply sorts them", async () => {
    const { made, prompts, sent, reply, at } = prompted();
    const asked = split([], [11, 10]);
    assert.deepEqual(trayOf(asked), {
        piles: [{ cards: [], room: 1 }, { cards: [11, 10] }],
    });
    prompts.play(table(asked));
    made.clock.advance(0);
    // Both lie on the library, at x 3000 and y 500, and rise to the second pile's row: half
    // way after 150 of 300 ms.
    assert.deepEqual(at(11), [3000, 500]);
    made.clock.advance(150);
    assert.deepEqual(at(11), [2000, -250]);
    assert.deepEqual(at(10), [2100, -250]);
    made.clock.advance(150);
    assert.deepEqual(at(11), [1000, -1000]);
    assert.deepEqual(at(10), [1200, -1000]);
    assert.equal(made.pose(11).width, 150);
    assert.equal(made.shown.busy, false);
    // A click on one is a click on that card, and the reply's piles are where they go, in
    // the 200 ms of a sort: 10 closes up as 11 crosses to the first pile.
    made.pointer.press(11, { x: 1010, y: -990 });
    made.pointer.release({ x: 1010, y: -990 });
    assert.deepEqual(sent[0].click, { kind: "on", on: card(11) });
    await reply({ kind: "open", prompt: split([11], [10], { revision: 1 }) });
    made.clock.advance(100);
    assert.deepEqual(at(11), [500, -1000]);
    assert.deepEqual(at(10), [1100, -1000]);
    made.clock.advance(100);
    assert.deepEqual(at(11), [0, -1000]);
    assert.deepEqual(at(10), [1000, -1000]);
});

test("the cards go back as the first steps of what the answer did begin", async () => {
    const { made, surface, prompts, reply, at } = prompted();
    prompts.play(table(split([11], [10])));
    made.clock.advance(300);
    surface.dressed.length = 0;
    prompts.press({ kind: "confirm" });
    await reply({ kind: "answered" });
    // Still up until the view the answer led to plays: 11 goes to the hand, beside 3, and
    // the library is left with 10.
    assert.deepEqual(at(10), [1000, -1000]);
    const kept = { ...object(11, "hand") };
    const after = table(undefined, {
        objects: [object(1), object(3, "hand"), object(10, "library"), kept],
        players: {
            you: { life: 20, library: [10], graveyard: [] },
            opponent: { life: 20, library: [], graveyard: [] },
        },
    });
    prompts.play(after, {
        beats: [
            [
                {
                    act: {
                        verb: "fly",
                        from: "library",
                        why: "effect",
                        object: kept,
                    },
                },
            ],
        ],
    });
    made.clock.advance(0);
    // The card kept flies to the hand from where it was held up, in its flight's 400 ms; the
    // other goes home to the library in the lift's 300, showing its face until it is there.
    made.clock.advance(150);
    assert.deepEqual(at(10), [2000, -250]);
    assert.deepEqual(at(11), [150, -437.5]);
    assert.ok(!surface.dressed.includes("10 back"));
    made.clock.advance(250);
    assert.deepEqual(at(10), [3000, 500]);
    assert.deepEqual(at(11), [400, 500]);
    assert.equal(made.shown.busy, false);
    assert.equal(prompts.asked().prompt, undefined);
});

test("a card carried between piles is dropped where the layout says it landed", async () => {
    const { made, prompts, sent, reply, at } = prompted();
    made.release = { duration: 100, delay: 0, stagger: 0, ease: (p) => p };
    const asked = split([], [11, 10]);
    prompts.play(table(asked));
    made.clock.advance(300);
    const told = [];
    prompts.lands = (tray, point) => {
        told.push({ tray, point });
        return point.x < 900 ? { pile: 0, at: 0 } : undefined;
    };
    // A card of a split may be carried, wherever it came from; one of the table may not.
    assert.equal(made.pointer.holdable(10), true);
    assert.equal(made.pointer.holdable(1), false);
    // Let go outside every pile, it goes back and nothing is sent.
    made.pointer.press(10, { x: 1210, y: -990 });
    made.pointer.move({ x: 1500, y: -700 });
    made.pointer.release({ x: 1500, y: -700 });
    assert.equal(sent.length, 0);
    made.clock.advance(100);
    assert.deepEqual(at(10), [1200, -1000]);
    // Let go over the first pile, it is dropped there, and waits where it was let go.
    made.pointer.press(10, { x: 1210, y: -990 });
    made.pointer.move({ x: 310, y: -890 });
    made.pointer.release({ x: 310, y: -890 });
    assert.deepEqual(told.at(-1), {
        tray: trayOf(asked),
        point: { x: 310, y: -890 },
    });
    assert.deepEqual(sent[0].click, {
        kind: "drop",
        on: card(10),
        pile: 0,
        at: 0,
    });
    assert.deepEqual(at(10), [300, -900]);
    // The reply lays it in its pile, from where it was let go.
    await reply({ kind: "open", prompt: split([10], [11], { revision: 1 }) });
    made.clock.advance(200);
    assert.deepEqual(at(10), [0, -1000]);
    assert.deepEqual(at(11), [1000, -1000]);
});

test("a pick's tray is clicked where it is held up, whatever a board says of its cards", async () => {
    const { made, prompts, sent } = prompted();
    const asked = {
        ...split([], []),
        purpose: "search",
        marks: [{ on: card(10), mark: "eligible" }],
        body: {
            shape: "pick",
            tray: [10],
            least: 0,
            most: 1,
            picked: [],
            warned: [],
        },
    };
    assert.deepEqual(trayOf(asked), { piles: [{ cards: [10] }] });
    // A phone leaves a touch on some cards to be read.
    prompts.clicks = () => false;
    prompts.play(table(asked));
    made.clock.advance(300);
    assert.equal(made.pose(10).y, -1000);
    // It has no pile to be carried to.
    assert.equal(made.pointer.holdable(10), false);
    made.pointer.press(10, { x: 10, y: -990 });
    made.pointer.release({ x: 10, y: -990 });
    assert.deepEqual(sent[0].click, { kind: "on", on: card(10) });
    // A pick on the table holds nothing up.
    assert.equal(
        trayOf({ ...asked, body: { ...asked.body, tray: [] } }),
        undefined,
    );
});

/** A space 1000 wide and 500 tall at (100, 50), for cards 100 to 200 wide (140 to 280 tall,
 * as a card is 5 by 7), 10 apart, under headings 20 tall. */
const space = {
    x: 100,
    y: 50,
    width: 1000,
    height: 500,
    card: { most: 200, least: 100, aspect: 5 / 7 },
    gap: 10,
    heading: 20,
};
const placed = (laid, slot) => {
    const { x, y, width } = laid.cards.get(slot);
    return [Math.round(x), Math.round(y), width];
};

test("one pile lies in rows about the middle of its space", () => {
    // Three at the widest: 620 across, from 290, and 280 tall, from 160.
    const three = laidOut({ piles: [{ cards: [1, 2, 3] }] }, space);
    assert.deepEqual(three.boxes, []);
    assert.deepEqual(placed(three, 1), [290, 160, 200]);
    assert.deepEqual(placed(three, 3), [710, 160, 200]);
    // Seven need two rows, which the space holds from 168 wide: 235.2 tall each, and 480.4
    // with the 10 between, from 59.8. Five fit across, 178 apart: 880 wide, from 160; the
    // other two are a row of 346, from 427.
    const seven = laidOut({ piles: [{ cards: [1, 2, 3, 4, 5, 6, 7] }] }, space);
    assert.deepEqual(placed(seven, 1), [160, 60, 168]);
    assert.deepEqual(placed(seven, 5), [872, 60, 168]);
    assert.deepEqual(placed(seven, 6), [427, 305, 168]);
    assert.deepEqual(placed(seven, 7), [605, 305, 168]);
});

test("two piles lie in boxes side by side, each as long as it can come to be", () => {
    const tray = { piles: [{ cards: [], room: 1 }, { cards: [11, 10] }] };
    const laid = laidOut(tray, space);
    // The first has room for one card and the second for both: at 200 wide, boxes of 220 and
    // 430 and the 10 between, 660 in all, from 270; 320 tall, from 140.
    assert.deepEqual(laid.boxes, [
        { x: 270, y: 140, width: 220, height: 320 },
        { x: 500, y: 140, width: 430, height: 320 },
    ]);
    assert.deepEqual(placed(laid, 11), [510, 170, 200]);
    assert.deepEqual(placed(laid, 10), [720, 170, 200]);
    // Let go in a box, a card lands before the first card whose middle is past it.
    assert.deepEqual(landing(tray, space, { x: 300, y: 300 }), {
        pile: 0,
        at: 0,
    });
    assert.deepEqual(landing(tray, space, { x: 600, y: 300 }), {
        pile: 1,
        at: 0,
    });
    assert.deepEqual(landing(tray, space, { x: 700, y: 300 }), {
        pile: 1,
        at: 1,
    });
    assert.deepEqual(landing(tray, space, { x: 900, y: 300 }), {
        pile: 1,
        at: 2,
    });
    // Outside every box, nowhere; and a tray of one pile has no place to give.
    assert.equal(landing(tray, space, { x: 495, y: 300 }), undefined);
    assert.equal(landing(tray, space, { x: 600, y: 100 }), undefined);
    assert.equal(
        landing({ piles: [{ cards: [10] }] }, space, { x: 600, y: 300 }),
        undefined,
    );
});

test("where the second pile has a space of its own, each box is as wide as its space", () => {
    const second = { x: 100, y: 700, width: 1000, height: 300 };
    const tray = { piles: [{ cards: [11], room: 2 }, { cards: [10, 12] }] };
    const laid = laidOut(tray, { ...space, second });
    // The cards are as wide as the lower of the two spaces leaves them: 300 less the heading
    // and the edges is 260 tall, 185 and a bit wide.
    const width = laid.cards.get(11).width;
    assert.equal(Math.round(width), 186);
    assert.deepEqual(
        laid.boxes.map((box) => [box.x, Math.round(box.y), box.width]),
        [
            [100, 150, 1000],
            [100, 700, 1000],
        ],
    );
    assert.deepEqual(placed(laid, 11).slice(0, 2), [110, 180]);
    assert.deepEqual(placed(laid, 12).slice(0, 2), [306, 730]);
});
