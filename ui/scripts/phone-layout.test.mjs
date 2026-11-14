// A crowded row still shows creature names, phones outside the standard widths use the
// same layout column, scaled or centered, and what a prompt asks is written clear of the
// cards on the table.
import assert from "node:assert/strict";
import { test } from "node:test";
import { askedTop } from "../src/mobile/asked.ts";
import { creatureHeight, phoneFrame, RATIO } from "../src/mobile/sizing.ts";

test("creature cards stay at least 56px high when width or height is crowded", () => {
    assert.equal(creatureHeight(100, 28), 56);
    assert.equal(creatureHeight(42, 90), 56);
    assert.equal(creatureHeight(96, 80), 80);
    assert.equal(creatureHeight(150, 150), 128);
});

test("the phone centers at 430px and scales the 375px column below that width", () => {
    assert.deepEqual(phoneFrame(480, 850), {
        width: 430,
        height: 850,
        scale: 1,
    });
    assert.deepEqual(phoneFrame(393, 759), {
        width: 393,
        height: 759,
        scale: 1,
    });
    const narrow = phoneFrame(360, 647);
    assert.equal(narrow.width, 375);
    assert.equal(narrow.scale, 360 / 375);
    assert.equal(narrow.height * narrow.scale, 647);
});

/** The bands of a 393 by 759 phone (mobile/layout.ts `phoneLayout`) with no creatures out and
 * `middle` the middle band's height: 44 idle, 110 with a stack. */
function bands(middle) {
    const heights = [
        ["opponentStrip", 48],
        ["opponentLands", 39],
        ["opponentOther", 54],
        ["opponentCreatures", 134],
        ["middle", middle],
        ["yourCreatures", 134],
        ["yourOther", 54],
        ["yourLands", 39],
        ["yourStrip", 60],
        ["hand", 87],
    ];
    const out = {};
    let top = 0;
    for (const [name, height] of heights) {
        out[name] = { top, height };
        top += height;
    }
    return out;
}

/** A row of `count` cards, `height` high, filling `band` as phoneLayout's rows do. */
const row = (band, count, height = band.height - 8, tapped = false) =>
    Array.from({ length: count }, (_, i) => ({
        slot: i,
        zone: "battlefield",
        x: 8 + i * 40,
        y: band.top + (band.height - (tapped ? height * RATIO : height)) / 2,
        width: height * RATIO,
        tapped,
        z: i,
        rotate: 0,
    }));

/** The top and bottom of each card in `layout`. */
const spans = ({ cards, stack }) => [
    ...cards.map((c) => [c.y, c.y + (c.tapped ? c.width : c.width / RATIO)]),
    ...stack.map((s) => [s.y, s.y + s.width / RATIO]),
];

const clearOf = (layout, top, height) =>
    spans(layout).every(([a, b]) => top + height <= a || top >= b);

test("what a prompt asks is written on the middle's gold line when the stack is empty", () => {
    const b = bands(44);
    const layout = {
        bands: b,
        cards: [...row(b.yourLands, 7), ...row(b.yourOther, 1)],
        stack: [],
    };
    const top = askedTop(layout, 24);
    assert.equal(top + 12, b.middle.top + 22);
});

test("an untap of lands under an ability on the stack writes its words clear of every land", () => {
    // Teferi, Hero of Dominaria's delayed trigger on the stack, the planeswalker in its row and
    // seven lands, some tapped: the words once stood over the land row.
    const b = bands(110);
    const layout = {
        bands: b,
        cards: [
            ...row(b.opponentLands, 2),
            ...row(b.yourOther, 1),
            ...row(b.yourLands, 4),
            ...row(b.yourLands, 3, b.yourLands.height - 8, true),
        ],
        stack: [
            {
                index: 0,
                x: 162,
                y: b.middle.top + (110 - 68 / RATIO) / 2,
                width: 68,
                z: 20000,
            },
        ],
    };
    for (const height of [24, 48]) {
        const top = askedTop(layout, height);
        assert.ok(clearOf(layout, top, height), `${height}px pill at ${top}`);
        assert.ok(top + height <= b.yourStrip.top);
        // Nearest the middle: just under the stack's card, in the empty creature band.
        assert.ok(top < b.yourCreatures.top + 20, `${height}px pill at ${top}`);
    }
});

test("among crowded rows what a prompt asks finds the nearest gap clear of every card", () => {
    const b = bands(110);
    const creatures = { top: b.yourCreatures.top + 10, height: 100 };
    const layout = {
        bands: b,
        cards: [
            ...row(b.opponentLands, 6),
            ...row(b.opponentOther, 3),
            ...row(creatures, 5, 100),
            ...row(b.yourOther, 3),
            ...row(b.yourLands, 8),
        ],
        stack: [
            {
                index: 0,
                x: 162,
                y: b.middle.top + (110 - 68 / RATIO) / 2,
                width: 68,
                z: 20000,
            },
        ],
    };
    const top = askedTop(layout, 24);
    assert.ok(clearOf(layout, top, 24), `pill at ${top}`);
});
