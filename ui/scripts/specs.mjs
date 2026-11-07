// Screenshots every moment of the specs with headless Playwright WebKit, each a live game from
// the spec server Vite proxies (`by specs --shots`, tools/). Usage:
// node scripts/specs.mjs <base-url> <out-dir> [card-slug...]
// Each moment is shot from both seats, `<moment>.png` and `<moment>-theirs.png`, once its
// actions have played out and the page has been quiet a while,
// as a reviewer would first see it settled; prints each path, and exits non-zero if a page
// logs an error or never shows the game.
import { mkdir } from "node:fs/promises";
import { join } from "node:path";
import { launch, openPage, settle, workers } from "./capture.mjs";

/** How long the page must hear nothing before it counts as settled, and the most to wait. */
const QUIET = 2500;
/** How long the last view shown takes to play out once nothing waits behind it. */
const LAST_VIEW = 2000;
const LONGEST = 30000;

const [base, out, ...only] = process.argv.slice(2);
if (!base || !out) {
    console.error(
        "usage: node scripts/specs.mjs <base-url> <out-dir> [card-slug...]",
    );
    process.exit(2);
}

const response = await fetch(new URL("spec-api/specs", base));
if (!response.ok) {
    console.error(`specs: the spec server answered HTTP ${response.status}`);
    process.exit(1);
}
const specs = (await response.json()).filter(
    (s) => only.length === 0 || only.includes(s.slug),
);
const unknown = only.filter((slug) => !specs.some((s) => s.slug === slug));
if (unknown.length) {
    console.error(`specs: no spec for ${unknown.join(", ")}`);
    process.exit(2);
}

const browser = await launch();
const run = workers();
let failed = false;
const stamp = Date.now();
for (const spec of specs) {
    const dir = join(out, spec.slug);
    await mkdir(dir, { recursive: true });
    for (const moment of spec.moments) {
        for (const view of ["yours", "theirs"]) {
            await run(async () => {
                const { context, page, errors } = await openPage(browser, {
                    width: 1600,
                    height: 900,
                });
                const live = new URL(
                    `spec-api/m/${spec.slug}/${moment.name}/${view}/shot-${stamp}`,
                    base,
                ).href;
                const path = join(
                    dir,
                    view === "yours"
                        ? `${moment.name}.png`
                        : `${moment.name}-theirs.png`,
                );
                try {
                    await page.goto(
                        `${base}?live=${encodeURIComponent(live)}#/game`,
                        { waitUntil: "load" },
                    );
                    await page.waitForSelector("[data-life]", {
                        timeout: LONGEST,
                    });
                    await page.waitForFunction(
                        ([quiet, longest, started]) => {
                            const log = window.berylLive?.log ?? [];
                            const last = log.at(-1)?.at ?? 0;
                            const now = performance.now();
                            const still =
                                now - last > quiet && !window.berylBehind;
                            return still || now - started > longest;
                        },
                        [
                            QUIET,
                            LONGEST,
                            await page.evaluate(() => performance.now()),
                        ],
                        { timeout: LONGEST + 5000, polling: 250 },
                    );
                    await page.waitForTimeout(LAST_VIEW);
                    await settle(page);
                    await page.screenshot({ path });
                    console.log(path);
                } catch (e) {
                    errors.push(String(e));
                } finally {
                    await context.close();
                }
                if (errors.length) {
                    failed = true;
                    console.error(
                        `specs: ${spec.slug}/${moment.name} (${view}): ${errors.join("; ")}`,
                    );
                }
            });
        }
    }
}
await run.drain();
await browser.close();
process.exit(failed ? 1 : 0);
