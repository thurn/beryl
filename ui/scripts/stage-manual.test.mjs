import assert from "node:assert/strict";
import { test } from "node:test";
import { zoneCards } from "../src/game/zoneCards.ts";
import { stepAnnouncement } from "../src/stage/notices.ts";
import { apply, tableOf } from "../src/stage/table.ts";
import { timingOf } from "../src/stage/timing.ts";
import { object, stage, view } from "./stage-table.mjs";

test("manual numbers replay through count steps at full precision", () => {
    const before = view([1], [], { status: { kind: "free" } });
    const mana = {
        white: 0,
        blue: 2_000_000,
        black: 0,
        red: 0,
        green: 0,
        colorless: 0,
    };
    const playerCounters = [{ name: "Energy", count: 2_147_483_647 }];
    const stats = object(1, "battlefield", {
        pt: { power: 200, toughness: -200 },
    });
    const damaged = { ...stats, damage: 300 };
    const counted = { ...damaged, counters: [{ kind: "plusOne", count: 999 }] };
    const finished = {
        ...counted,
        namedCounters: [{ name: "Page", count: 999 }],
    };
    const tallies = JSON.parse(
        JSON.stringify([
            { kind: "life", player: "you", life: 40_000 },
            { kind: "pool", player: "you", mana },
            { kind: "playerCounters", player: "you", counters: playerCounters },
            { kind: "stats", object: stats },
            { kind: "damage", object: damaged },
            { kind: "counters", counter: "plusOne", object: counted },
            { kind: "namedCounters", name: "Page", object: finished },
        ]),
    );
    const replay = (steps) =>
        steps.reduce(
            (table, tally) => apply(table, { verb: "count", tally }),
            tableOf(before),
        ).view;
    const after = {
        ...before,
        objects: [finished],
        players: {
            ...before.players,
            you: {
                ...before.players.you,
                life: 40_000,
                mana,
                namedCounters: playerCounters,
            },
        },
    };
    assert.deepEqual(replay(tallies), after);
    assert.notDeepEqual(
        replay(tallies.filter(({ kind }) => kind !== "playerCounters")),
        after,
    );
    const cleared = apply(tableOf(after), {
        verb: "count",
        tally: { kind: "playerCounters", player: "you", counters: [] },
    });
    assert.equal(cleared.view.players.you.namedCounters, undefined);
});

test("manual notices play on the stage and read as banners without a card source", () => {
    const initial = view([], [], { status: { kind: "free" } });
    for (const [notice, title] of [
        [{ kind: "roll", sides: 6, outcomes: [3] }, "Rolled 4 (d6)"],
        [{ kind: "roll", sides: 2, outcomes: [0] }, "Heads"],
        [{ kind: "shuffle" }, "Shuffled"],
        [{ kind: "rewind", to: 41 }, "Rewound to 41"],
    ]) {
        const { stage: made, surface } = stage((act, found) =>
            timingOf(act, found),
        );
        const act = { verb: "announce", notice };
        const rewind = notice.kind === "rewind";
        const next = {
            ...initial,
            players: {
                ...initial.players,
                you: { ...initial.players.you, life: rewind ? 12 : 20 },
            },
        };
        made.play(initial);
        made.play(next, {
            ...(rewind && { reset: { abilities: [] } }),
            beats: [[{ act }]],
        });
        assert.equal(made.shown.table.players.you.life, next.players.you.life);
        const { duration, delay } = timingOf(act, initial);
        made.clock.advance(delay + duration / 2);
        assert.equal(surface.notices.get(notice.kind), 0.5);
        assert.deepEqual(
            stepAnnouncement(made.shown.notices, made.shown.table),
            {
                kind: notice.kind,
                title,
                coin: false,
            },
        );
        made.clock.advance(duration);
        assert.equal(
            stepAnnouncement(made.shown.notices, made.shown.table),
            undefined,
        );
    }
});

test("manual scripts position cards by slot in each side's ordered zones", () => {
    const orders = {
        you: { hand: [1, 2, 3], exile: [4], sideboard: [5] },
        opponent: { hand: [], exile: [], sideboard: [] },
    };
    const before = view([], [1, 2, 3], {
        status: { kind: "free" },
        table: { orders },
    });
    before.objects.push(object(4, "exile"), object(5, "sideboard"));
    const slots = (table, zone, side = "you") =>
        zoneCards(table.view, side, zone).map((card) => card.slot);
    const reorder = {
        verb: "fly",
        from: "hand",
        why: "effect",
        at: 0,
        object: object(3, "hand"),
    };
    const movedExile = object(4, "exile", { zoneSide: "opponent" });
    const exiled = object(5, "exile");
    const sideboard = object(7, "sideboard");
    const acts = [
        reorder,
        {
            verb: "appear",
            from: "nowhere",
            why: "created",
            at: 0,
            object: object(6, "exile"),
        },
        { verb: "fly", from: "exile", why: "effect", object: movedExile },
        {
            verb: "appear",
            from: "nowhere",
            why: "created",
            at: 0,
            object: sideboard,
        },
        {
            verb: "vanish",
            from: "sideboard",
            why: "exile",
            subject: { kind: "object", slot: 5 },
            at: 0,
            object: exiled,
        },
        {
            verb: "vanish",
            from: "exile",
            why: "effect",
            subject: { kind: "object", slot: 6 },
        },
    ];
    const start = tableOf(before);
    const reordered = apply(start, reorder);
    assert.deepEqual(slots(reordered, "hand"), [3, 1, 2]);
    assert.deepEqual(reordered.view.objects, start.view.objects);
    const exile = apply(reordered, acts[1]);
    assert.deepEqual(slots(exile, "exile"), [6, 4]);

    const sideboardAdded = acts
        .slice(0, 4)
        .reduce((table, act) => apply(table, act), start);
    assert.deepEqual(slots(sideboardAdded, "sideboard"), [7, 5]);
    const after = acts
        .slice(4)
        .reduce((table, act) => apply(table, act), sideboardAdded);
    assert.deepEqual(after.view, {
        ...before,
        objects: [...before.objects.slice(0, 3), movedExile, exiled, sideboard],
        table: {
            orders: {
                you: { hand: [3, 1, 2], exile: [5], sideboard: [7] },
                opponent: { hand: [], exile: [4], sideboard: [] },
            },
        },
    });
    assert.deepEqual(slots(after, "sideboard"), [7]);
    assert.deepEqual(slots(after, "exile", "opponent"), [4]);
    assert.deepEqual(slots(start, "hand"), [1, 2, 3]);
});

test("a manual hand fly moves the existing card toward its new place during the beat", () => {
    const hand = [1, 2, 3];
    const initial = view([], hand, {
        status: { kind: "free" },
        table: {
            orders: {
                you: { hand, exile: [], sideboard: [] },
                opponent: { hand: [], exile: [], sideboard: [] },
            },
        },
    });
    const { stage: made, surface } = stage({ fly: { duration: 200 } });
    const reorder = {
        verb: "fly",
        from: "hand",
        why: "effect",
        at: 0,
        object: object(3, "hand"),
    };
    const { view: after } = apply(tableOf(initial), reorder);
    made.play(initial);
    const was = made.pose(3).x;
    made.play(after, { beats: [[{ act: reorder }]] });
    made.clock.advance(100);
    assert.ok(made.pose(3).x > 0 && made.pose(3).x < was);
    assert.deepEqual(made.shown.table.table.orders.you.hand, [3, 1, 2]);
    made.settle();
    assert.equal(made.pose(3).x, 0);
    assert.deepEqual(surface.log, ["make 1", "make 2", "make 3"]);
});
