// Captures UI states with headless Playwright WebKit; `by ui-shots` (tools/) serves the UI and
// runs this. Usage: node scripts/shots.mjs <base-url> <out-dir> <shots.json> [name...]
// Prints each screenshot's path; exits non-zero if a page logs an error or never settles.
import { readFile } from "node:fs/promises";
import { join } from "node:path";
import { webkit } from "playwright";

const [base, out, list, ...only] = process.argv.slice(2);
if (!base || !out || !list) {
    console.error(
        "usage: node scripts/shots.mjs <base-url> <out-dir> <shots.json> [name...]",
    );
    process.exit(2);
}

const { shots } = JSON.parse(await readFile(list, "utf8"));
const chosen = only.length ? shots.filter((s) => only.includes(s.name)) : shots;
const unknown = only.filter((name) => !shots.some((s) => s.name === name));
if (unknown.length) {
    console.error(`ui-shots: no shot named ${unknown.join(", ")}`);
    process.exit(2);
}

const browser = await webkit.launch();
let failed = false;
try {
    for (const shot of chosen) {
        const context = await browser.newContext({
            viewport: { width: shot.width, height: shot.height },
            deviceScaleFactor: shot.scale ?? 1,
            reducedMotion: "reduce",
        });
        const page = await context.newPage();
        const errors = [];
        page.on("pageerror", (e) => errors.push(e.message));
        page.on(
            "console",
            (m) => m.type() === "error" && errors.push(m.text()),
        );
        await page.goto(new URL(shot.route, base).href, { waitUntil: "load" });
        // Settled: fonts loaded, finite animations done and two frames drawn after them, so measured
        // layout (arrows) is final.
        await page.evaluate(async () => {
            await document.fonts.ready;
            const finite = document
                .getAnimations()
                .filter(
                    (a) =>
                        a.effect?.getComputedTiming().iterations !== Infinity,
                );
            await Promise.all(
                finite.map((a) => a.finished.catch(() => undefined)),
            );
            await new Promise((r) =>
                requestAnimationFrame(() => requestAnimationFrame(r)),
            );
        });
        const path = join(out, `${shot.name}.png`);
        await page.screenshot({ path, fullPage: true });
        if (errors.length) {
            failed = true;
            console.error(`ui-shots: ${shot.name}: ${errors.join("\n  ")}`);
        }
        console.log(path);
        await context.close();
    }
} finally {
    await browser.close();
}
process.exit(failed ? 1 : 0);
