// The web side of `just b3d match samples` (samples.py): shoots each conformance sample's page in
// the app's browser engine at the job's scale, in one browser page.
//
// Usage, from ui/ (where Playwright is installed): node <this file> <job.json>
// The job is {scale, shots: [{html, width, height, png}]}: each shot's page is `html`, shot in a
// viewport of `width` by `height` points into `png`.
//
// Node resolves a module from the file that imports it, and this file is outside ui/, so the
// capture helpers (and through them Playwright) are imported from ui/scripts by path.
import { readFile } from "node:fs/promises";
import { join } from "node:path";
import { pathToFileURL } from "node:url";

const scripts = (name) =>
    import(pathToFileURL(join(process.cwd(), "scripts", name)).href);
const { launch } = await scripts("capture.mjs");
const { hangContext } = await scripts("hang.mjs");
const { takeSlot } = await scripts("slots.mjs");

const job = JSON.parse(await readFile(process.argv[2], "utf8"));

const release = await takeSlot();
const browser = await launch();
try {
    const context = await hangContext(browser, {
        viewport: { width: 64, height: 64 },
        deviceScaleFactor: job.scale,
    });
    const page = await context.newPage();
    for (const shot of job.shots) {
        await page.setViewportSize({ width: shot.width, height: shot.height });
        await page.setContent(shot.html, { waitUntil: "load" });
        await page.evaluate(() => document.fonts.ready);
        await page.screenshot({ path: shot.png });
    }
    await context.close();
} finally {
    await browser.close();
    release();
}
