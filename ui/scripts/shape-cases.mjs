// What the shape suite (shapes.mjs) needs for the shapes that hold cards up off the table or
// list what the table has no card of: what the session's state says is held up, the gesture
// that carries a card or a trigger to its place, and the scry flight, a case of the split
// shape played tick by tick.
import assert from "node:assert/strict";

/** Whether the stage holds `prompt` (src/stage/prompt.ts `onTable`): where it does not, the
 * screen is held to show none of it. */
export function onTable(prompt) {
    return ["pick", "split", "order", "pay", "declare"].includes(
        prompt.state?.shape,
    );
}

/** What `prompt` holds up, as the session's state says it: a split's piles, each under its
 * name with its cards in their order; a pick's tray, in no order; an order's items, first
 * to resolve first. No card of them shows its back, and none is drawn twice. */
export function heldUp(prompt) {
    const state = (prompt && onTable(prompt) && prompt.state) || {};
    return {
        piles:
            state.shape === "split"
                ? state.piles.map(({ label, cards }) => ({ label, cards }))
                : [],
        loose:
            state.shape === "pick" ? [...state.tray].sort((a, b) => a - b) : [],
        order: state.shape === "order" ? state.items.map(({ id }) => id) : [],
        backs: 0,
        twice: 0,
    };
}

/** The cards `prompt` holds up. */
export const cardsUp = (prompt) => {
    const { piles, loose } = heldUp(prompt);
    return [...piles.flatMap((pile) => pile.cards), ...loose];
};

/** In the page: where to let go what is carried to before the thing at `at` of the things
 * `selectors` find in their order, along the `axis` they lie along (`backward`, if their
 * order runs against it), within the box `within` finds if any: short of the first's middle,
 * past the last's, or between two; the middle of the box where it holds nothing. */
function placeFor({ selectors, at, axis, within, backward }) {
    const boxes = selectors.map((selector) =>
        document.querySelector(selector).getBoundingClientRect(),
    );
    const [start, size, cross, girth] =
        axis === "x"
            ? ["left", "width", "top", "height"]
            : ["top", "height", "left", "width"];
    const middles = boxes.map((box) => box[start] + box[size] / 2);
    const way = backward ? -1 : 1;
    const frame = within
        ? document.querySelector(within).getBoundingClientRect()
        : undefined;
    const along =
        middles.length === 0
            ? frame[start] + frame[size] / 2
            : at === 0
              ? middles[0] - 6 * way
              : at >= middles.length
                ? middles.at(-1) + 6 * way
                : (middles[at - 1] + middles[at]) / 2;
    const across = boxes[0]
        ? boxes[0][cross] + boxes[0][girth] / 2
        : frame[cross] + frame[girth] * 0.6;
    return axis === "x" ? { x: along, y: across } : { x: across, y: along };
}

const node = (name) => `[data-stage="${name}"]`;

/** The depth the layouts hold a tray's cards up from (src/stage/desktop.ts TRAY_DEPTH,
 * phone.ts DEPTH.tray): over everything at rest. */
const HELD_UP = 850;

/** Carries what the drop `click` is of to where it says, on the prompt as `before` has it: a
 * card of a split to its pile, before the card that lies at `at` there, or a waiting trigger
 * to its place in the order. `from` is a point of it the pointer reaches. */
export async function carry(page, click, before, from) {
    const { state } = before.prompt;
    const to = await page.evaluate(
        placeFor,
        state.shape === "split"
            ? {
                  selectors: state.piles[click.pile].cards.map(node),
                  at: click.at,
                  axis: "x",
                  within: `.stage-tray-pile[data-pile="${click.pile}"]`,
              }
            : {
                  selectors: state.items.map(
                      ({ id }) => `.stage-order-item[data-item="${id}"]`,
                  ),
                  at: click.at,
                  axis: "x",
                  // The first to resolve is at the right.
                  backward: true,
              },
    );
    await page.mouse.move(from.x, from.y);
    await page.mouse.down();
    await page.mouse.move((from.x + to.x) / 2, (from.y + to.y) / 2 - 12, {
        steps: 6,
    });
    await page.mouse.move(to.x, to.y, { steps: 6 });
    await page.mouse.up();
}

/** In the page: plays out what the answer to a scry led to, a tick at a time, and returns
 * what each tick showed of the `cards` it looked at: how many nodes each has, whether it
 * shows its face, and where the model has it; and whether the pill that says what the scry
 * did was up. */
async function flown({ tick, cards }) {
    const stage = window.berylStage;
    await new Promise((resolve) => setTimeout(resolve));
    const sample = () => ({
        at: stage.clock.now,
        pill: stage.shown.notices.some(
            ({ act }) => act.verb === "announce" && act.notice.kind === "scry",
        ),
        cards: cards.map((slot) => {
            const nodes = document.querySelectorAll(`[data-stage="${slot}"]`);
            return {
                slot,
                nodes: nodes.length,
                faced: nodes[0]?.dataset.card !== undefined,
                zone: nodes[0]?.dataset.zone,
                pose: stage.pose(slot),
            };
        }),
    });
    const seen = [sample()];
    // The pill passes over the table and may outlast the script (timing.ts, `passes`):
    // play on until it has gone too.
    const playing = () => stage.shown.busy || stage.shown.notices.length > 0;
    for (let i = 0; playing() && i < 2000; i++) {
        stage.clock.advance(tick);
        // The faces are React's, drawn a moment after the model has them.
        await new Promise((resolve) => setTimeout(resolve));
        seen.push(sample());
    }
    return seen;
}

/** The scry flight, one of the five themes the bug reports repeated: the cards a scry looks
 * at are the library's own, held up face up and sorted by the prompt, and they go back to
 * the library as the answer plays, each one node from the look to the end. `before` is the
 * prompt as its Done was pressed, `page` has just sent that press and `ends` is the view the
 * recording played on to. Held to what each report asked:
 * - by-ykr, by-8qq: the cards come off the library itself and go back to it (no deck drawn
 *   apart from the table's): each is in the library's zone throughout, and ends on its pile;
 * - by-1mrh: none is put face down on its way: one that has not come to rest shows its face;
 * - by-2wn7, by-kyig: a card put on top ends over one put on the bottom, which ends under
 *   the pile, and neither is anywhere else at the end;
 * - by-3t7z: the pill that says what the scry did is up while its step plays and gone after.
 * (The opponent's side of it, by-pv6p, by-yfb2 and by-nva, is the `lift` verb's check and
 * this shape's `theirs`.) */
export async function scryFlight(page, before, ends, tick) {
    const [bottom, top] = before.prompt.state.piles.map((pile) => pile.cards);
    const cards = [...bottom, ...top];
    const seen = await page.evaluate(flown, { tick, cards });
    const last = seen.at(-1);
    const posed = (sample, i) => JSON.stringify(sample?.cards[i].pose);
    // A card is on its way from the tick the tray lets it go to the tick it first lies
    // still outside it; what moves it after that (the library put in its new order) moves
    // it as the pile's own card.
    const home = new Set();
    for (const [n, sample] of seen.entries()) {
        for (const [i, card] of sample.cards.entries()) {
            const what = `scry card ${card.slot} at ${sample.at} ms`;
            assert.equal(card.nodes, 1, `${what}: one node`);
            assert.equal(card.zone, "library", `${what}: of the library`);
            const still = n > 0 && posed(seen[n - 1], i) === posed(sample, i);
            if (card.pose.depth < HELD_UP && still) home.add(card.slot);
            // The tick it lands on has it dressed as it rests.
            const lands =
                n + 1 < seen.length &&
                posed(seen[n + 1], i) === posed(sample, i);
            if (!home.has(card.slot) && !lands) {
                assert.ok(card.faced, `${what}: face up on its way`);
            }
        }
    }
    assert.deepEqual([...home].sort(), [...cards].sort(), "back on the pile");
    assert.ok(
        seen.some((sample) =>
            sample.cards.some((card) => card.pose.depth < HELD_UP),
        ),
        "the cards leave the tray",
    );
    assert.ok(
        seen.some((sample) => sample.pill),
        "the pill says what the scry did",
    );
    assert.equal(last.pill, false, "the pill is gone once it has played");
    const library = ends.view.players.you.library;
    const depth = (slot) =>
        last.cards.find((card) => card.slot === slot).pose.depth;
    for (const slot of top) {
        assert.ok(
            library.indexOf(slot) >= library.length - top.length,
            `${slot} is put on top`,
        );
        for (const under of bottom) {
            assert.ok(
                depth(slot) > depth(under),
                `${slot}, on top, lies over ${under}, on the bottom`,
            );
        }
    }
    for (const slot of bottom) {
        assert.ok(library.indexOf(slot) < bottom.length, `${slot} is beneath`);
    }
}
