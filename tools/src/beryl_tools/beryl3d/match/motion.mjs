// The web side of `just b3d match motion` (motion.py): opens a fixture page on Playwright's
// paused clock, steps the clock to each of the times Beryl 3D's frames were taken at, and
// writes the page's elements at each, without their lines of text and with a `track` that
// names the same node in every frame (page.mjs's `records`).
//
// Usage, from ui/ (where Playwright is installed): node <this file> <job.json>
// The job is page.mjs's {base, route, click, fixture, width, height, scale, art_dir} and {times_ms,
// advances, roles, json}. The page opens with the views that arrived before the span's first
// batch, at rest. `advances` is [[offset_ms, count], ...]: at each offset the script calls the
// fixture transport's `advance` (ui/src/ipc.ts) `count` times, which hands the page the next
// views live, as Beryl 3D was handed them. `times_ms` lists the offsets each frame is read at,
// in increasing order, and `json` is where {frames: [{offset_ms, elements}]} is written. Offsets
// are ms after the first advance.
//
// Each step runs the fake clock, which runs the stage's clock and Motion's animation frames,
// and then moves every CSS and Web Animations animation on by the same time: they run on the
// document's timeline, which no fake clock stops (ui/scripts/counts.mjs), so each is paused
// when first seen and its current time is set by hand from then on.
import { readFile, writeFile } from "node:fs/promises";
import { FRAME, records, withPage } from "./page.mjs";

const job = JSON.parse(await readFile(process.argv[2], "utf8"));

/** Moves the page's animations on by `ms` and drains React's posted tasks. It runs in the
 * page. An animation first seen now starts at its beginning; one that reaches its end is
 * finished, so that whatever waits on it (Motion awaits `finished`) goes on. */
async function advance(ms) {
    const drained = () =>
        new Promise((resolve) => {
            const channel = new MessageChannel();
            channel.port1.onmessage = () => resolve();
            channel.port2.postMessage(undefined);
        });
    await drained();
    window.berylMatchAnimations ??= new WeakSet();
    const seen = window.berylMatchAnimations;
    for (const animation of document.getAnimations()) {
        if (animation.playState === "finished") continue;
        if (!seen.has(animation)) {
            seen.add(animation);
            animation.pause();
            animation.currentTime = 0;
            continue;
        }
        if (animation.playState !== "paused") animation.pause();
        const timing = animation.effect?.getComputedTiming();
        const next = Number(animation.currentTime ?? 0) + ms;
        if (timing && Number.isFinite(timing.endTime) && next >= timing.endTime) {
            try {
                animation.finish();
            } catch {
                animation.currentTime = next;
            }
        } else {
            animation.currentTime = next;
        }
    }
    void document.documentElement.getBoundingClientRect();
    await drained();
}

const frames = [];
const failed = await withPage(job, async (page, errors) => {
    // Where the clock stands, in ms after the first advance. It moves a whole frame at a time
    // while a frame fits before the next time, so that the stage sees 60 Hz frames as it does
    // in the app, and by the remainder after that.
    let clock = 0;
    const advances = [...job.advances];
    // Hands the page the views due by now.
    const due = async () => {
        while (advances.length && advances[0][0] <= clock + 1e-6) {
            const [, count] = advances.shift();
            await page.evaluate((n) => {
                for (let i = 0; i < n; i++) window.berylFixture.advance();
            }, count);
        }
    };
    const step = async (ms) => {
        await due();
        await page.clock.runFor(ms);
        await page.evaluate(advance, ms);
        clock += ms;
    };
    await due();
    for (const time of job.times_ms) {
        while (clock + FRAME <= time + 1e-6) await step(FRAME);
        if (time - clock > 1e-6) await step(time - clock);
        const elements = await page.evaluate(records, {
            roles: job.roles,
            only: null,
            light: true,
        });
        frames.push({ offset_ms: time, elements });
    }
    for (const error of errors) console.log(`page error: ${error}`);
});
if (!failed) await writeFile(job.json, `${JSON.stringify({ frames })}\n`);
process.exit(failed ? 1 : 0);
