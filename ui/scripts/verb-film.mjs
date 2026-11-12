// Films a verb for the independent motion review (`by motion --film`, tools' motion_film.py;
// UI Conventions, "Motion is Beryl's own"): the verb's moments of the verb suite
// (ui/verbs.json) played on the stage in stepped stage time, from both seats in both layouts,
// and each take of the verb laid out as sheets a reviewer can judge its motion from. Usage:
//   node scripts/verb-film.mjs <base-url> <fixtures-dir> <out-dir> <verb>
//       [--case NAME] [--all] [--arena <manifest.json>]
// A take is a run of the verb's steps of one case that play together: one card's flight, or
// a batch. The first take of each case is filmed for each layout and seat, or with `--all`
// every take; `--case` keeps one case. Each moment is played twice on a page of its own, as
// stepped.mjs plays it: once to learn when each step plays and where everything goes, and once
// to shoot it. For each take it writes `<out-dir>/<layout>/<seat>/<moment>-<n>-<case>/`:
//   table.png  the whole table before the take, as it begins, a third and two thirds through,
//              as it ends and after: who did what, where it went, what held still;
//   strip.png  30 frames a second of stage time, framed on everything that moves meanwhile
//              (strip-2.png and on when one sheet cannot hold them);
//   ticks.png  every tick of the 300 ms in which its subject changes most, framed on it;
//   track/     every tick of the take framed on its subject, named by stage time in ms as `by
//              video measure` reads a folder;
// and `<out-dir>/takes.json`, every take with its steps, times, sheets and its subjects' poses
// tick by tick. With `--arena`, a manifest of Arena's instances of the verb as frames
// (motion_film.py writes it), it also writes `<out-dir>/contact/<case>-<k>-<seat>.png`: an
// instance over a seat's take of its case on the desktop, every frame at one scale of the table. A caption is a
// frame's stage time from the take's start, bright while a step of the take plays. CSS
// animations, which run on the page's time, are stopped in every frame: only what the stage's
// clock moves is filmed. Exits non-zero if a moment does not play out, its two plays differ or
// a page logs an error.
import { isDeepStrictEqual } from "node:util";
import { mkdir, readFile, rm, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { launch } from "./capture.mjs";
import { ending } from "./fixture-views.mjs";
import {
    ANSWERS,
    differing,
    EDGE,
    GAP,
    openStage,
    reel,
    TICK,
    walk,
} from "./film-walk.mjs";
import { install as installMarks } from "./written-marks.mjs";

const args = process.argv.slice(2);
/** Takes the flag `name` and its value out of the arguments; its value, or undefined. */
function option(name) {
    const at = args.indexOf(name);
    return at < 0 ? undefined : args.splice(at, 2)[1];
}
const only = option("--case") ?? null;
const arena = option("--arena");
const every = args.includes("--all");
if (every) args.splice(args.indexOf("--all"), 1);
const [base, fixtures, out, verb] = args;
const suite = JSON.parse(
    await readFile(new URL("../verbs.json", import.meta.url), "utf8"),
);
if (!base || !fixtures || !out || !suite.verbs.includes(verb)) {
    console.error(
        "usage: node scripts/verb-film.mjs <base-url> <fixtures-dir> <out-dir> <verb>" +
            ` [--case NAME] [--all] [--arena <manifest.json>]\nverbs: ${suite.verbs.join(", ")}`,
    );
    process.exit(2);
}
const { shots } = JSON.parse(
    await readFile(new URL("../shots.json", import.meta.url), "utf8"),
);
const phone = shots.find((shot) =>
    new URL(shot.route, base).searchParams.has("phone"),
);
/** The layouts filmed, as verbs.mjs opens them: the desktop's window, and the first phone
 * size the shots take. */
const LAYOUTS = {
    desktop: { query: "stage", size: { width: 1600, height: 900 } },
    phone: {
        query: "phone&stage",
        size: { width: phone.width, height: phone.height },
    },
};
/** The moments that play the verb: the suite's first, to where its ability has come and gone,
 * and those the verb watches besides, each to its recording's end. */
const MOMENTS = [
    { name: "cast", seats: suite.moment, whole: false },
    ...(suite.watches[verb] ?? []).map((name) => ({
        name,
        seats: suite.more[name],
        whole: true,
    })),
];
/** Ticks filmed before a take and after it: after, as long as what a finished ability left
 * room for takes to close up (src/stage/tracks.ts CLOSED). */
const LEAD = 6;
const TAIL = 16;
/** The span filmed tick by tick, in stage ms. */
const KEY = 300;
/** Device pixels a CSS pixel: frames sharp enough to read a card in a crop. */
const SCALE = 2;
/** How wide a sheet is at most, and how tall before it is cut into pages, in CSS pixels. */
const WIDE = 1536;
const TALL = 1500;
/** How wide a whole-table frame is drawn on its sheet. */
const TABLE_TILE = 760;
/** How wide an Arena frame is drawn on a contact sheet: the first that keeps the sheet within a
 * page, else the last. */
const ARENA_TILES = [380, 320, 260, 220];
/** The most frames of an Arena instance on a contact sheet. */
const CONTACT = 16;

const slug = (text) =>
    String(text ?? "any")
        .toLowerCase()
        .replace(/[^a-z0-9]+/g, "-")
        .replace(/^-|-$/g, "");
const stamp = (at) => at.toFixed(3).padStart(10, "0");

/** The first tick at or after the stage time `at`. */
const tickAt = (ticks, at) => {
    const found = ticks.findIndex((t) => t.at >= at);
    return found < 0 ? ticks.length - 1 : found;
};

/** The box a subject of `step` has at tick `t` of `film` (a `reel`): its card's or ability's,
 * or a player's life total, for a count of life or a mark on a player; for an arrow, a line
 * or a streak, the box about both the things it is between; and the whole table, `size`, for
 * a step about no thing on it. */
function boxOf(step, film, t, orbs, size) {
    if (step.table) return [0, 0, size.width, size.height];
    const player = step.act.tally?.kind === "life" && step.act.tally.player;
    if (player) return orbs[player] ?? null;
    const of = (name) => orbs[name] ?? film.box(name, t);
    if (step.ends) {
        const ends = step.ends.map(of).filter(Boolean);
        if (!ends.length) return null;
        return [
            Math.min(...ends.map((b) => b[0])),
            Math.min(...ends.map((b) => b[1])),
            Math.max(...ends.map((b) => b[2])),
            Math.max(...ends.map((b) => b[3])),
        ];
    }
    return step.subject === null ? null : of(step.subject);
}

/** The window around `boxes` with `pad` about them, on whole pixels within `size`. */
function around(boxes, pad, size) {
    const x0 = Math.max(
        0,
        Math.floor(Math.min(...boxes.map((b) => b[0])) - pad),
    );
    const y0 = Math.max(
        0,
        Math.floor(Math.min(...boxes.map((b) => b[1])) - pad),
    );
    const x1 = Math.min(
        size.width,
        Math.ceil(Math.max(...boxes.map((b) => b[2])) + pad),
    );
    const y1 = Math.min(
        size.height,
        Math.ceil(Math.max(...boxes.map((b) => b[3])) + pad),
    );
    return { x: x0, y: y0, width: x1 - x0, height: y1 - y0 };
}

/** The verb's takes among a play's steps: runs of its steps of one case, each beginning before
 * the one before it is over. */
function takesOf(steps) {
    const found = [];
    steps.forEach((step, n) => {
        if (step.act.verb !== verb || (only !== null && step.case !== only)) {
            return;
        }
        const open = found.at(-1);
        if (open && open.case === step.case && step.from <= open.to + TICK) {
            open.steps.push({ ...step, n });
            open.to = Math.max(open.to, step.to);
        } else {
            found.push({
                case: step.case,
                from: step.from,
                to: step.to,
                steps: [{ ...step, n }],
            });
        }
    });
    return found;
}

/** How much a take's subjects change from tick `t` of `film` to the next: how far each moves,
 * grows and turns in pixels and degrees, and how much of it comes or goes. */
function changeOf(take, film, t) {
    let change = 0;
    for (const step of take.steps) {
        const name = step.subject;
        const a = name === null ? null : film.pose(name, t);
        const b = name === null ? null : film.pose(name, t + 1);
        if (!a !== !b) change += 100;
        if (!a || !b) continue;
        change +=
            Math.hypot(b.x - a.x, b.y - a.y) +
            Math.abs(b.width - a.width) +
            Math.abs(b.turn - a.turn) +
            Math.abs(b.rotation - a.rotation) +
            100 * Math.abs(b.opacity - a.opacity);
    }
    return change;
}

/** What is shot of `take` in a play of `ticks`, read as `film`: its windows on the page, the
 * ticks of each of its sheets, and where each step's subject comes to rest in its `track`
 * frames. Null when nothing of it is drawn. */
function plan(take, { ticks, orbs }, film, size) {
    const first = tickAt(ticks, take.from);
    const last = tickAt(ticks, take.to);
    const from = Math.max(0, first - LEAD);
    const to = Math.min(ticks.length - 1, last + TAIL);
    const near = [];
    for (
        let t = Math.max(0, first - 1);
        t <= last + 1 && t < ticks.length;
        t++
    ) {
        near.push(t);
    }
    const subjects = take.steps.flatMap((step) =>
        near.map((t) => boxOf(step, film, t, orbs, size)).filter(Boolean),
    );
    if (!subjects.length) return null;
    const pad = 0.3 * Math.max(...subjects.map((b) => b[2] - b[0]));
    const close = around(subjects, pad, size);
    // Everything that moves, comes or goes while the take is filmed: each box it has then.
    const moved = film.names.flatMap((name) => {
        const boxes = [
            film.box(name, from),
            ...film
                .changes(name)
                .filter((t) => from < t && t <= to)
                .map((t) => film.box(name, t)),
        ];
        const still = boxes.every((b) => isDeepStrictEqual(b, boxes[0]));
        return still ? [] : boxes.filter(Boolean);
    });
    let action = around([...subjects, ...moved], 16, size);
    if (action.width * action.height >= 0.6 * size.width * size.height) {
        action = { x: 0, y: 0, ...size };
    }
    // The 300 ms in which the subjects change most, or all of a shorter take.
    const span = Math.min(Math.round(KEY / TICK), last - first);
    let key = first;
    let most = -1;
    for (let start = first; start + span <= last; start++) {
        let change = 0;
        for (let t = start; t < start + span; t++) {
            change += changeOf(take, film, t);
        }
        if (change > most) [key, most] = [start, change];
    }
    const track = [Math.max(0, first - EDGE), Math.min(to, last + EDGE)];
    const through = (share) => first + Math.round((last - first) * share);
    return {
        close,
        action,
        track,
        strip: Array.from({ length: to - from + 1 }, (_, k) => from + k).filter(
            (t) => (t - first) % 2 === 0,
        ),
        key: [Math.max(track[0], key - 2), Math.min(track[1], key + span + 2)],
        table: [
            ...new Set([from, first, through(1 / 3), through(2 / 3), last, to]),
        ],
        rests: take.steps.map((step) => {
            const box =
                boxOf(step, film, tickAt(ticks, step.to), orbs, size) ??
                subjects.at(-1);
            return [
                ((box[0] + box[2]) / 2 - close.x) / close.width,
                ((box[1] + box[3]) / 2 - close.y) / close.height,
            ];
        }),
    };
}

const browser = await launch();
const board = await browser.newPage();
let failed = false;

/** Where a sheet finds the frame saved at `path`: the board's page fetches each from disk, so
 * that a sheet of whole tables shot at twice their size is not sent to it as one string. */
const FRAMES = "http://frames.invalid/";
await board.route(`${FRAMES}**`, (route) =>
    route.fulfill({
        path: decodeURIComponent(new URL(route.request().url()).pathname),
    }),
);
const served = (path) => new URL(path, FRAMES).href;

/** How many frames of `width` by `height` go across a sheet and how wide each is drawn: the
 * fewest across, no frame wider than its device pixels or than `widest`, that keep `count` of
 * them within a sheet's height; and how many rows a page of the sheet holds. */
function arrange(count, width, height, widest = WIDE) {
    const natural = Math.min(width * SCALE, widest);
    for (let across = 1; ; across++) {
        const tile = Math.min(
            natural,
            Math.floor((WIDE - 8 * across) / across),
        );
        const row = (tile * height) / width + 24;
        const fits = Math.ceil(count / across) * row <= TALL;
        if (fits || across >= 12 || tile < Math.min(natural, 150)) {
            return { across, tile, rows: Math.max(1, Math.floor(TALL / row)) };
        }
    }
}

/** Draws `sections` under `title` and saves the sheet to `path`. A section is a `label` over
 * `frames` (a `path`, a `caption`, and `live` for one a step plays in) drawn `tile` wide and
 * `across` a row. */
async function sheet(path, title, sections) {
    const html = [];
    for (const { label, frames, tile, across } of sections) {
        const figures = frames.map(
            (frame) =>
                `<figure><figcaption class="${frame.live ? "live" : ""}">${frame.caption}</figcaption>` +
                `<img src="${served(frame.path)}" style="width:${tile}px"></figure>`,
        );
        html.push(
            `<h2>${label}</h2><div class="grid" style="grid-template-columns:repeat(${across},${tile}px)">${figures.join("")}</div>`,
        );
    }
    await board.setViewportSize({ width: WIDE + 24, height: 900 });
    await board.setContent(
        `<style>body{margin:0;padding:10px 12px;background:#2b2b2b;color:#bbb;font:13px -apple-system,sans-serif;display:inline-block}
        h1{font-size:15px;margin:0 0 6px;color:#eee}h2{font-size:13px;margin:8px 0 4px;color:#ddd;font-weight:600}
        .grid{display:grid;gap:8px}figure{margin:0}figcaption{margin-bottom:2px;opacity:.7}
        figcaption.live{color:#ffd76a;opacity:1;font-weight:600}img{display:block}</style>
        <h1>${title}</h1>${html.join("")}`,
    );
    await board.evaluate(() =>
        Promise.all([...document.images].map((image) => image.decode())),
    );
    await board.locator("body").screenshot({ path });
}

/** Saves `frames`, each `width` by `height` as shot, as one sheet at `path`, or as pages
 * `-2`, `-3` beside it when one cannot hold them; the paths. */
async function pages(path, title, label, frames, { width, height }, widest) {
    const { across, tile, rows } = arrange(
        frames.length,
        width,
        height,
        widest,
    );
    const saved = [];
    for (let at = 0; at < frames.length; at += across * rows) {
        const page = at / (across * rows) + 1;
        const to = page === 1 ? path : path.replace(/\.png$/, `-${page}.png`);
        const part = frames.slice(at, at + across * rows);
        const more = frames.length > across * rows ? `, page ${page}` : "";
        await sheet(to, title, [
            { label: `${label}${more}`, frames: part, tile, across },
        ]);
        saved.push(to);
    }
    return saved;
}

/** Films every take of `chosen`, each with its `shots` planned, in a second play of the
 * moment, which must go as the first, `learned`, did; and lays out each take's sheets. */
async function shoot(folder, chosen, learned, open, play, seat, title) {
    const byTick = new Map();
    const want = (tick, shot) =>
        byTick.set(tick, [...(byTick.get(tick) ?? []), shot]);
    for (const take of chosen) {
        const { shots: planned } = take;
        const at = (tick) => stamp(learned.ticks[tick].at);
        take.frames = { track: [], strip: [], table: [] };
        for (const kind of ["track", "strip", "table"]) {
            await mkdir(join(folder, take.name, kind), { recursive: true });
        }
        const add = (kind, tick, clip, type) => {
            const path = join(
                folder,
                take.name,
                kind,
                `${at(tick)}-${seat}.${type === "png" ? "png" : "jpg"}`,
            );
            take.frames[kind].push({ tick, path });
            want(tick, { path, clip, type });
        };
        for (let t = planned.track[0]; t <= planned.track[1]; t++) {
            add("track", t, planned.close, "png");
        }
        for (const t of planned.strip) add("strip", t, planned.action, "jpeg");
        for (const t of planned.table) add("table", t, undefined, "jpeg");
    }
    const second = await open();
    try {
        await second.page.exposeFunction("berylShoot", async (tick) => {
            for (const { path, clip, type } of byTick.get(tick)) {
                await second.page.screenshot({
                    path,
                    clip,
                    type,
                    ...(type === "jpeg" ? { quality: 85 } : {}),
                    animations: "disabled",
                });
            }
        });
        const shot = await play(second.page, [...byTick.keys()]);
        if (!isDeepStrictEqual(shot, learned)) {
            throw new Error(
                `the stage played the moment otherwise the second time: ${differing(learned, shot)}`,
            );
        }
        if (second.errors.length) throw new Error(second.errors.join("\n"));
    } finally {
        await second.context.close();
    }
    for (const take of chosen) {
        const { shots: planned, frames } = take;
        const caption = ({ tick, path }) => {
            const at = learned.ticks[tick].at;
            const by = Math.round(at - take.from);
            return {
                path,
                caption: `${by < 0 ? "−" : "+"}${Math.abs(by)} ms`,
                live: take.steps.some(
                    (step) => step.from <= at && at <= step.to,
                ),
            };
        };
        const named = `${title} · ${take.name}`;
        const whole = second.size;
        take.sheets = {
            table: await pages(
                join(folder, take.name, "table.png"),
                named,
                "the whole table: before, as it begins, a third and two thirds through, as it ends, after",
                frames.table.map(caption),
                whole,
                TABLE_TILE,
            ),
            strip: await pages(
                join(folder, take.name, "strip.png"),
                named,
                "30 frames a second of stage time, framed on everything that moves",
                frames.strip.map(caption),
                planned.action,
            ),
            ticks: await pages(
                join(folder, take.name, "ticks.png"),
                named,
                "every tick of the 300 ms in which its subject changes most",
                frames.track
                    .filter(
                        ({ tick }) =>
                            planned.key[0] <= tick && tick <= planned.key[1],
                    )
                    .map(caption),
                planned.close,
            ),
        };
        take.caption = caption;
    }
}

/** A take as `takes.json` tells it: where its frames and sheets are, what it is of, and each
 * step's subject tick by tick as the stage had it, times from the take's start. */
function told(take, { layout, seat, moment, fixture, learned, film, folder }) {
    const [first, last] = take.shots.track;
    return {
        layout,
        seat,
        moment,
        fixture,
        name: take.name,
        case: take.case,
        from: take.from,
        to: take.to,
        folder: join(folder, take.name),
        sheets: take.sheets,
        track: join(folder, take.name, "track"),
        crop: take.shots.close,
        action: take.shots.action,
        steps: take.steps.map((step, k) => {
            const name = step.subject;
            return {
                n: step.n,
                case: step.case,
                from: step.from - take.from,
                to: step.to - take.from,
                act: step.act,
                rest: take.shots.rests[k],
                poses: learned.ticks.slice(first, last + 1).map((tick, at) => {
                    const pose =
                        name === null ? null : film.pose(name, first + at);
                    return { at: tick.at - take.from, ...(pose ?? {}) };
                }),
            };
        }),
        // Every step that plays while the take is filmed, in order: what else the eye has.
        during: learned.steps
            .filter(
                (step) =>
                    step.to >= learned.ticks[take.shots.strip[0]].at &&
                    step.from <= learned.ticks[take.shots.strip.at(-1)].at,
            )
            .map((step) => ({
                verb: step.act.verb,
                case: step.case,
                from: step.from - take.from,
                to: step.to - take.from,
            })),
    };
}

const filmed = [];
for (const [layout, { query, size }] of Object.entries(LAYOUTS)) {
    await rm(join(out, layout), { recursive: true, force: true });
    for (const seat of Object.keys(suite.moment)) {
        const seen = new Set();
        for (const moment of MOMENTS) {
            const fixture = moment.whole
                ? moment.seats[seat]
                : moment.seats[seat].whole;
            const folder = join(out, layout, seat);
            const end = moment.whole
                ? ending(
                      JSON.parse(
                          await readFile(
                              join(fixtures, `${fixture}.json`),
                              "utf8",
                          ),
                      ),
                  )
                : null;
            const open = async () => ({
                ...(await openStage(browser, {
                    base,
                    fixtures,
                    size,
                    query,
                    fixture,
                    scale: SCALE,
                    paced: true,
                })),
                size,
            });
            // No recording draws a mark: its film is of the script written by hand, played
            // after the suite's first moment.
            const written = verb === "mark";
            const play = async (page, shoot) => {
                if (written) await installMarks(page);
                return page.evaluate(walk, {
                    tick: TICK,
                    gap: GAP,
                    answers: end?.answers ?? ANSWERS,
                    edge: EDGE,
                    shoot,
                    whole: moment.whole,
                    paced: true,
                    written,
                });
            };
            try {
                const first = await open();
                let learned;
                try {
                    learned = await play(first.page, []);
                    if (first.errors.length) {
                        throw new Error(first.errors.join("\n"));
                    }
                } finally {
                    await first.context.close();
                }
                if (
                    !learned.done ||
                    (end && !isDeepStrictEqual(learned.shows, end.shows))
                ) {
                    throw new Error("the moment did not play out");
                }
                const film = reel(learned.ticks);
                const chosen = takesOf(learned.steps).filter((take) => {
                    if (!every && seen.has(take.case)) return false;
                    take.shots = plan(take, learned, film, size);
                    take.name = `${moment.name}-${take.steps[0].n}-${slug(take.case)}`;
                    return take.shots && seen.add(take.case);
                });
                if (!chosen.length) continue;
                await shoot(
                    folder,
                    chosen,
                    learned,
                    open,
                    play,
                    seat,
                    `${verb} · ${layout} · ${seat}`,
                );
                for (const take of chosen) {
                    filmed.push({
                        take,
                        ...told(take, {
                            layout,
                            seat,
                            moment: moment.name,
                            fixture,
                            learned,
                            film,
                            folder,
                        }),
                    });
                }
                console.log(
                    `filmed: ${layout}/${seat}/${moment.name}, ${chosen.length} takes`,
                );
            } catch (e) {
                failed = true;
                console.error(
                    `FAILED: ${layout}/${seat}/${moment.name}\n${e.stack ?? e}`,
                );
            }
        }
    }
}

// The contact sheets: an Arena instance over each seat's take of its case on the desktop, every
// frame at one scale of the table, a fifteenth of a second apart.
const contacts = [];
if (arena) {
    const { instances } = JSON.parse(await readFile(arena, "utf8"));
    const numbered = new Map();
    await mkdir(join(out, "contact"), { recursive: true });
    const table = LAYOUTS.desktop.size.width;
    for (const instance of instances) {
        const mine = filmed.filter(
            (f) => f.layout === "desktop" && f.case === instance.case,
        );
        if (!mine.length || !instance.frames.length) continue;
        const k = (numbered.get(instance.case) ?? 0) + 1;
        numbered.set(instance.case, k);
        // At most CONTACT of the instance's frames, and the take's as far apart in time.
        const skip = Math.ceil(instance.frames.length / CONTACT);
        const rate = Math.round((10 * 15) / skip) / 10;
        const fit = (width) =>
            Math.max(1, Math.floor((WIDE + 8) / (Math.round(width) + 8)));
        const widest = Math.max(...mine.map((f) => f.action.width / table));
        /** The sheet's two sections at `scale` pixels a table's width, and how tall they
         * come to. */
        const drawn = (f, scale) => {
            const sections = [
                {
                    label: `Arena · ${instance.label}`,
                    frames: instance.frames
                        .filter((_, n) => n % skip === 0)
                        .map(({ path, at }) => ({
                            path,
                            caption: at,
                            live: false,
                        })),
                    tile: Math.round(scale * instance.share),
                    aspect: instance.height / instance.width,
                },
                {
                    label: `Beryl · ${f.seat} · ${f.name}`,
                    frames: f.take.frames.strip
                        .filter((_, n) => n % (2 * skip) === 0)
                        .map(f.take.caption),
                    tile: Math.round((scale * f.action.width) / table),
                    aspect: f.action.height / f.action.width,
                },
            ].map((section) => ({ ...section, across: fit(section.tile) }));
            const tall = sections.reduce(
                (sum, { frames, tile, across, aspect }) =>
                    sum +
                    Math.ceil(frames.length / across) * (tile * aspect + 24),
                0,
            );
            return { sections, tall };
        };
        // A sheet a seat, so that each stays within what one image shows clearly: Arena's
        // frame as wide as keeps the sheet within a page, no wider than it was cut, and three
        // of the take's frames to a row at least.
        for (const f of mine) {
            const scales = ARENA_TILES.map((tile) =>
                Math.min(
                    tile / instance.share,
                    (WIDE / 3 - 8) / widest,
                    instance.width / instance.share,
                ),
            );
            const scale =
                scales.find((scale) => drawn(f, scale).tall <= TALL) ??
                scales.at(-1);
            const path = join(
                out,
                "contact",
                `${slug(instance.case)}-${k}-${f.seat}.png`,
            );
            await sheet(
                path,
                `${verb} · ${instance.case ?? "any"} · Arena over Beryl at one scale of the table, ${rate} frames a second`,
                drawn(f, scale).sections,
            );
            contacts.push({
                case: instance.case,
                instance: instance.label,
                seat: f.seat,
                path,
            });
        }
    }
}
await writeFile(
    join(out, "takes.json"),
    `${JSON.stringify(
        {
            verb,
            tick: TICK,
            takes: filmed.map(({ take, ...rest }) => rest),
            contacts,
        },
        null,
        1,
    )}\n`,
);
await browser.close();
process.exit(failed ? 1 : 0);
