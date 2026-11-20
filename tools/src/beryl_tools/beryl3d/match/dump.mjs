// The web side of `just b3d match snapshot` (snapshot.py): opens a fixture page in the app's
// browser engine on Playwright's paused clock, runs the clock to the moment Beryl 3D shows, and
// writes the page's screenshot and its elements as the records `web.json` holds.
//
// Usage, from ui/ (where Playwright is installed): node <this file> <job.json>
// The job is {base, route, click, fixture, width, height, scale, offset_ms, png, json, roles, target,
// art_dir, only}: `fixture` is {name, file}, the file that answers the page's request for
// /fixtures/<name>.json; `offset_ms` is how long after the fixture's last view arrived to stop
// the clock, or null for the view at rest; `advances` is [[offset_ms, count], ...], the
// fixture transport's `advance` called `count` times at each offset, which hands the page the
// views Beryl 3D played live (clock.py), with the clock run a 60 Hz frame at a time so that the
// stage sees frames as it does in the app; `roles` is data/beryl3d/match/roles.json's `roles`;
// `only` is the index of the element to draw alone (`--only`), in the records this script
// writes, or null for the whole page.
//
// page.mjs opens the page and reads its records.
import { readFile, writeFile } from "node:fs/promises";
import { FRAME, MOST, fontFallbacks, records, withPage } from "./page.mjs";

/** Frames in a row the stage must be idle for the view to count as at rest. */
const TAIL = 30;

const job = JSON.parse(await readFile(process.argv[2], "utf8"));

/** Finishes the page's CSS animations and transitions and drains React's posted tasks, as
 * counts.mjs's `settle` does; whether the stage is still playing. It runs in the page. */
async function settle() {
    for (let round = 0; round < 8; round++) {
        void document.documentElement.getBoundingClientRect();
        const running = document
            .getAnimations()
            .filter((animation) => animation.playState === "running");
        if (running.length === 0) break;
        for (const animation of running) {
            try {
                animation.finish();
            } catch {
                animation.pause();
            }
        }
    }
    await new Promise((resolve) => {
        const channel = new MessageChannel();
        channel.port1.onmessage = () => resolve();
        channel.port2.postMessage(undefined);
    });
    return Boolean(window.berylStage?.shown.busy);
}

/** The style that leaves only the marked node and what is inside it on the page (`--only`):
 * everything else is hidden without changing the layout, and the page's background is black,
 * as Beryl 3D's window is where it draws nothing. The node is made visible again, so that the
 * nodes inside it inherit its visibility and keep their own rules. */
const ONLY = `
body *:not([data-b3d-only], [data-b3d-only] *) { visibility: hidden !important; }
[data-b3d-only] { visibility: visible !important; }
html, body { background: #000 !important; }
`;

const failed = await withPage(job, async (page, errors) => {
    if (job.offset_ms === null) {
        let quiet = 0;
        for (let played = 0; quiet < TAIL && played < MOST; played++) {
            await page.clock.runFor(FRAME);
            quiet = (await page.evaluate(settle)) ? 0 : quiet + 1;
        }
    } else {
        let clock = 0;
        const advances = [...(job.advances ?? [])];
        const due = async () => {
            while (advances.length && advances[0][0] <= clock + 1e-6) {
                const [, count] = advances.shift();
                await page.evaluate((n) => {
                    for (let i = 0; i < n; i++) window.berylFixture.advance();
                }, count);
            }
        };
        const offset = Math.max(0, job.offset_ms);
        await due();
        while (clock + FRAME <= offset + 1e-6) {
            await page.clock.runFor(FRAME);
            clock += FRAME;
            await due();
        }
        if (offset - clock > 1e-6) await page.clock.runFor(offset - clock);
    }
    // The records are read before anything is hidden, since a hidden node is not kept.
    let elements;
    if (job.only === null || job.only === undefined) {
        await page.screenshot({ path: job.png });
        elements = await page.evaluate(records, { roles: job.roles, only: null });
    } else {
        elements = await page.evaluate(records, { roles: job.roles, only: job.only });
        await page.addStyleTag({ content: ONLY });
        await page.screenshot({ path: job.png });
    }
    // snapshot.py puts each line printed here in report.txt as "warning: web <line>".
    for (const line of await page.evaluate(fontFallbacks)) console.log(line);
    await writeFile(
        job.json,
        `${JSON.stringify({ target: job.target, scale: job.scale, elements }, null, 1)}\n`,
    );
    for (const error of errors) console.log(`page error: ${error}`);
});
process.exit(failed ? 1 : 0);
