// Screenshots every moment of the specs with headless Playwright WebKit, each a live game from
// the spec server Vite proxies (`by specs --shots`, tools/). Usage:
//   node scripts/specs.mjs <base-url> <out-dir> [card-slug...] [--moment NAME]
//     [--viewport WxH] [--select SELECTOR] [--crop SELECTOR|X,Y,W,H]
// Each moment is shot from both seats, `<card>/<moment>.png` and `<card>/<moment>-theirs.png`,
// once its actions have played out and the page has been quiet a while, as a reviewer would
// first see it settled; prints each path, and exits non-zero if a page logs an error or never
// shows the game. `--select` keeps only the seats where an element it selects shows (a
// prompt: `.prompt-title`); `--crop` shoots only the box of the first element it selects
// (with a margin), or a fixed region. A seat that fails is tried again once.
import { mkdir } from "node:fs/promises";
import { join } from "node:path";
import { launch, openPage, settle, workers } from "./capture.mjs";
import { chosen, option, retried, viewport, warm } from "./moments.mjs";

/** How long the page must hear nothing before it counts as settled, and the most to wait. */
const QUIET = 2500;
/** How long the last view shown takes to play out once nothing waits behind it. */
const LAST_VIEW = 2000;
const LONGEST = 30000;
/** The margin around a cropped element. */
const MARGIN = 24;

const args = process.argv.slice(2);
const moment = option(args, "--moment");
const size = viewport(option(args, "--viewport"));
const select = option(args, "--select");
const crop = option(args, "--crop");
const [base, out, ...only] = args;
if (!base || !out) {
    console.error(
        "usage: node scripts/specs.mjs <base-url> <out-dir> [card-slug...] [--moment NAME] [--viewport WxH] [--select SELECTOR] [--crop SELECTOR|X,Y,W,H]",
    );
    process.exit(2);
}
const region =
    crop && /^\d+,\d+,\d+,\d+$/.test(crop) ? crop.split(",").map(Number) : null;

let specs;
try {
    specs = await chosen(base, only, moment);
} catch (e) {
    console.error(`specs: ${e.message}`);
    process.exit(2);
}

/** The clip `--crop` asks for on `page`, or undefined for the whole page. Throws if its
 * selector shows nothing. */
async function clip(page) {
    if (!crop) return undefined;
    if (region) {
        const [x, y, width, height] = region;
        return { x, y, width, height };
    }
    const box = await page.locator(crop).first().boundingBox();
    if (!box) throw new Error(`nothing shows ${crop} to crop to`);
    const x = Math.max(0, box.x - MARGIN);
    const y = Math.max(0, box.y - MARGIN);
    return {
        x,
        y,
        width: Math.min(size.width - x, box.width + 2 * MARGIN),
        height: Math.min(size.height - y, box.height + 2 * MARGIN),
    };
}

const browser = await launch();
await warm(browser, base);
const run = workers();
let failed = false;
let skipped = 0;
const stamp = Date.now();
for (const spec of specs) {
    const dir = join(out, spec.slug);
    await mkdir(dir, { recursive: true });
    for (const m of spec.moments) {
        for (const view of ["yours", "theirs"]) {
            await run(async () => {
                let errors = [];
                const path = join(
                    dir,
                    view === "yours" ? `${m.name}.png` : `${m.name}-theirs.png`,
                );
                await retried(async (n) => {
                    const opened = await openPage(browser, size);
                    const { context, page } = opened;
                    errors = opened.errors;
                    const live = new URL(
                        `spec-api/m/${spec.slug}/${m.name}/${view}/shot-${stamp}-${n}`,
                        base,
                    ).href;
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
                        if (
                            select &&
                            !(await page.locator(select).first().isVisible())
                        ) {
                            skipped += 1;
                            return;
                        }
                        await page.screenshot({ path, clip: await clip(page) });
                        console.log(path);
                    } catch (e) {
                        errors.push(String(e).split("\n")[0]);
                    } finally {
                        await context.close();
                    }
                    if (errors.length) throw new Error(errors.join("; "));
                }, 2).catch(() => undefined);
                if (errors.length) {
                    failed = true;
                    console.error(
                        `specs: ${spec.slug}/${m.name} (${view}): ${errors.join("; ")}`,
                    );
                }
            });
        }
    }
}
await run.drain();
await browser.close();
if (select) console.log(`specs: ${skipped} seats show no ${select}`);
process.exit(failed ? 1 : 0);
