// The stage plays the timing the session sends with each step of a script, with no browser
// (`npm test`), and times the steps it makes itself by the session's table of them
// (src/stage/timing.ts). Which timing each step gets is the session's
// (beryl_session's script/timing.rs and its tests).
import assert from "node:assert/strict";
import { test } from "node:test";
import { ScriptStage } from "../src/stage/stage.ts";
import { ownTimings, played } from "../src/stage/timing.ts";
import { move, row, view } from "./stage-table.mjs";

/** A `StepTiming` as the session sends it, with `more` over a step of no wait. */
const sent = (more = {}) => ({
    duration: 0,
    delay: 0,
    stagger: 0,
    curve: "linear",
    with: false,
    after: [],
    leads: false,
    passes: false,
    alone: false,
    opens: false,
    ahead: false,
    room: { kind: "step" },
    phases: [],
    effects: [],
    ...more,
});

/** A stage timed as the app's is, and every step's start and end it tells of. */
function timed() {
    const made = new ScriptStage({
        layout: row,
        timings: (step) => played(step).timing,
        effects: (step) => played(step).effects,
    });
    const told = [];
    made.follow((edge) => told.push([edge.edge, edge.at, edge.step.act.verb]));
    return { made, told };
}

test("each step plays as the session timed it", () => {
    const { made, told } = timed();
    made.play(view([1, 2, 3]));
    made.play(view([2, 3, 1]), {
        beats: [[move(1, 2), move(2, 0)]],
        timing: [
            sent({ duration: 400, stagger: 100, curve: "easeOut" }),
            sent({ duration: 200, delay: 50 }),
        ],
    });
    made.settle();
    assert.deepEqual(told, [
        ["start", 0, "fly"],
        ["start", 150, "fly"],
        ["end", 350, "fly"],
        ["end", 400, "fly"],
    ]);
});

test("the effects a step begins are the session's, drawn in the data's layer", () => {
    const { made } = timed();
    made.play(view([1]));
    let begun;
    made.effects = (s) => (begun = played(s).effects);
    made.play(view([1]), {
        beats: [[move(1, 0)]],
        timing: [
            sent({
                duration: 100,
                effects: [
                    {
                        name: "flash",
                        when: "end",
                        on: { kind: "player", player: "opponent" },
                        delay: 0,
                        curve: "easeOut",
                    },
                ],
            }),
        ],
    });
    made.settle();
    assert.equal(begun.length, 1);
    assert.deepEqual(
        [begun[0].name, begun[0].when, begun[0].on, begun[0].layer],
        ["flash", "end", "opponent", "over"],
    );
});

test("the stage's own steps play as the session's table says, an arrow after its ends", () => {
    const step = { kind: "arrowBegins", arrow: "attack" };
    ownTimings({
        entries: [
            {
                step,
                timing: sent({ duration: 66, case: "attack line", with: true }),
                afterEnds: true,
            },
        ],
    });
    const arrow = {
        act: {
            verb: "point",
            span: "begin",
            arrow: "attack",
            from: { kind: "object", slot: 1 },
            to: { kind: "player", player: "opponent" },
        },
    };
    const { timing, case: found } = played(arrow);
    assert.equal(found, "attack line");
    assert.equal(timing.duration, 66);
    // A player is no end that moves.
    assert.deepEqual(timing.after, [1]);
    // A shape the table does not have is over as it begins.
    const block = { act: { ...arrow.act, arrow: "block" } };
    assert.equal(played(block).timing.duration, 0);
    ownTimings({ entries: [] });
});
