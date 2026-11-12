// The stage's streaks, its announcements and its waits, with no browser (`npm test`): a
// `streak` step draws a comet from where its source lay to where its target is, for as long as
// it plays; an `announce` step that passes over the table holds up nothing and plays on to its
// own end over the beats and scripts that follow; and the design language's timings give each
// of the verbs that move no card its case. The same in a browser, from both seats and in both
// layouts, is verbs.mjs (verb-cases-rest.mjs).
import assert from "node:assert/strict";
import { test } from "node:test";
import { caseOf, effectsOf, timingOf } from "../src/stage/timing.ts";
import { move, object, reach, stage, view } from "./stage-table.mjs";

const card = (slot) => ({ kind: "object", slot });
/** A streak of one blue mana from `from` into `to`. */
const mana = (from, to, more = {}) => ({
    act: {
        verb: "streak",
        ...(from && { from }),
        to,
        stream: { kind: "mana", color: "blue" },
        amount: 1,
        ...more,
    },
});
const banner = {
    act: { verb: "announce", notice: { kind: "turn", active: "you" } },
};
const coin = {
    act: {
        verb: "announce",
        notice: { kind: "coin", player: "you", won: false },
    },
};
/** The first point of a path: where a streak's tail is. */
const tailOf = (d) =>
    d
        .match(/^M(-?[\d.]+) (-?[\d.]+)/)
        .slice(1)
        .map(Number);

test("a streak leaves where its source lay and lands on what it pays for", () => {
    const { stage: made, surface } = stage({ streak: { duration: 200 } });
    made.play(view([1, 2]));
    made.play(view([1, 2]), { beats: [[mana(card(1), card(2))]] });
    made.clock.advance(0);
    const [first] = made.streaks();
    assert.equal(first.look, "blue");
    assert.equal(first.to, 2);
    assert.equal(first.through, 0);
    // Permanent 1 lies at 0, 0, 100 wide and 140 tall: its middle is where the streak leaves.
    assert.deepEqual(first.shape.tail, { x: 50, y: 70 });
    assert.deepEqual(tailOf(surface.streaks.get(first.key)), [50, 70]);
    const heads = [];
    for (const at of [50, 100, 150, 199]) {
        reach(made.clock, at, 16);
        const [streak] = made.streaks();
        assert.equal(streak.through, at / 200);
        heads.push(streak.shape.head);
        assert.equal(surface.streaks.get(streak.key), streak.shape.d);
    }
    // Its head goes from one to the other on an arc that rises up the stage.
    for (const [i, head] of heads.entries()) {
        assert.ok(head.x > (heads[i - 1]?.x ?? 50), "the head goes on");
        assert.ok(head.y < 70, `the head is at y ${head.y}`);
    }
    assert.ok(Math.hypot(heads.at(-1).x - 450, heads.at(-1).y - 70) < 5);
    // The tail has left the source by then, and is drawn in as the head lands.
    const last = made.streaks()[0].shape;
    assert.ok(last.tail.x > 400, `the tail is at x ${last.tail.x}`);
    made.clock.advance(1);
    assert.deepEqual(made.streaks(), []);
    assert.deepEqual(surface.streaked, ["+ blue 1", "- blue 1"]);
    assert.equal(surface.streaks.size, 0);
});

test("a streak is the same however its time is reached", () => {
    const read = (tick) => {
        const { stage: made } = stage({ streak: { duration: 300 } });
        made.play(view([1, 2]));
        made.play(view([1, 2]), { beats: [[mana(card(1), card(2))]] });
        reach(made.clock, 120, tick);
        return made.streaks();
    };
    assert.deepEqual(read(16), read(120));
});

test("a streak keeps to where its source lay, and follows what it lands on", () => {
    const { stage: made } = stage({
        streak: { duration: 200 },
        fly: { duration: 200 },
    });
    made.play(view([1, 2, 3]));
    // Permanent 3 changes places with the first while a streak from 1 goes into it, and
    // then 1 moves on too.
    made.play(view([3, 1, 2]), {
        beats: [[move(3, 0), mana(card(1), card(3))]],
    });
    made.clock.advance(0);
    assert.deepEqual(made.streaks()[0].shape.tail, { x: 50, y: 70 });
    made.clock.advance(60);
    // Still at the place the source had as the step began, though the source is gliding away.
    assert.notEqual(made.pose(1).x, 0);
    assert.deepEqual(made.streaks()[0].shape.tail, { x: 50, y: 70 });
    made.clock.advance(139);
    const { head } = made.streaks()[0].shape;
    const target = made.pose(3);
    assert.ok(
        Math.hypot(head.x - (target.x + 50), head.y - (target.y + 70)) < 6,
        "the head closes on its target where that now is",
    );
});

test("streaks that leave one place for one target together fan out", () => {
    const { stage: made } = stage({ streak: { duration: 200 } });
    made.play(view([1, 2, 3]));
    const from = { kind: "player", player: "you" };
    const spent = mana(from, card(1));
    made.play(view([1, 2, 3]), {
        beats: [[spent, spent, spent, mana(card(2), card(1))]],
    });
    made.clock.advance(60);
    const [first, second, third, other] = made.streaks().map((s) => s.shape);
    assert.deepEqual(second.tail, first.tail);
    // To either side of the way by turns, and then higher.
    assert.ok(second.head.y > first.head.y, "the second on the other side");
    assert.ok(third.head.y < first.head.y, "the third higher than the first");
    assert.notDeepEqual(other.tail, first.tail);
    // And each closes on the one target.
    made.clock.advance(139);
    const heads = made.streaks().map((s) => s.shape.head);
    for (const head of heads) {
        assert.ok(
            Math.hypot(head.x - 50, head.y - 70) < 8,
            JSON.stringify(head),
        );
    }
});

test("a streak rises out of its row toward the middle of the table", () => {
    const { stage: made } = stage({ streak: { duration: 200 } });
    const theirs = (slot) =>
        object(slot, "battlefield", { controller: "opponent" });
    const table = view([1, 2], [], {
        objects: [theirs(1), object(2)],
    });
    made.play(table);
    // Their permanent's mana bows down the stage, and yours up it.
    made.play(table, {
        beats: [[mana(card(1), card(2)), mana(card(2), card(1))]],
    });
    made.clock.advance(100);
    const [down, up] = made.streaks().map((s) => s.shape.head);
    assert.ok(down.y > 70, `theirs is at y ${down.y}`);
    assert.ok(up.y < 70, `yours is at y ${up.y}`);
});

test("mana a player spends leaves their pool", () => {
    const { stage: made } = stage({ streak: { duration: 200 } });
    made.play(view([1]));
    const from = { kind: "player", player: "you" };
    made.play(view([1]), { beats: [[mana(from, card(1))]] });
    made.clock.advance(0);
    // Your pool lies from 1200, 800, and its first symbol's middle is 24, 17 in from there.
    assert.deepEqual(made.streaks()[0].shape.tail, { x: 1224, y: 817 });
});

test("a source with no place on the table sends no streak", () => {
    const { stage: made, surface } = stage({ streak: { duration: 200 } });
    made.play(view([1]));
    made.play(view([1]), {
        beats: [[mana(undefined, card(1))], [mana(card(9), card(1))]],
    });
    for (const at of [0, 200]) {
        made.clock.advance(at - made.clock.now);
        const [streak] = made.streaks();
        assert.equal(streak.shape, null);
        assert.equal(surface.streaks.get(streak.key), null);
    }
    made.settle();
    assert.equal(surface.streaks.size, 0);
});

test("a reset and a surface taken away leave no streak drawn", () => {
    const { stage: made, surface } = stage({ streak: { duration: 200 } });
    made.play(view([1, 2]));
    made.play(view([1, 2]), { beats: [[mana(card(1), card(2))]] });
    made.clock.advance(50);
    assert.equal(surface.streaks.size, 1);
    made.unmount();
    assert.equal(surface.streaks.size, 0);
    made.mount(surface);
    assert.equal(surface.streaks.size, 1, "it is still under way");
    made.play(view([1, 2]));
    assert.equal(surface.streaks.size, 0);
    assert.deepEqual(made.streaks(), []);
});

test("what a streak lands on flashes as it lands, and its source glows as it leaves", () => {
    const act = mana(card(1), card(2)).act;
    assert.equal(caseOf(act), "mana");
    assert.deepEqual(
        effectsOf(act).map(({ name, when, on }) => [name, when, on]),
        [
            ["glow", "start", 1],
            ["flash", "end", 2],
        ],
    );
    const burn = {
        ...act,
        from: { kind: "ability", id: 1 },
        to: { kind: "player", player: "opponent" },
        stream: { kind: "damage" },
    };
    assert.equal(caseOf(burn), "damage");
    assert.deepEqual(
        effectsOf(burn).map(({ name, when, on }) => [name, when, on]),
        [
            ["glow", "start", "a1"],
            ["impact", "end", "opponent"],
        ],
    );
    // A pool is no card to glow, and mana from nowhere has no source.
    const spent = { ...act, from: { kind: "player", player: "you" } };
    assert.deepEqual(
        [spent, mana(undefined, card(2)).act].map((a) =>
            effectsOf(a).map((e) => e.name),
        ),
        [["flash"], ["flash"]],
    );
    // The life a streak of damage takes counts as the streak lands.
    const count = {
        verb: "count",
        tally: { kind: "life", player: "opponent", life: 17 },
    };
    const table = view([]);
    const landed = timingOf(count, table, burn);
    assert.equal(landed.with, true);
    assert.equal(landed.delay, timingOf(burn, table).duration);
});

test("an announcement that passes over the table holds up nothing", () => {
    const { stage: made, surface } = stage((act) =>
        act.verb === "announce"
            ? { duration: 1000, passes: true }
            : { duration: 100 },
    );
    made.play(view([1, 2]));
    const told = [];
    made.follow(({ edge, at, step }) => told.push([edge, at, step.act.verb]));
    made.play(view([2, 1]), { beats: [[banner], [move(2, 0)]] });
    made.clock.advance(0);
    // Its beat is over as it begins, and the next beat plays under it.
    assert.deepEqual(made.shown.notices, [banner]);
    assert.equal(made.step.verb, "fly");
    assert.equal(surface.notices.get("turn"), 0);
    made.clock.advance(100);
    assert.equal(made.shown.busy, false, "the script is played out");
    assert.deepEqual(made.shown.notices, [banner], "and the banner still up");
    assert.equal(surface.notices.get("turn"), 0.1);
    // A script that comes meanwhile plays under it too.
    made.play(view([1, 2]), { beats: [[move(1, 0)]] });
    reach(made.clock, 999, 16);
    assert.deepEqual(made.shown.notices, [banner]);
    assert.equal(surface.notices.get("turn"), 0.999);
    made.clock.advance(1);
    assert.deepEqual(made.shown.notices, []);
    assert.equal(surface.notices.has("turn"), false);
    assert.deepEqual(told, [
        ["start", 0, "announce"],
        ["start", 0, "fly"],
        ["end", 100, "fly"],
        ["start", 100, "fly"],
        ["end", 200, "fly"],
        ["end", 1000, "announce"],
    ]);
});

test("an announcement ends at its own time, before what happens after it in one tick", () => {
    const { stage: made } = stage((act) =>
        act.verb === "announce"
            ? { duration: 150, passes: true }
            : { duration: 100 },
    );
    made.play(view([1, 2]));
    const told = [];
    made.follow(({ edge, at, step }) => told.push([edge, at, step.act.verb]));
    made.play(view([2, 1]), { beats: [[banner], [move(2, 0)], [move(1, 0)]] });
    // One tick over all of it.
    made.clock.advance(1000);
    assert.deepEqual(told, [
        ["start", 0, "announce"],
        ["start", 0, "fly"],
        ["end", 100, "fly"],
        ["start", 100, "fly"],
        ["end", 150, "announce"],
        ["end", 200, "fly"],
    ]);
});

test("a beat waits for an announcement to begin, and no longer", () => {
    const { stage: made } = stage((act) =>
        act.verb === "announce"
            ? { duration: 1000, delay: 80, passes: true }
            : { duration: 50 },
    );
    made.play(view([1, 2]));
    const told = [];
    made.follow(({ edge, at, step }) => told.push([edge, at, step.act.verb]));
    made.play(view([2, 1]), { beats: [[move(2, 0), banner], [move(1, 0)]] });
    // However the clock reaches it, the next beat begins as the banner does.
    made.clock.advance(60);
    assert.equal(made.step.verb, "announce");
    made.clock.advance(500);
    assert.deepEqual(told, [
        ["start", 0, "fly"],
        ["end", 50, "fly"],
        ["start", 80, "announce"],
        ["start", 80, "fly"],
        ["end", 130, "fly"],
    ]);
});

test("a coin is waited for, and a reset takes down what was announced", () => {
    const { stage: made, surface } = stage((act) =>
        act.verb === "announce"
            ? { duration: 1000, passes: act.notice.kind !== "coin" }
            : { duration: 100 },
    );
    made.play(view([1, 2]));
    made.play(view([2, 1]), { beats: [[coin], [move(2, 0)]] });
    made.clock.advance(500);
    assert.deepEqual(made.shown.notices, [coin]);
    assert.equal(made.step.verb, "announce", "the move waits for the coin");
    assert.equal(surface.notices.get("coin"), 0.5);
    made.clock.advance(500);
    assert.deepEqual(made.shown.notices, []);
    assert.equal(made.step.verb, "fly");
    made.play(view([2, 1]), { beats: [[banner]] });
    made.clock.advance(200);
    assert.equal(surface.notices.has("turn"), true);
    made.play(view([1]));
    assert.equal(surface.notices.has("turn"), false);
    made.clock.advance(0);
    assert.deepEqual(made.shown.notices, []);
});

test("the design language says which announcements pass and how long each wait is", () => {
    const table = view([]);
    assert.equal(timingOf(banner.act, table).passes, true);
    assert.equal(timingOf(coin.act, table).passes, false);
    const scry = {
        verb: "announce",
        notice: { kind: "scry", player: "you", top: 1, bottom: 1 },
    };
    assert.equal(timingOf(scry, table).passes, true);
    assert.deepEqual(
        [banner.act, scry, coin.act].map((act) => caseOf(act)),
        ["turn banner", "scry pill", "coin"],
    );
    // A spell or ability left to be read, and the table as the game ends.
    const read = { verb: "dwell", subject: card(1) };
    const ended = { verb: "dwell", subject: { kind: "player", player: "you" } };
    assert.equal(caseOf(read), undefined);
    assert.equal(caseOf(ended), "game's end");
    assert.ok(timingOf(ended, table).duration > timingOf(read, table).duration);
    assert.ok(timingOf(read, table).duration > 0);
    // What changes with no motion.
    const turn = { kind: "turn", number: 2, active: "you", step: "untap" };
    const shuffled = { kind: "library", player: "you", order: [] };
    assert.equal(caseOf({ verb: "set", fact: turn }), "step begins");
    assert.equal(caseOf({ verb: "set", fact: shuffled }), "library shuffled");
    assert.equal(
        caseOf({ verb: "set", fact: { kind: "object", object: object(1) } }),
        undefined,
    );
});
