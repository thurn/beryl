// The stage's streaks, its announcements and its waits, with no browser (`npm test`): a
// `streak` step draws a comet from where its source lay to where its target is, for as long as
// it plays; an `announce` step that passes over the table holds up nothing and plays on to its
// own end over the beats and scripts that follow. How each step is timed and which effects
// it begins is the session's (beryl_session's script/timing.rs and its tests). The same in a
// browser, from both seats and in both layouts, is verbs.mjs (verb-cases-rest.mjs).
import assert from "node:assert/strict";
import { test } from "node:test";
import { LIFT, lifts } from "../src/stage/lift.ts";
import { bowOf } from "../src/stage/bows.ts";
import { glide } from "../src/stage/tracks.ts";
import { move, object, reach, row, stage, view } from "./stage-table.mjs";

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
        notice: { kind: "coin", player: "you", won: false, source: 1 },
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

test("a coin has the table to itself: it waits for the turn's banner to go", () => {
    const { stage: made } = stage((act) =>
        act.verb !== "announce"
            ? { duration: 100 }
            : act.notice.kind === "coin"
              ? { duration: 300, alone: true }
              : { duration: 1000, passes: true },
    );
    made.play(view([1, 2]));
    const told = [];
    made.follow(({ edge, at, step }) => told.push([edge, at, step.act.verb]));
    made.play(view([2, 1]), { beats: [[banner], [move(2, 0)], [coin]] });
    made.clock.advance(2000);
    assert.deepEqual(told, [
        ["start", 0, "announce"],
        ["start", 0, "fly"],
        ["end", 100, "fly"],
        ["end", 1000, "announce"],
        ["start", 1000, "announce"],
        ["end", 1300, "announce"],
    ]);
    // With no banner up it is thrown at once.
    told.length = 0;
    made.play(view([2, 1]), { beats: [[coin]] });
    made.clock.advance(1000);
    assert.deepEqual(told, [
        ["start", 2000, "announce"],
        ["end", 2300, "announce"],
    ]);
});

test("a land from the left of the opponent's hand swings past their life badge", () => {
    const hand = object(1, "hand", {
        name: null,
        controller: "opponent",
        owner: "opponent",
    });
    const land = { ...hand, name: "Land", zone: "battlefield" };
    const start = { x: 600, y: -107, width: 105 };
    const end = { x: 510, y: 112, width: 78 };
    const layout = {
        ...row,
        opponentLandTurn: () => ({ x: 1072, y: 270, width: 144 }),
        rest(table) {
            const found = row.rest(table);
            const card = found.get(1);
            Object.assign(card.pose, card.object.zone === "hand" ? start : end);
            return found;
        },
    };
    const { stage: made } = stage(
        { fly: { duration: 615 } },
        undefined,
        layout,
    );
    const act = { verb: "fly", from: "hand", object: land, why: "play" };
    made.play(view([], [1], { objects: [hand] }));
    assert.equal(made.pose(1).face, "back");
    made.play(view([1], [], { objects: [land] }), { beats: [[{ act }]] });
    made.clock.advance(0);
    const leaving = made.pose(1);
    assert.deepEqual(
        [leaving.x, leaving.y, leaving.face],
        [start.x, start.y, "front"],
    );
    made.clock.advance(265);
    const turn = made.pose(1);
    assert.ok(
        turn.x + turn.width / 2 > 1040,
        "it clears the right of the life badge",
    );
    assert.ok(
        turn.y + turn.width * 0.7 > 240,
        "it reaches down into the table",
    );
    assert.ok(turn.flat < 0.8, "it pitches towards the table");
    assert.ok(
        turn.shadow.x < turn.x && turn.shadow.y > turn.y + turn.width,
        "its shadow separates from the card",
    );
    made.clock.advance(110);
    assert.ok(made.pose(1).x < turn.x, "it glides back to the left");
    assert.ok(made.pose(1).width > 140, "it stays wide as it starts back");
    made.clock.advance(240);
    const landed = made.pose(1);
    assert.equal(landed.shadow, undefined);
    assert.deepEqual(
        [landed.x, landed.y, landed.width, landed.rotation],
        [end.x, end.y, end.width, 0],
    );
    assert.equal(
        bowOf({ ...act, object: { ...land, controller: "you" } }, 1),
        undefined,
    );
});

test("a permanent leaving the stack is thrown up, crosses high, and drops onto its place", () => {
    const resolved = {
        verb: "fly",
        from: "stack",
        object: { ...object(1), zone: "battlefield", controller: "you" },
    };
    const cast = { ...resolved, from: "hand" };
    assert.deepEqual([lifts(resolved), lifts(cast)], [true, false]);
    assert.equal(bowOf(resolved, 1), undefined, "it bows no other way");
    // From the stack at the right, 200 wide, to a place 400 to its left and 100 below it,
    // where it rests 100 wide: the same for either player, whose table the camera looks
    // down on from the same side.
    const flat = { rotation: 0, skew: 0, turn: 0, opacity: 1, depth: 0 };
    const [start, rest] = [
        { ...flat, x: 1000, y: 300, width: 200, face: "front" },
        { ...flat, x: 600, y: 400, width: 100, face: "front" },
    ];
    const even = (progress) => progress;
    const path = glide(start, even, true, { lift: true });
    const middle = (progress) => {
        const pose = path(progress, rest);
        return { x: pose.x + pose.width / 2, y: pose.y, width: pose.width };
    };
    // It leaves from where it lay, and two frames of sixty later it is already smaller.
    assert.deepEqual(middle(0), { x: 1100, y: 300, width: 200 });
    const small = middle(LIFT.shrunk).width;
    assert.ok(
        small < LIFT.width * 200 && small > 150,
        `it is smaller: ${small}`,
    );
    // A quarter of its time through it is as high as it goes, well above where it left
    // though its place is below that, and nearly half way across (Arena's is half way
    // across and 0.2 of the table's height above its line, `UrGalw_f35w 17:01.26`).
    const quarter = middle(0.25);
    assert.ok(quarter.y < start.y - 100, `it has risen: ${quarter.y}`);
    assert.ok(
        quarter.x < 1100 - 0.3 * 450 && quarter.x > 1100 - 0.6 * 450,
        `it is nearly half way across: ${quarter.x}`,
    );
    // It is still that high past half its time, then comes down evenly, narrowing.
    const [half, late] = [middle(0.55), middle(0.8)];
    assert.ok(half.y < start.y - 80, `it crosses high: ${half.y}`);
    assert.ok(late.y > half.y + 60 && late.y < rest.y, `it drops: ${late.y}`);
    assert.ok(late.width < half.width && late.width > rest.width);
    assert.deepEqual(middle(1), { x: 650, y: 400, width: 100 });
    // One with no lift keeps to its line.
    const straight = glide(start, even, true)(0.25, rest);
    assert.deepEqual([straight.x, straight.y, straight.width], [900, 325, 175]);
});
