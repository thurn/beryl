// Screenshots every moment of the specs with headless Playwright WebKit, each a live game from
// the spec server Vite proxies (`by specs --shots`, tools/). Usage:
//   node scripts/specs.mjs <base-url> <out-dir> [card-slug...] [--moment NAME]
//     [--viewport WxH] [--select SELECTOR] [--crop SELECTOR|X,Y,W,H]
// Each moment is shot from both seats, `<card>/<moment>.png` and `<card>/<moment>-theirs.png`,
// once its actions have played out and its stage is at rest, as a reviewer would first see it
// settled; prints each path, and exits non-zero if a page logs an error or never shows the
// game, or if a seat's screen fails the audits `by ui-shots` holds a fixture's to. `--select` keeps only the seats where an element it selects shows (a
// prompt: `.prompt-title`); `--crop` shoots only the box of the first element it selects
// (with a margin), or a fixed region. A seat that fails is tried again once.
import { mkdir } from "node:fs/promises";
import { join } from "node:path";
import {
    clippedText,
    crowdedControls,
    launch,
    openPage,
    phoneFaults,
    screenFaults,
    settle,
    stageFaults,
    workers,
} from "./capture.mjs";
import { toldErrors } from "./told.mjs";
import {
    chosen,
    option,
    play,
    rest,
    retried,
    viewport,
    warm,
    LAYOUT,
    HANG_MS,
} from "./moments.mjs";

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

/** What the screen audits find on a settled seat's `page` (capture.mjs), as `by ui-shots`
 * holds a fixture's screen to them: the screen is held to the view its stage shows. */
async function faults(page) {
    const found = [
        ...(await toldErrors(page)).map(
            (e) => `the screen tells of an error: ${e}`,
        ),
        ...(await clippedText(page)).map((cut) => `text cut: ${cut}`),
        ...(await screenFaults(page)).map((fault) => `screen: ${fault}`),
        ...(await crowdedControls(page)).map((fault) => `controls: ${fault}`),
        ...(await stageFaults(page)).map((fault) => `stage: ${fault}`),
    ];
    if (process.env.BERYL_PHONE) {
        found.push(
            ...(await phoneFaults(page)).map((fault) => `phone: ${fault}`),
        );
    }
    return found;
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
                            `${base}?live=${encodeURIComponent(live)}&spec${LAYOUT}#/game`,
                            { waitUntil: "load" },
                        );
                        await page.waitForSelector("[data-life]", {
                            timeout: HANG_MS,
                        });
                        await play(live);
                        // The page is settled when the stage is at rest.
                        await page.waitForFunction(
                            `(${rest})().settled`,
                            undefined,
                            { timeout: HANG_MS },
                        );
                        await settle(page);
                        if (
                            select &&
                            !(await page.locator(select).first().isVisible())
                        ) {
                            skipped += 1;
                            return;
                        }
                        await page.screenshot({ path, clip: await clip(page) });
                        errors.push(...(await faults(page)));
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
