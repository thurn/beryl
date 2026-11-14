import assert from "node:assert/strict";
import { test } from "node:test";
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
