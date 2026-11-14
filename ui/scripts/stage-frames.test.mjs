// The app's own frame timing (src/stage/frames.ts), with no browser (`npm test`): which frames
// of a run count as dropped, which verbs they are charged to, and that a run is reported once
// the stage is idle and only if it dropped any.
import assert from "node:assert/strict";
import { test } from "node:test";
import { timeFrames } from "../src/stage/frames.ts";

/** A stage that tells its edges when asked to and is idle when told, and frames that come
 * when called for, at the times given. */
function rig() {
    let follower;
    let idle;
    const stage = {
        follow: (f) => {
            follower = f;
            return () => {};
        },
        idle: () => new Promise((resolve) => (idle = resolve)),
    };
    let waiting = [];
    const frames = {
        request: (f) => waiting.push(f),
        cancel: () => {},
    };
    const step = (verb) => ({ act: { verb } });
    return {
        stage,
        frames,
        step,
        edge: (edge, s) => follower({ edge, step: s }),
        frame: (time) => {
            const due = waiting;
            waiting = [];
            for (const f of due) f(time);
        },
        waiting: () => waiting.length,
        idle: async () => {
            idle();
            await Promise.resolve();
        },
    };
}

test("charges a late frame to every verb playing, and reports the run once idle", async () => {
    const r = rig();
    const reports = [];
    timeFrames(r.stage, r.frames, (run) => reports.push(run));
    const [strike, count] = [r.step("strike"), r.step("count")];
    r.edge("start", strike);
    r.frame(0);
    r.frame(16);
    r.edge("start", count);
    r.frame(66);
    r.edge("end", strike);
    r.frame(82);
    r.edge("end", count);
    r.frame(98);
    assert.equal(
        reports.length,
        0,
        "nothing is reported while the stage plays",
    );
    await r.idle();
    assert.deepEqual(reports, [
        {
            frames: 4,
            dropped: 1,
            worst: 50,
            verbs: {
                strike: { frames: 2, dropped: 1, late: 33 },
                count: { frames: 2, dropped: 1, late: 33 },
                "(none)": { frames: 1, dropped: 0, late: 0 },
            },
        },
    ]);
});

test("reports no run that dropped no frame, nor a page that was hidden", async () => {
    const r = rig();
    const reports = [];
    timeFrames(r.stage, r.frames, (run) => reports.push(run));
    r.edge("start", r.step("tap"));
    r.frame(0);
    r.frame(16);
    r.frame(5016);
    r.frame(5032);
    await r.idle();
    assert.deepEqual(reports, []);
});

test("one run follows another on one frame loop", async () => {
    const r = rig();
    const reports = [];
    timeFrames(r.stage, r.frames, (run) => reports.push(run));
    r.edge("start", r.step("tap"));
    r.frame(0);
    await r.idle();
    r.edge("start", r.step("fly"));
    assert.equal(
        r.waiting(),
        1,
        "the new run waits on the frame already asked for",
    );
    r.frame(16);
    r.frame(60);
    await r.idle();
    assert.equal(reports.length, 1);
    assert.deepEqual(Object.keys(reports[0].verbs), ["fly"]);
    assert.equal(reports[0].frames, 1);
});
