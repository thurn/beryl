import assert from "node:assert/strict";
import { test } from "node:test";
import { commands } from "../src/table/commands.ts";
import { fill } from "../src/table/fill.ts";
import { allowed, menuRows } from "../src/table/menuRows.ts";

const view = {
    turn: { active: "you" },
    table: {
        seats: 2,
        acting: [],
        orders: { you: { hand: [1] }, opponent: { hand: [2, 3] } },
        topRevealed: true,
    },
    players: { you: { library: [7] }, opponent: { library: [8] } },
    stack: [],
};
const card = (kind, zone, owner = "you", name = "Example") => ({
    kind,
    name: 1,
    object: {
        name,
        slot: 1,
        zone,
        owner,
        controller: owner,
        types: ["creature"],
    },
    action: { zone, reference: { view: "one", handle: 1 } },
});
const rows = (target, state = view, printed) =>
    menuRows(target.kind, {
        target,
        view: state,
        printed,
        cards: "name" in target ? [target] : [],
    });
const moves = [
    "Graveyard",
    "Exile",
    "Hand",
    "Top of library",
    "Bottom of library",
];
const labels = (menu) =>
    menu.groups.flatMap(({ rows }) => rows.map(({ label }) => label));

for (const [name, target, first] of [
    ["own permanent", card("permanent", "battlefield"), ["Tap", ...moves]],
    [
        "opposing permanent",
        card("their_permanent", "battlefield", "opponent"),
        [...moves, "Take control"],
    ],
    [
        "hand card",
        card("hand_card", "hand"),
        ["Play", "Play face down", "Reveal", "Graveyard"],
    ],
    ["stack entry", card("stack_entry", "stack"), ["Resolve", "Graveyard"]],
    [
        "graveyard card",
        card("pile_card", "graveyard"),
        ["Play", "Battlefield", "Hand"],
    ],
    [
        "library",
        { kind: "library", side: "you" },
        ["Draw", "Draw cards…", "Look at top cards…", "Search", "Shuffle"],
    ],
    [
        "life total",
        { kind: "life", side: "you" },
        ["−1", "+1", "−5", "+5", "Set life…"],
    ],
]) {
    test(`menu rows for ${name}`, () => {
        const menu = rows(target);
        assert.deepEqual(labels(menu).slice(0, first.length), first);
        assert.equal(labels(menu).at(-1), "Every action…");
        if (name === "own permanent")
            assert.equal(menu.groups[0].rows[0].click, true);
        if (name === "opposing permanent")
            assert.equal(menu.groups[0].heading, "Move to");
        if (name === "library") {
            assert.ok(
                menu.groups
                    .flatMap(({ rows }) => rows)
                    .find(({ id }) => id === "library.keep_top_revealed")
                    .checked,
            );
        }
    });
}

test("menu rows show Transform for a two-faced card", () => {
    const printed = new Map([
        [
            1,
            {
                card: {
                    faces: [{ name: "Front" }, { name: "Back" }],
                    layout: "transform",
                },
                face: 0,
            },
        ],
    ]);
    assert.ok(
        labels(
            rows(
                card("permanent", "battlefield", "you", "Front"),
                view,
                printed,
            ),
        ).includes("Transform"),
    );
    assert.ok(
        !labels(rows(card("permanent", "battlefield"))).includes("Transform"),
    );
});

test("menu permissions distinguish public cards, inaccessible cards and lent piles", () => {
    const tap = commands.find(({ id }) => id === "permanent.tap");
    const peek = commands.find(({ id }) => id === "permanent.peek");
    const search = commands.find(({ id }) => id === "library.search");
    const draw = commands.find(({ id }) => id === "library.draw");
    const mine = card("permanent", "battlefield");
    mine.object.facing = "down";
    const theirs = card("their_permanent", "battlefield", "opponent");
    theirs.object.facing = "down";
    assert.equal(allowed(tap, mine, view), true);
    assert.equal(allowed(tap, theirs, view), true);
    assert.equal(allowed(peek, mine, view), true);
    assert.equal(allowed(peek, theirs, view), false);
    assert.equal(allowed(tap, { ...theirs, action: undefined }, view), false);
    const library = { kind: "library", side: "opponent" };
    assert.equal(allowed(search, library, view), false);
    const lent = {
        ...view,
        table: {
            ...view.table,
            acting: [{ side: "opponent", zone: "library" }],
        },
    };
    assert.equal(allowed(search, library, lent), true);
    assert.equal(allowed(draw, library, lent), false);
    const revealedHand = {
        ...lent,
        table: {
            ...lent.table,
            acting: [...lent.table.acting, { side: "opponent", zone: "hand" }],
        },
    };
    assert.equal(allowed(draw, library, revealedHand), false);
    assert.equal(
        allowed(draw, library, {
            ...lent,
            table: { ...lent.table, lentHand: true },
        }),
        true,
    );
    const hand = rows({ kind: "hand", side: "opponent" });
    assert.deepEqual(hand.header, { title: "Hand", whose: "2 cards" });
    assert.deepEqual(hand.groups, []);
});

test("fill substitutes nested arrays and values, preserves text, and rejects missing references", () => {
    const template = {
        batch: {
            actions: [
                { attach: { objects: "$objects", in: "$in", host: "$target" } },
                { note: { objects: "$objects", in: "$in", note: "$text" } },
                { draw: { seat: "$seat", count: "$number" } },
            ],
        },
    };
    const values = {
        objects: [{ view: "one", handle: 4 }],
        in: "battlefield",
        target: { id: { view: "one", handle: 7 }, in: "battlefield" },
        seat: 0,
        number: 3,
        text: "$objects",
    };
    assert.deepEqual(fill(template, values), {
        batch: {
            actions: [
                {
                    attach: {
                        objects: values.objects,
                        in: values.in,
                        host: values.target,
                    },
                },
                {
                    note: {
                        objects: values.objects,
                        in: values.in,
                        note: "$objects",
                    },
                },
                { draw: { seat: 0, count: 3 } },
            ],
        },
    });
    assert.throws(() => fill(template, {}), /Missing table action value/);
});
