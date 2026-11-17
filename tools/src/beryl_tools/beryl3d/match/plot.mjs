// Turns the SVG plots `just b3d match motion` writes (plot.py) into PNGs, which an agent can
// look at, in one browser page.
//
// Usage, from ui/ (where Playwright is installed): node <this file> <job.json>
// The job is {plots: [[svg, png], ...]}, the path of each SVG and of the PNG to write.
import { readFile } from "node:fs/promises";
import { join } from "node:path";
import { pathToFileURL } from "node:url";

const scripts = (name) =>
    import(pathToFileURL(join(process.cwd(), "scripts", name)).href);
const { launch } = await scripts("capture.mjs");
const { takeSlot } = await scripts("slots.mjs");

const job = JSON.parse(await readFile(process.argv[2], "utf8"));
const release = await takeSlot();
const browser = await launch();
let failed = false;
try {
    const page = await browser.newPage({ deviceScaleFactor: 2 });
    for (const [svg, png] of job.plots) {
        const markup = await readFile(svg, "utf8");
        const [, width, height] = markup.match(/width="(\d+)" height="(\d+)"/);
        await page.setViewportSize({ width: Number(width), height: Number(height) });
        await page.setContent(
            `<html><body style="margin:0">${markup}</body></html>`,
        );
        await page.screenshot({ path: png });
    }
} catch (e) {
    console.error(e.stack ?? String(e));
    failed = true;
} finally {
    await browser.close();
    release();
}
process.exit(failed ? 1 : 0);
