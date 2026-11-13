// The first slice on the stage, with no browser (`npm test`): a creature cast, its enters
// trigger and the token it makes, as the stage plays them and as the screen around the table
// hears of them (src/stage/feed.ts). The same moment in a browser, from both seats, is
// verbs.mjs.
import assert from "node:assert/strict";
import { test } from "node:test";
import { Feed } from "../src/stage/feed.ts";
import { abilityName } from "../src/stage/layout.ts";
import { BOWS } from "../src/stage/tracks.ts";
import { object, stage, view } from "./stage-table.mjs";

// The slice of by-w8gv.21.14, as the session cuts it (script/build.rs): a creature cast, it
// resolves, its enters trigger lifts out of it, a token appears, and the ability is done.
const CREATURE = 3;
const TOKEN = 7;
const TRIGGER = abilityName(1);
const trigger = {
    kind: "ability",
    source: CREATURE,
    name: `Card ${CREATURE}`,
    controller: "you",
    triggered: true,
    targets: [],
};
const cast = {
    act: {
        verb: "fly",
        from: "hand",
        why: "cast",
        object: object(CREATURE, "stack"),
        item: { kind: "spell", slot: CREATURE, targets: [] },
    },
};
const resolve = {
    act: {
        verb: "fly",
        from: "stack",
        why: "resolve",
        object: object(CREATURE),
    },
};
const emerge = { act: { verb: "emerge", id: 1, item: trigger } };
const create = {
    act: {
        verb: "appear",
        from: "nowhere",
        why: "created",
        object: object(TOKEN, "battlefield", { token: true }),
    },
    cause: { kind: "ability", id: 1 },
};
const finish = {
    act: {
        verb: "vanish",
        subject: { kind: "ability", id: 1 },
        from: "stack",
        why: "resolve",
    },
};
const SLICE = {
    fly: { duration: 400 },
    emerge: { duration: 300, delay: 100 },
    appear: { duration: 100 },
};
const before = () => view([1], [CREATURE]);
const onStack = () => {
    const table = view([1]);
    table.objects.push(object(CREATURE, "stack"));
    table.stack = [cast.act.item];
    return table;
};
const triggered = () => ({ ...view([1, CREATURE]), stack: [trigger] });
const after = () => view([1, CREATURE, TOKEN]);

/** Plays the stage on in ticks of `tick` until it has nothing to play, giving it `more()`
 * whenever it runs dry, and returns what `watch` read after each tick while the step `verb`
 * was playing, with the stage time since that step began. */
function during(made, verb, watch, tick, more = () => false) {
    const seen = [];
    for (;;) {
        if (!made.shown.busy && !more()) return seen;
        made.clock.advance(tick);
        const step = made.step;
        if (step?.verb === verb && made.clock.now >= step.from) {
            seen.push([made.clock.now - step.from, watch()]);
        }
    }
}

// The lift is the motion most often broken (12 reports, 7 commits: by-w8gv.21), each time by
// another way the views came batched. Here it starts from the model, so the same steps give
// the same lift however the session cut them into scripts and whenever those arrived.
test("a trigger lifts out of its source the same however its views are batched", () => {
    const lift = (give) => {
        const { stage: made } = stage(SLICE);
        made.play(before(), { reset: { abilities: [] }, beats: [] });
        const more = give(made);
        return during(made, "emerge", () => made.pose(TRIGGER), 25, more);
    };
    // As the session sends them to a seat that answers between: three views, each given when
    // the one before has played out and the stage has stood idle a while.
    const apart = lift((made) => {
        const scripts = [
            [onStack(), { beats: [[cast]] }],
            [triggered(), { beats: [[resolve], [emerge]] }],
            [after(), { beats: [[create], [finish]] }],
        ];
        return () => {
            const next = scripts.shift();
            if (!next) return false;
            made.clock.advance(730);
            made.play(...next);
            return true;
        };
    });
    // All three at once, before the first has begun.
    const queued = lift((made) => {
        made.play(onStack(), { beats: [[cast]] });
        made.play(triggered(), { beats: [[resolve], [emerge]] });
        made.play(after(), { beats: [[create], [finish]] });
    });
    // One view for the whole moment, as a seat that only watches gets it.
    const whole = lift((made) => {
        made.play(after(), {
            beats: [[cast], [resolve], [emerge], [create], [finish]],
        });
    });
    // The lift waits its delay, then leaves the creature where that came to rest, second
    // on the battlefield, small, and is on the stack 300 ms later.
    const at = (ms) => apart.find(([time]) => time === ms)[1];
    assert.deepEqual(
        [at(0).x, at(0).y, at(0).width],
        [400 + 10, 10, 50],
        "it starts on its source",
    );
    // Half way it is off the straight line by its small sag, away from its controller's edge
    // toward the middle of the table, as Arena's is: a quarter turn from the line, which runs
    // 1590 across and 10 up. Its farthest is half way along.
    const off = BOWS.lifts.share;
    assert.ok(off <= 0.025, "it goes all but straight");
    const near = (pose) =>
        [pose.x, pose.y, pose.width].map((n) => Math.round(n * 1e6) / 1e6);
    assert.deepEqual(
        near(at(150)),
        near({ x: 1205 - 10 * off, y: 5 - 1590 * off, width: 75 }),
        "it is half way after half its time, off the line by its bow",
    );
    assert.equal(apart.at(-1)[0], 275);
    assert.equal(apart.length, 12);
    assert.deepEqual(queued, apart);
    assert.deepEqual(whole, apart);
});

test("a token appears at its place with no flight, and the ability is gone at once", () => {
    const { stage: made, surface } = stage(SLICE);
    made.play(triggered(), { reset: { abilities: [1] }, beats: [] });
    assert.deepEqual(surface.log, ["make 1", "make 3", "make a1"]);
    assert.deepEqual([made.pose(TRIGGER).x, made.pose(TRIGGER).y], [2000, 0]);
    made.play(after(), { beats: [[create], [finish]] });
    made.clock.advance(50);
    // Half way through its appearing it is where it will rest, half seen, and nothing else
    // has moved.
    assert.deepEqual(
        [made.pose(TOKEN).x, made.pose(TOKEN).y, made.pose(TOKEN).opacity],
        [800, 0, 0.5],
    );
    assert.equal(made.pose(TOKEN).depth, TOKEN, "it lies with what rests");
    assert.deepEqual([made.pose(1).x, made.pose(CREATURE).x], [0, 400]);
    assert.notEqual(made.pose(TRIGGER), null);
    made.clock.advance(50);
    // The ability's step begins as the token's ends, and takes its node in the same tick.
    assert.equal(made.pose(TOKEN).opacity, 1);
    assert.equal(made.pose(TRIGGER), null);
    assert.equal(surface.log.at(-1), "remove a1");
    assert.equal(made.shown.busy, false);
});

test("the table opens for a token in its delay, and the token is seen as the delay ends", () => {
    const { stage: made } = stage({
        appear: { duration: 100, delay: 200, opens: true },
    });
    made.play(view([1, CREATURE]));
    const told = [];
    made.follow(({ edge, at }) => told.push([edge, at, made.clock.now]));
    // The token takes the first place of its row: the two permanents make room for it.
    const first = { ...create, act: { ...create.act, at: 0 } };
    made.play(view([TOKEN, 1, CREATURE]), { beats: [[first]] });
    const seen = () => [
        made.pose(1).x,
        made.pose(CREATURE).x,
        made.pose(TOKEN).opacity,
    ];
    // Half way through the delay the row is most of the way there, slowing, and the token,
    // which the table already holds, is not seen: its step has not begun.
    made.clock.advance(100);
    assert.deepEqual(seen(), [300, 700, 0]);
    assert.deepEqual(told, []);
    assert.equal(made.step.verb, "appear");
    // As the delay ends the row has made room, and the token begins to show where it rests.
    made.clock.advance(100);
    assert.deepEqual(seen(), [400, 800, 0]);
    assert.deepEqual(told, [["start", 200, 200]]);
    made.clock.advance(50);
    assert.deepEqual(seen(), [400, 800, 0.5]);
    assert.equal(made.pose(TOKEN).x, 0);
    made.clock.advance(50);
    assert.deepEqual(told.at(-1), ["end", 300, 300]);
    assert.equal(made.shown.busy, false);
    // However the time is reached, each edge is told at its own time, in order.
    const { stage: again } = stage({
        appear: { duration: 100, delay: 200, opens: true },
    });
    again.play(view([1, CREATURE]));
    const edges = [];
    again.follow(({ edge, at }) => edges.push([edge, at]));
    again.play(view([TOKEN, 1, CREATURE]), { beats: [[first]] });
    again.settle();
    assert.deepEqual(edges, [
        ["start", 200],
        ["end", 300],
    ]);
});

test("a card that appears from another zone does not fly there", () => {
    const { stage: made } = stage(SLICE);
    made.play(view([1], [2]));
    const lands = {
        act: {
            verb: "appear",
            from: "hand",
            why: "effect",
            object: object(2),
        },
    };
    made.play(view([1, 2]), { beats: [[lands]] });
    made.clock.advance(25);
    assert.deepEqual(
        [made.pose(2).x, made.pose(2).y, made.pose(2).opacity],
        [400, 0, 0.25],
    );
});

/** An update of session `session` holding `shown`. */
const update = (shown, script, session = 1) => ({
    session,
    view: {
        ...shown,
        script,
        events: [],
        status: { kind: "playing" },
    },
});

test("the screen takes a view when its script has played out, never before", () => {
    const { stage: made } = stage(SLICE);
    const feed = new Feed(made);
    let heard = 0;
    feed.subscribe(() => heard++);
    const first = update(before(), { reset: { abilities: [] }, beats: [] });
    assert.equal(feed.arrive(first, false), true, "it began a game");
    assert.equal(feed.fed().shown, first);
    assert.equal(feed.fed().behind, false);
    const second = update(onStack(), { beats: [[cast]] });
    const third = update(triggered(), { beats: [[resolve], [emerge]] });
    assert.equal(feed.arrive(second, false), false);
    feed.arrive(third, false);
    assert.equal(feed.fed().shown, first);
    assert.equal(feed.fed().behind, true);
    made.clock.advance(399);
    assert.equal(feed.fed().shown, first);
    made.clock.advance(1);
    // The cast has played; the next script is under way in the same tick.
    assert.equal(feed.fed().shown, second);
    assert.equal(feed.fed().behind, true);
    made.clock.advance(799);
    assert.equal(feed.fed().shown, second);
    heard = 0;
    made.clock.advance(1);
    assert.equal(feed.fed().shown, third);
    assert.equal(feed.fed().behind, false);
    assert.equal(heard, 1, "the screen hears once of a script's end");
});

test("a skip plays out everything given; a view from the past shows at once", () => {
    const { stage: made } = stage(SLICE);
    const feed = new Feed(made);
    feed.arrive(
        update(before(), { reset: { abilities: [] }, beats: [] }),
        true,
    );
    const second = update(onStack(), { beats: [[cast]] });
    feed.arrive(second, true);
    assert.equal(feed.fed().shown, second);
    assert.equal(made.pose(CREATURE).x, 2000);
    const third = update(triggered(), { beats: [[resolve], [emerge]] });
    const fourth = update(after(), { beats: [[create], [finish]] });
    feed.arrive(third, false);
    feed.arrive(fourth, false);
    made.clock.advance(100);
    assert.equal(feed.fed().shown, second);
    feed.skip();
    assert.equal(feed.fed().shown, fourth);
    assert.equal(feed.fed().behind, false);
    assert.equal(made.pose(TOKEN).x, 800);
    assert.equal(made.pose(TRIGGER), null);
    // An earlier game's view, arriving late, is not this game's.
    feed.arrive(update(before(), { beats: [] }, 0), false);
    assert.equal(feed.fed().shown, fourth);
});
