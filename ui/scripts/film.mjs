// Films each moment of the specs from both seats (`by specs --film`, tools/): plays the two
// seats' games in step with motion on, screenshots both every INTERVAL ms from the play to
// the settle, and lays out the bursts of motion as one sheet per moment, at most MOST frames,
// each frame your seat over theirs, labelled with its time. One image shows how the moment
// moves for both players, where a still shows only how it ends. Usage:
//   node scripts/film.mjs <base-url> <out-dir> [card-slug...] [--moment NAME] [--viewport WxH]
// Writes <out-dir>/<card>/<moment>.png, and every frame under <out-dir>/<card>/<moment>/,
// and prints each sheet's path. Under the stage (`by specs --stage`) the two seats' stages are
// held and stepped INTERVAL ms of stage time a frame: a frame's time is the stage's, whatever
// the machine's speed.
import { mkdir, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { launch, workers } from "./capture.mjs";
import {
    chosen,
    LAST_VIEW,
    LONGEST,
    moving,
    openSeat,
    option,
    play,
    QUIET,
    rest,
    viewport,
    warm,
} from "./moments.mjs";

/** How often both seats are shot, the most frames a sheet keeps, and how many a row. */
const INTERVAL = 120;
const MOST = 8;
const ACROSS = 4;
/** Each frame's width on the sheet. */
const TILE = 400;
/** How much of a downscaled frame's pixels must change from the frame before for it to
 * count as motion. */
const DIFFERENT = 0.004;

const args = process.argv.slice(2);
const moment = option(args, "--moment");
const size = viewport(option(args, "--viewport"));
const [base, out, ...only] = args;
if (!base || !out) {
    console.error(
        "usage: node scripts/film.mjs <base-url> <out-dir> [card-slug...] [--moment NAME]",
    );
    process.exit(2);
}

/** Shoots both pages every INTERVAL ms until both have been quiet (no view heard, none
 * queued) for QUIET ms and LAST_VIEW more, or LONGEST. Each frame: its ms since the play and
 * both seats' JPEGs. */
async function film(pages, lives) {
    const staged = await Promise.all(
        pages.map((p) => p.evaluate(() => window.berylStage !== undefined)),
    );
    if (staged.every(Boolean)) return stepped(pages, lives);
    const frames = [];
    const quietSince = pages.map(() => undefined);
    const start = Date.now();
    await play(...lives);
    while (Date.now() - start < LONGEST) {
        const t = Date.now() - start;
        const shots = await Promise.all(
            pages.map((p) => p.screenshot({ type: "jpeg", quality: 70 })),
        );
        frames.push({ t, shots });
        const quiet = await Promise.all(
            pages.map((p) =>
                p.evaluate(() => {
                    const last = window.berylLive?.log.at(-1)?.at ?? 0;
                    return performance.now() - last > 0 && !window.berylBehind
                        ? performance.now() - last
                        : 0;
                }),
            ),
        );
        quiet.forEach((q, i) => {
            quietSince[i] =
                q >= QUIET ? (quietSince[i] ?? Date.now()) : undefined;
        });
        if (
            quietSince.every(
                (q) => q !== undefined && Date.now() - q > LAST_VIEW,
            )
        ) {
            break;
        }
        await new Promise((r) =>
            setTimeout(r, Math.max(0, INTERVAL - (Date.now() - start - t))),
        );
    }
    return frames;
}

/** `film` on the stage: holds both seats' clocks, plays, and once neither page is owed a view
 * (moments.mjs `rest`) shoots both and steps each stage still playing INTERVAL ms on, until
 * both are at rest. Each frame: the stage ms since the play, and both seats' JPEGs. */
async function stepped(pages, lives) {
    const each = (run, ...given) =>
        Promise.all(pages.map((p) => p.evaluate(run, ...given)));
    await each(() => window.berylStage.clock.hold());
    await play(...lives);
    await Promise.all(
        pages.map((p) =>
            p.waitForFunction(`!(${rest})().owed`, undefined, {
                timeout: LONGEST,
            }),
        ),
    );
    const frames = [];
    for (let t = 0; t < LONGEST; t += INTERVAL) {
        const shots = await Promise.all(
            pages.map((p) => p.screenshot({ type: "jpeg", quality: 70 })),
        );
        frames.push({ t, shots });
        if ((await each(`(${rest})()`)).every((at) => at.settled)) break;
        await each((ms) => {
            const { clock, shown } = window.berylStage;
            if (shown.busy) clock.advance(ms);
        }, INTERVAL);
    }
    await each(() => window.berylStage.clock.release());
    return frames;
}

/** Draws the sheet of `frames` in `page`: the first and last frames and, for the bursts of
 * motion that moved most, the frame before each, its middle and its end, at most MOST in all.
 * Returns how many it shows. */
async function sheet(page, title, frames) {
    const data = frames.map((f) => ({
        t: f.t,
        srcs: f.shots.map(
            (b) => `data:image/jpeg;base64,${b.toString("base64")}`,
        ),
    }));
    await page.setContent(
        `<body style="margin:0;background:#2b2b2b;color:#ddd;font:13px -apple-system,sans-serif"></body>`,
    );
    return page.evaluate(
        async ({ title, data, most, across, tile, aspect, different }) => {
            const load = (src) =>
                new Promise((ok) => {
                    const i = new Image();
                    i.onload = () => ok(i);
                    i.src = src;
                });
            const small = document.createElement("canvas");
            small.width = 160;
            small.height = 90 * 2;
            const g = small.getContext("2d", { willReadFrequently: true });
            const pixels = async (srcs) => {
                const imgs = await Promise.all(srcs.map(load));
                imgs.forEach((im, k) => g.drawImage(im, 0, 90 * k, 160, 90));
                return g.getImageData(0, 0, 160, 180).data;
            };
            // How much each frame changed from the one before it.
            const change = [0];
            let last = await pixels(data[0].srcs);
            for (let i = 1; i < data.length; i++) {
                const p = await pixels(data[i].srcs);
                let changed = 0;
                for (let k = 0; k < p.length; k += 4) {
                    if (
                        Math.abs(p[k] - last[k]) +
                            Math.abs(p[k + 1] - last[k + 1]) +
                            Math.abs(p[k + 2] - last[k + 2]) >
                        48
                    ) {
                        changed += 1;
                    }
                }
                change.push(changed / (p.length / 4));
                last = p;
            }
            // Bursts of motion, each shown by the frame before it, its middle and its end,
            // the bursts that moved most first; then the first and last frames.
            const bursts = [];
            for (let i = 1; i < data.length; i++) {
                if (change[i] <= different) continue;
                const open = bursts.at(-1);
                if (open && open.end === i - 1) {
                    open.end = i;
                    open.moved += change[i];
                } else {
                    bursts.push({ start: i, end: i, moved: change[i] });
                }
            }
            const picked = new Set([0, data.length - 1]);
            for (const b of [...bursts].sort((x, y) => y.moved - x.moved)) {
                const frames = [
                    b.start - 1,
                    Math.round((b.start + b.end) / 2),
                    b.end,
                ];
                const fresh = frames.filter((i) => !picked.has(i));
                if (picked.size + fresh.length > most) {
                    if (picked.size + 1 <= most) picked.add(frames[1]);
                    continue;
                }
                fresh.forEach((i) => picked.add(i));
            }
            const shown = [...picked].sort((x, y) => x - y);
            const h = Math.round(tile * aspect);
            document.body.innerHTML = `<div style="padding:10px 12px;font-size:15px">${title}  <span style="opacity:.6">top: you · bottom: them</span></div><div style="display:grid;grid-template-columns:repeat(${across},${tile}px);gap:10px;padding:0 12px 12px">${shown
                .map(
                    (i) =>
                        `<figure style="margin:0"><figcaption style="margin-bottom:3px">${data[i].t} ms</figcaption>${data[
                            i
                        ].srcs
                            .map(
                                (s) =>
                                    `<img src="${s}" style="display:block;width:${tile}px;height:${h}px;margin-bottom:2px">`,
                            )
                            .join("")}</figure>`,
                )
                .join("")}</div>`;
            await Promise.all([...document.images].map((i) => i.decode()));
            return shown.length;
        },
        {
            title,
            data,
            most: MOST,
            across: ACROSS,
            tile: TILE,
            aspect: size.height / size.width,
            different: DIFFERENT,
        },
    );
}

let specs;
try {
    specs = await chosen(base, only, moment);
} catch (e) {
    console.error(`film: ${e.message}`);
    process.exit(2);
}
const browser = await launch();
await warm(browser, base);
// Each task drives two pages and shoots them as fast as it can: fewer at once.
const run = workers();
let failed = false;
for (const spec of specs) {
    for (const m of spec.moments) {
        await run(async () => {
            const seats = await Promise.all([
                moving(browser, size),
                moving(browser, size),
            ]);
            const errors = seats.flatMap((s) => s.errors);
            const dir = join(out, spec.slug);
            const path = join(dir, `${m.name}.png`);
            try {
                const lives = await Promise.all(
                    ["yours", "theirs"].map((seat, i) =>
                        openSeat(seats[i].page, base, spec.slug, m.name, seat),
                    ),
                );
                const frames = await film(
                    seats.map((s) => s.page),
                    lives,
                );
                await mkdir(join(dir, m.name), { recursive: true });
                await Promise.all(
                    frames.flatMap((f) =>
                        f.shots.map((b, i) =>
                            writeFile(
                                join(
                                    dir,
                                    m.name,
                                    `${String(f.t).padStart(5, "0")}-${i ? "theirs" : "yours"}.jpg`,
                                ),
                                b,
                            ),
                        ),
                    ),
                );
                const page = await seats[0].context.newPage();
                // As wide as the sheet, whatever size the seats were filmed at.
                await page.setViewportSize({
                    width: ACROSS * (TILE + 10) + 24,
                    height: 900,
                });
                const kept = await sheet(
                    page,
                    `${spec.slug} / ${m.name} · ${m.title}`,
                    frames,
                );
                await page.locator("body").screenshot({ path });
                console.log(`${path}  (${kept} of ${frames.length} frames)`);
            } catch (e) {
                errors.push(String(e).split("\n")[0]);
            } finally {
                await Promise.all(seats.map((s) => s.context.close()));
            }
            if (errors.length) {
                failed = true;
                console.error(
                    `film: ${spec.slug}/${m.name}: ${errors.join("; ")}`,
                );
            }
        });
    }
}
await run.drain();
await browser.close();
process.exit(failed ? 1 : 0);
