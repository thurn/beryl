// The stage's effects, with no browser (`npm test`): a step begins the effects it is given on
// what it is about, a card or a player's life total, or on what each says it is on, as it
// starts, as it ends or part of the way through, and each plays on the stage's clock for its
// own time or its step's, whatever the script goes on to do. The same in a browser, with the
// design language's effects and looks from both seats, is verbs.mjs.
import assert from "node:assert/strict";
import { test } from "node:test";
import { object, reach, stage, view } from "./stage-table.mjs";

const even = (progress) => progress;
/** An effect as the stage is given one. */
const effect = (name, when, more = {}) => ({
    name,
    layer: "over",
    when,
    delay: 0,
    ease: even,
    ...more,
});
const token = (slot) => ({
    act: {
        verb: "appear",
        from: "nowhere",
        why: "created",
        object: object(slot, "battlefield", { token: true }),
    },
});
const land = (slot) => ({
    act: {
        verb: "fly",
        from: "hand",
        why: "play",
        object: object(slot),
    },
});
const destroy = (slot, to) => ({
    act: {
        verb: "vanish",
        subject: { kind: "object", slot },
        from: "battlefield",
        why: "destroy",
        ...(to ? { object: to } : {}),
    },
});
/** The effects of the tests: a glow of 300 ms under a card that appears, a flash of 200 ms as a
 * flight ends, 50 ms after, and a card that leaves breaking up for as long as it takes to. */
const EFFECTS = (act) =>
    ({
        appear: [effect("glow", "start", { layer: "under", duration: 300 })],
        fly: [effect("settle", "end", { duration: 200, delay: 50 })],
        vanish: [effect("dissolve", "start")],
    })[act.verb] ?? [];
const TIMES = {
    appear: { duration: 100 },
    fly: { duration: 400 },
    vanish: { duration: 600 },
};

for (const tick of [16, 100, 1000]) {
    test(`an effect plays for its own time from the step that begins it: ticks of ${tick} ms`, () => {
        const { stage: made, surface } = stage(TIMES, EFFECTS);
        made.play(view([1]));
        made.play(view([1, 2]), { beats: [[token(2)]] });
        made.clock.advance(0);
        assert.deepEqual(made.effect(2), [
            { name: "glow", layer: "under", progress: 0 },
        ]);
        assert.deepEqual([...surface.effects], [["2 glow under", 0]]);
        assert.deepEqual(made.effect(1), [], "only what the step is about");
        // The step is over at 100 ms and the stage has nothing left to play: the glow goes on.
        reach(made.clock, 150, tick);
        if (made.clock.now === 150) {
            assert.equal(made.shown.busy, false);
            assert.deepEqual(made.effect(2), [
                { name: "glow", layer: "under", progress: 0.5 },
            ]);
            assert.equal(surface.effects.get("2 glow under"), 0.5);
        }
        reach(made.clock, 300, tick);
        assert.deepEqual(made.effect(2), []);
        assert.equal(surface.effects.size, 0, "nothing of it stays");
    });

    test(`an effect of a step's end begins there, after its delay: ticks of ${tick} ms`, () => {
        const { stage: made, surface } = stage(TIMES, EFFECTS);
        made.play(view([1], [2]));
        made.play(view([1, 2]), { beats: [[land(2)]] });
        reach(made.clock, 400, tick);
        // The flight is over at 400 ms; the flash is from 450 to 650.
        if (made.clock.now === 400) assert.deepEqual(made.effect(2), []);
        reach(made.clock, 500, tick);
        if (made.clock.now === 500) {
            assert.deepEqual(made.effect(2), [
                { name: "settle", layer: "over", progress: 0.25 },
            ]);
            assert.equal(surface.effects.get("2 settle over"), 0.25);
        }
        reach(made.clock, 650, tick);
        assert.deepEqual(made.effect(2), []);
        assert.equal(surface.effects.size, 0);
    });

    test(`an effect with no time of its own lasts as long as its step: ticks of ${tick} ms`, () => {
        const { stage: made, surface } = stage(TIMES, EFFECTS);
        made.play(view([1, 2]));
        made.play(view([1]), {
            beats: [[destroy(2, object(2, "graveyard"))], [token(3)]],
        });
        // A card that leaves is gone as its step ends, and has broken up by then.
        const gone = 600;
        reach(made.clock, gone / 2, tick);
        if (made.clock.now === gone / 2) {
            assert.deepEqual(made.effect(2), [
                { name: "dissolve", layer: "over", progress: 0.5 },
            ]);
        }
        reach(made.clock, gone, tick);
        assert.deepEqual(made.effect(2), []);
        assert.equal(surface.effects.has("2 dissolve over"), false);
    });
}

test("an effect goes with its node", () => {
    const { stage: made, surface } = stage(TIMES, EFFECTS);
    made.play(view([1]));
    // A token appears and is destroyed while it still glows: it ceases to exist.
    made.play(view([1]), { beats: [[token(2)], [destroy(2)]] });
    made.clock.advance(150);
    assert.equal(made.effect(2).length, 2, "it glows as it breaks up");
    made.clock.advance(550);
    assert.equal(made.pose(2), null);
    assert.deepEqual(made.effect(2), []);
    // The slot's next token begins with nothing of the last one's.
    made.play(view([1, 2]), { beats: [[token(2)]] });
    made.clock.advance(0);
    assert.deepEqual(made.effect(2), [
        { name: "glow", layer: "under", progress: 0 },
    ]);
    assert.deepEqual([...surface.effects.keys()], ["2 glow under"]);
});

test("a step that takes no time begins no effect that lasts its step", () => {
    const { stage: made } = stage({}, EFFECTS);
    made.play(view([1, 2]));
    made.play(view([1]), { beats: [[destroy(2, object(2, "graveyard"))]] });
    made.clock.advance(0);
    assert.deepEqual(made.effect(2), []);
});

test("time passed over begins no effect, and a stage that starts over has none", () => {
    const { stage: made, surface } = stage(TIMES, EFFECTS);
    made.play(view([1]));
    made.play(view([1, 2]), { beats: [[token(2)]] });
    made.skip();
    assert.deepEqual(made.effect(2), []);
    made.play(view([1, 2, 3]), { beats: [[token(3)]] });
    made.clock.advance(50);
    assert.equal(made.effect(3).length, 1);
    made.play(view([1, 2, 3]));
    assert.deepEqual(made.effect(3), []);
    assert.equal(surface.effects.size, 0);
});

test("a surface mounted while an effect plays draws it where it is", () => {
    const { stage: made, surface } = stage(TIMES, EFFECTS);
    made.play(view([1]));
    made.play(view([1, 2]), { beats: [[token(2)]] });
    made.clock.advance(150);
    made.unmount();
    surface.effects.clear();
    made.mount(surface);
    assert.deepEqual([...surface.effects], [["2 glow under", 0.5]]);
});

test("a number's effect is on its permanent, or on the player whose life it is", () => {
    const count = (tally) => ({ act: { verb: "count", tally } });
    const lit = (act) =>
        act.verb === "count"
            ? [effect("pulse", "start", { duration: 200 })]
            : [];
    const { stage: made, surface } = stage({ count: { duration: 400 } }, lit);
    made.play(view([1]));
    made.play(view([1]), {
        beats: [
            [
                count({ kind: "life", player: "opponent", life: 18 }),
                count({
                    kind: "counters",
                    counter: "plusOne",
                    object: object(1),
                }),
                count({ kind: "pool", player: "you", mana: {} }),
            ],
        ],
    });
    made.clock.advance(100);
    const half = [{ name: "pulse", layer: "over", progress: 0.5 }];
    assert.deepEqual(made.effect("opponent"), half);
    assert.deepEqual(made.effect(1), half);
    assert.deepEqual(made.effect("you"), [], "mana in a pool is on nothing");
    assert.deepEqual([...surface.effects.keys()].sort(), [
        "1 pulse over",
        "opponent pulse over",
    ]);
    made.clock.advance(100);
    assert.equal(surface.effects.size, 0);
});

test("an effect on what a step strikes begins as the blow lands", () => {
    const strike = (source, to) => ({
        act: { verb: "strike", source, to, amount: 1 },
    });
    // The blow lands 0.4 of the way through; the burst is on what is struck, if the stage
    // holds it.
    const lit = (act) => [
        effect("impact", "start", {
            on: act.to.kind === "object" ? act.to.slot : undefined,
            into: 0.4,
            duration: 100,
        }),
    ];
    const { stage: made } = stage({ strike: { duration: 1000 } }, lit);
    made.play(view([1, 2]));
    made.play(view([1, 2]), {
        beats: [
            [strike(1, { kind: "object", slot: 2 })],
            [strike(1, { kind: "object", slot: 9 })],
        ],
    });
    made.clock.advance(399);
    assert.deepEqual(made.effect(2), []);
    made.clock.advance(51);
    assert.deepEqual(made.effect(2), [
        { name: "impact", layer: "over", progress: 0.5 },
    ]);
    assert.deepEqual(made.effect(1), [], "not on what strikes");
    made.clock.advance(50);
    assert.deepEqual(made.effect(2), []);
    // Nothing is drawn for what the stage does not hold.
    made.clock.advance(950);
    assert.deepEqual(made.effect(9), []);
});
