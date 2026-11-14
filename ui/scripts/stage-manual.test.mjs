import assert from "node:assert/strict";
import { test } from "node:test";
import { zoneCards } from "../src/game/zoneCards.ts";
import { stepAnnouncement } from "../src/stage/notices.ts";
import { apply, tableOf } from "../src/stage/table.ts";
import { timingOf } from "../src/stage/timing.ts";
import { object, row, stage, view } from "./stage-table.mjs";

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
        [{ kind: "phase", phase: "firstMain" }, "First main"],
        [{ kind: "tableTurn", name: "Seat 2" }, "Seat 2's turn"],
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

test("resetting with identical manual markers keeps their stage names", () => {
    const first = 65_537;
    const second = 65_538;
    const marker = (id) => ({
        kind: "ability",
        source: null,
        name: "Borrowed Token",
        text: "Ability",
        controller: "opponent",
        manual: { id, face: "back" },
        targets: [],
    });
    const before = view([], [], {
        status: { kind: "free" },
        stack: [marker(first), marker(second)],
    });
    const { stage: made, surface } = stage({ vanish: { duration: 200 } });
    made.play(before, { reset: { abilities: [first, second] }, beats: [] });
    made.play(before);
    const firstMarker = made.shown.resting.get(`a${first}`);
    assert.equal(firstMarker.source, undefined);
    assert.equal(firstMarker.ability.controller, "opponent");
    assert.equal(firstMarker.ability.manual.face, "back");
    assert.ok(made.shown.resting.has(`a${second}`));

    const act = {
        verb: "vanish",
        from: "stack",
        why: "resolve",
        subject: { kind: "ability", id: second },
    };
    const after = { ...before, stack: [marker(first)] };
    made.play(after, { beats: [[{ act }]] });
    made.settle();
    assert.deepEqual(made.shown.table.stack, [marker(first)]);
    assert.ok(made.shown.resting.has(`a${first}`));
    assert.equal(made.pose(`a${second}`), null);
    assert.deepEqual(surface.log, [
        `make a${first}`,
        `make a${second}`,
        `remove a${second}`,
    ]);
});

test("an anonymous draw flies from its pile with a fresh slot", () => {
    const owner = "opponent";
    const old = object(1, "library", { owner, controller: owner });
    const back = object(2, "hand", { owner, controller: owner, name: null });
    const empty = { hand: [], exile: [], sideboard: [] };
    const initial = view([], [], {
        objects: [old],
        status: { kind: "free" },
        table: { orders: { you: empty, opponent: empty } },
    });
    initial.players[owner].library = [old.slot];
    const forget = { verb: "set", fact: { kind: "forget", slots: [old.slot] } };
    const fly = {
        verb: "fly",
        why: "draw",
        from: "library",
        object: back,
        origin: { zone: "library", side: owner },
    };
    const after = apply(apply(tableOf(initial), forget), fly).view;
    assert.deepEqual(after.objects, [back]);
    assert.deepEqual(after.players[owner].library, []);
    assert.deepEqual(after.table.orders[owner].hand, [back.slot]);
    const layout = {
        ...row,
        anchors: (table) => ({
            ...row.anchors(table),
            pile: () => ({ x: 3000, y: 500, width: 100 }),
        }),
    };
    const { stage: made } = stage(
        { fly: { duration: 100 } },
        undefined,
        layout,
    );
    made.play(initial);
    made.play(after, { beats: [[{ act: forget }, { act: fly }]] });
    made.clock.advance(50);
    const pose = made.pose(back.slot);
    assert.equal(pose.face, "back");
    assert.ok(pose.x > 0 && pose.x < 3000);
    assert.equal(made.pose(old.slot), null);
});

test("a manual look keeps a counted packet lifted through unrelated updates", () => {
    const library = [1, 2, 3, 4];
    const empty = { hand: [], exile: [], sideboard: [] };
    const before = view([], [], {
        status: { kind: "free" },
        table: { orders: { you: empty, opponent: empty } },
    });
    before.players.you.library = library;
    before.objects = library.map((slot) => object(slot, "library"));
    const handling = {
        zone: "pile",
        pile: "library",
        side: "you",
        count: 2,
        bottom: true,
    };
    const opened = { ...before, handling };
    const lift = { verb: "lift", span: "begin", handling };
    const close = { ...lift, span: "end" };
    const layout = {
        ...row,
        rest(table) {
            const poses = row.rest(table);
            for (const slot of table.handled ?? []) {
                const pose = poses.get(slot).pose;
                pose.y += 250;
            }
            return poses;
        },
    };
    const { stage: made } = stage(
        { lift: { duration: 200 } },
        undefined,
        layout,
    );
    made.play(before);
    const y = made.pose(1).y;
    made.play(opened, { beats: [[{ act: lift }]] });
    made.clock.advance(100);
    assert.ok(made.pose(1).y > y && made.pose(1).y < y + 250);
    made.settle();
    assert.equal(made.pose(1).y, y + 250);
    assert.equal(made.pose(2).y, y + 250);
    assert.equal(made.pose(3).y, y);

    made.play(opened, { beats: [] });
    made.settle();
    assert.equal(made.pose(1).y, y + 250);
    made.play(before, { beats: [[{ act: close }]] });
    made.settle();
    assert.equal(made.pose(1).y, y);

    made.play(opened);
    assert.equal(made.pose(1).y, y + 250);
});
