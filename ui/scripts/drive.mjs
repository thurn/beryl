// A headless WebKit page an agent drives one step at a time: `by ui-live` (tools/) starts it
// beside the live game server and Vite, and `by specs --drive` beside the spec server. Usage:
//   node scripts/drive.mjs <port> <app-url> <out-dir> [width] [height]
// Each `POST /` body is the body of an async JavaScript function, run in Node with these in
// scope, whose return value comes back as JSON. Snippets run one at a time, in the order
// they arrive:
//   page, context   the Playwright page (opened at <app-url>) and its context; `page` is
//                   always the current one, so `spec.seat` and `spec.fixture` switch it at once
//   shot(name)      screenshots the page to <out-dir>/<n>-<name>.png and returns the path
//   probe           the measuring helpers below (`act`, `compare` and `sweep` first)
//   errors          the page's errors and console errors so far
//   spec            spec moments (`by specs --drive`): `open(card, moment, seat)` opens one seat
//                   in `page`, held; `both(card, moment)` opens yours in `page` and theirs in a
//                   second page; `play()` plays every open seat at once; `audit()` plays them
//                   and audits each seat's motion (`audit(action)` a live click's);
//                   `ordered(patterns)` asserts the order things showed in; `seat(name)` makes
//                   that seat's page the one `page`, `shot` and `probe` act on; `settled()`
//                   waits for the page to go quiet; `fixture(name, at)` opens a ui-test or
//                   ui-shots fixture; `gestures()` drags each card the viewer holds.
// The browser is launched again if it goes away. Restart `by ui-live` to load a change to
// this file (`by specs --drive` restarts the driver itself); changes under src/ reload in
// the page. Motion is not reduced, unlike ui-shots: animation and layout shift are what a
// player sees.
import { createServer } from "node:http";
import { execFile } from "node:child_process";
import { access, mkdir, readFile } from "node:fs/promises";
import { basename, join } from "node:path";
import { promisify } from "node:util";
import { compareSheet, launch } from "./capture.mjs";
import { ordered, timeline } from "./audit.mjs";
import { summary } from "./fixture-views.mjs";
import { openSeat, play, settled } from "./moments.mjs";
import { watching } from "./stage-audit.mjs";

const [port, url, out, width = "1600", height = "900"] = process.argv.slice(2);
if (!port || !url || !out) {
    console.error(
        "usage: node scripts/drive.mjs <port> <app-url> <out-dir> [width] [height]",
    );
    process.exit(2);
}
await mkdir(out, { recursive: true });

// A snippet's stray promise must not end the driver and the review with it.
process.on("unhandledRejection", (e) =>
    console.error(`drive: unhandled rejection: ${e?.stack ?? e}`),
);

let browser = await launch();
// Playwright closes the browser on a signal, but the server below would keep Node running:
// exit, so `by ui-live` stopping leaves no driver holding its port.
let exiting = false;
for (const signal of ["SIGTERM", "SIGINT", "SIGHUP"]) {
    process.on(signal, () => {
        exiting = true;
        browser.close().finally(() => process.exit(0));
    });
}
const size = { width: Number(width), height: Number(height) };
const errors = [];
const watch = (p, prefix = "") => {
    p.on("pageerror", (e) => errors.push(`${prefix}${e.message}`));
    p.on(
        "console",
        (m) => m.type() === "error" && errors.push(`${prefix}${m.text()}`),
    );
};
let context;
// The page `shot` and `probe` act on: a seat's, once `spec.seat` picks it.
let current;
let first;
/** Opens the context and its first page at the app. */
async function begin() {
    context = await browser.newContext({ viewport: size });
    current = first = await context.newPage();
    watch(first);
    await first.goto(url);
}
await begin();
// A browser that went away (a crash, the machine under memory pressure) comes back with a
// fresh page; the snippet after it opens its moments again.
const relaunch = () => {
    if (exiting) return;
    console.error("drive: the browser went away; launching it again");
    for (const key of Object.keys(seats)) delete seats[key];
    launch()
        .then(async (b) => {
            browser = b;
            browser.on("disconnected", relaunch);
            await begin();
        })
        .catch((e) => console.error(`drive: relaunching failed: ${e}`));
};
browser.on("disconnected", relaunch);
/** `page` as snippets see it: whichever page is current when they use it. */
const page = new Proxy(
    {},
    {
        get(_, key) {
            const value = current[key];
            return typeof value === "function" ? value.bind(current) : value;
        },
    },
);

let shots = 0;
const shot = async (name = "shot") => {
    shots += 1;
    const path = join(out, `${String(shots).padStart(3, "0")}-${name}.png`);
    await page.screenshot({ path });
    return path;
};

/** What the probes describe an element as: its tag, data attributes, role and some text. */
const DESCRIBE = `(e) => {
    const data = [...e.attributes]
        .filter((a) => (a.name.startsWith("data-") && !a.name.startsWith("data-probe")) || a.name === "role" || a.name === "class")
        .map((a) => a.name + "=" + JSON.stringify(a.value.slice(0, 40)))
        .join(" ");
    const text = (e.innerText || e.getAttribute("aria-label") || "").trim().slice(0, 40);
    return e.tagName.toLowerCase() + (data ? " " + data : "") + (text ? " «" + text + "»" : "");
}`;

/** Elements whose movement the motion probe tracks by default. */
const TRACKED =
    "[data-slot], [data-zone], [data-card], button, [role=button], [role=dialog], h1, h2, h3, img, svg";

const probe = {
    /** Starts recording where every element matching `selector` is, each frame. Call the
     * returned function after an action (it waits `ms` more first) for the elements that moved
     * or resized since recording began, worst first: how far and over how many frames. */
    async motion(selector = TRACKED, { exclude } = {}) {
        await page.evaluate(
            ([selector, describe, exclude]) => {
                const name = new Function(`return ${describe}`)();
                const seen = new Map();
                const frame = () => {
                    for (const e of document.querySelectorAll(selector)) {
                        if (exclude && e.closest(exclude)) continue;
                        const r = e.getBoundingClientRect();
                        if (r.width === 0 && r.height === 0) continue;
                        let s = seen.get(e);
                        if (!s) {
                            s = {
                                name: name(e),
                                first: r,
                                last: r,
                                moves: 0,
                                max: 0,
                            };
                            seen.set(e, s);
                        }
                        const d = Math.max(
                            Math.abs(r.left - s.first.left),
                            Math.abs(r.top - s.first.top),
                            Math.abs(r.width - s.first.width),
                            Math.abs(r.height - s.first.height),
                        );
                        if (
                            r.left !== s.last.left ||
                            r.top !== s.last.top ||
                            r.width !== s.last.width ||
                            r.height !== s.last.height
                        ) {
                            s.moves += 1;
                        }
                        s.max = Math.max(s.max, d);
                        s.last = r;
                    }
                    const m = window.__motion;
                    if (m?.seen !== seen) return;
                    for (const e of document.querySelectorAll("*")) {
                        if (e.scrollTop || e.scrollLeft) {
                            const was = m.scrolls.get(e) ?? [0, 0];
                            m.scrolls.set(e, [
                                Math.max(was[0], Math.abs(e.scrollLeft)),
                                Math.max(was[1], Math.abs(e.scrollTop)),
                            ]);
                        }
                    }
                    requestAnimationFrame(frame);
                };
                window.__motion = {
                    seen,
                    scrolls: new Map(),
                    start: performance.now(),
                };
                requestAnimationFrame(frame);
            },
            [selector, DESCRIBE, exclude],
        );
        return async (ms = 1000) => {
            await page.waitForTimeout(ms);
            return page.evaluate((describe) => {
                const name = new Function(`return ${describe}`)();
                const { seen, scrolls, start } = window.__motion;
                window.__motion = undefined;
                const moved = [...seen.values()]
                    .filter((s) => s.max >= 2)
                    .sort((a, b) => b.max - a.max)
                    .map((s) => ({
                        element: s.name,
                        maxShiftPx: Math.round(s.max),
                        framesMoving: s.moves,
                    }));
                return {
                    ms: Math.round(performance.now() - start),
                    // A scrolled box moves everything in it: a game screen should not scroll.
                    scrolled: [...scrolls].map(([e, [x, y]]) => ({
                        element: name(e),
                        maxScrollPx: [x, y],
                    })),
                    tracked: seen.size,
                    moved: moved.length,
                    worst: moved.slice(0, 25),
                };
            }, DESCRIBE);
        };
    },

    /** Sweeps the pointer across every element `locator` matches, left to right, resting
     * `dwell` ms on each as a player's pointer would. For each stop it reports what moved
     * other than the hovered element, the one the pointer just left (which may settle back),
     * and their contents, and what became covered that was not covered before the sweep.
     * Only what is pointed at should change. */
    async sweep(locator, { dwell = 400, selector = TRACKED } = {}) {
        const count = await locator.count();
        const items = [];
        for (let i = 0; i < count; i += 1) {
            const box = await locator.nth(i).boundingBox();
            if (box) items.push({ i, x: box.x });
        }
        items.sort((a, b) => a.x - b.x);
        await page.mouse.move(1, 1);
        await page.waitForTimeout(dwell);
        const coveredBefore = new Set(
            (await probe.covered()).map((c) => c.element),
        );
        const stops = [];
        let previous;
        for (const { i } of items) {
            const target = locator.nth(i);
            await target.evaluate((e) =>
                e.setAttribute("data-probe-hover", ""),
            );
            const stop = await probe.motion(selector, {
                exclude: "[data-probe-hover]",
            });
            const point = await probe.pointAt(target);
            const motion = await stop(dwell);
            const covered = (await probe.covered()).filter(
                (c) => !coveredBefore.has(c.element),
            );
            const name = await target.evaluate(
                (e) =>
                    e.getAttribute("data-card") ??
                    e.getAttribute("aria-label") ??
                    e.tagName.toLowerCase(),
            );
            await previous
                ?.evaluate((e) => e.removeAttribute("data-probe-hover"))
                .catch(() => undefined);
            previous = target;
            if (point && (motion.moved > 0 || covered.length > 0)) {
                stops.push({
                    hovered: name,
                    othersMoved: motion.moved,
                    worst: motion.worst.slice(0, 5),
                    newlyCovered: covered.slice(0, 5),
                });
            }
        }
        await previous
            ?.evaluate((e) => e.removeAttribute("data-probe-hover"))
            .catch(() => undefined);
        await page.mouse.move(1, 1);
        return { swept: items.length, stopsWithSideEffects: stops };
    },

    /** Moves the mouse onto the visible part of `locator`'s element without scrolling, as a
     * player's pointer would (Playwright's `hover` and `click` scroll their target into view
     * first, which moves the page). Returns the point, or null if none of it is on screen. */
    async pointAt(locator) {
        // The point nearest the middle of its box on screen where the element is what the
        // pointer hits: in a fan, its strip rather than the neighbor over its middle.
        const point = await locator
            .evaluate((e) => {
                const r = e.getBoundingClientRect();
                const [left, right] = [
                    Math.max(r.left, 0),
                    Math.min(r.right, innerWidth),
                ];
                const [top, bottom] = [
                    Math.max(r.top, 0),
                    Math.min(r.bottom, innerHeight),
                ];
                if (right - left < 2 || bottom - top < 2) return null;
                const mid = { x: (left + right) / 2, y: (top + bottom) / 2 };
                let best = null;
                for (let y = top + 1; y < bottom; y += 4) {
                    for (let x = left + 1; x < right; x += 4) {
                        if (!e.contains(document.elementFromPoint(x, y)))
                            continue;
                        const d = Math.hypot(x - mid.x, y - mid.y);
                        if (!best || d < best.d) best = { x, y, d };
                    }
                }
                return best ?? mid;
            })
            .catch(() => null);
        if (!point) return null;
        await page.mouse.move(point.x, point.y);
        return { x: point.x, y: point.y };
    },

    /** Elements matching `selector` that something else covers: at their center or at points
     * inset from their corners, the topmost element is neither they nor inside them. */
    covered(selector = "[data-slot], [data-zone], button, [role=button]") {
        return page.evaluate(
            ([selector, describe]) => {
                const name = new Function(`return ${describe}`)();
                const out = [];
                for (const e of document.querySelectorAll(selector)) {
                    const r = e.getBoundingClientRect();
                    if (r.width < 4 || r.height < 4) continue;
                    const inset = Math.min(8, r.width / 4, r.height / 4);
                    const points = [
                        [r.left + r.width / 2, r.top + r.height / 2],
                        [r.left + inset, r.top + inset],
                        [r.right - inset, r.top + inset],
                        [r.left + inset, r.bottom - inset],
                        [r.right - inset, r.bottom - inset],
                    ].filter(
                        ([x, y]) =>
                            x >= 0 &&
                            y >= 0 &&
                            x < innerWidth &&
                            y < innerHeight,
                    );
                    if (points.length === 0) {
                        out.push({ element: name(e), offscreen: true });
                        continue;
                    }
                    const by = new Set();
                    let hidden = 0;
                    for (const [x, y] of points) {
                        const top = document.elementFromPoint(x, y);
                        if (
                            top &&
                            top !== e &&
                            !e.contains(top) &&
                            !top.contains(e)
                        ) {
                            hidden += 1;
                            by.add(name(top));
                        }
                    }
                    if (hidden > 0) {
                        out.push({
                            element: name(e),
                            pointsCovered: `${hidden}/${points.length}`,
                            by: [...by].slice(0, 3),
                        });
                    }
                }
                return out;
            },
            [selector, DESCRIBE],
        );
    },

    /** Visible text cut off by its box: overflowing an element that clips, ellipsized or
     * line-clamped. */
    truncated() {
        return page.evaluate((describe) => {
            const name = new Function(`return ${describe}`)();
            const out = [];
            for (const e of document.querySelectorAll("body *")) {
                if (!e.childNodes.length || !(e.innerText || "").trim())
                    continue;
                const s = getComputedStyle(e);
                const clips =
                    s.overflow !== "visible" ||
                    s.overflowX !== "visible" ||
                    s.textOverflow === "ellipsis" ||
                    s.webkitLineClamp !== "none";
                if (!clips) continue;
                const over =
                    e.scrollWidth > e.clientWidth + 1 ||
                    e.scrollHeight > e.clientHeight + 1;
                const r = e.getBoundingClientRect();
                if (over && r.width > 0 && r.height > 0) {
                    out.push({
                        element: name(e),
                        text: e.innerText.trim().slice(0, 80),
                        box: [Math.round(r.width), Math.round(r.height)],
                        content: [e.scrollWidth, e.scrollHeight],
                    });
                }
            }
            return out;
        }, DESCRIBE);
    },

    /** The live game's messages so far (`window.berylLive.log`, src/ipc.ts), summarized: when
     * each command was sent and answered, and each view that arrived, with its turn, step,
     * who had priority, whether the human was asked a decision, and what happened in it. */
    timeline({ since = 0 } = {}) {
        return page.evaluate((since) => {
            const log = window.berylLive?.log;
            if (!log) return "not a live page (?live=)";
            return log
                .filter((m) => m.at >= since)
                .map((m) => {
                    const at = Math.round(m.at);
                    if (m.kind === "invoke") {
                        return { at, sent: m.command, args: m.args };
                    }
                    if (m.kind === "result") {
                        return { at, answered: m.command, ok: m.ok };
                    }
                    const v = m.payload.view;
                    const names = new Map(
                        v.objects.map((o) => [o.slot, o.name]),
                    );
                    const what = (v.events ?? []).map((e) => {
                        const parts = [e.kind];
                        for (const [k, x] of Object.entries(e)) {
                            if (k === "kind") continue;
                            const named =
                                (k === "slot" || k === "source") && names.has(x)
                                    ? names.get(x)
                                    : x;
                            parts.push(`${k}=${JSON.stringify(named)}`);
                        }
                        return parts.join(" ");
                    });
                    return {
                        at,
                        view: `turn ${v.turn.number} ${v.turn.active} ${v.turn.step}, priority ${v.turn.priority}`,
                        status:
                            v.status.kind === "deciding"
                                ? `deciding ${v.status.decision.kind}`
                                : v.status.kind,
                        events: what,
                    };
                });
        }, since);
    },

    /** Runs `action` (an async function, e.g. a click), then waits up to `timeout` ms for the
     * next view that asks the human a decision or ends the game. Says how long that took, how
     * many views arrived meanwhile, and what the decision is. Without an action, waits for
     * the next such view from now, or from log entry `from`. */
    async decision(action, timeout = 60000, from = undefined) {
        const before =
            from ??
            (await page.evaluate(() => window.berylLive?.log.length ?? 0));
        const start = Date.now();
        if (action) await action();
        const found = await page.waitForFunction(
            (before) => {
                const log = window.berylLive?.log ?? [];
                const next = log
                    .slice(before)
                    .find(
                        (m) =>
                            m.kind === "event" &&
                            ["deciding", "over"].includes(
                                m.payload.view.status.kind,
                            ),
                    );
                return next ? next.payload.view.status : false;
            },
            before,
            { timeout },
        );
        const status = await found.jsonValue();
        const views = await page.evaluate(
            (before) =>
                window.berylLive.log
                    .slice(before)
                    .filter((m) => m.kind === "event").length,
            before,
        );
        return {
            waitedMs: Date.now() - start,
            views,
            status:
                status.kind === "deciding"
                    ? `${status.decision.kind}: ${status.decision.candidates
                          .map((c) => c.label)
                          .slice(0, 12)
                          .join(" | ")}`
                    : status.kind,
        };
    },

    /** One player action, observed the way the review loop asks: a screenshot before, the
     * action, screenshots at each of `samples` ms after it ended, then (unless `wait` is
     * false) a wait for the next decision and a screenshot of it. Returns the screenshot
     * paths, how long after the action the first view and the next decision arrived, what
     * moved meanwhile, the decision waited for, and the game events that arrived
     * (`probe.timeline` from the start of the action). */
    async act(
        name,
        action,
        { samples = [100, 500, 2000], wait = true, timeout = 120000 } = {},
    ) {
        const shots = { before: await shot(`${name}-before`) };
        const stop = await probe.motion();
        // Latencies count from here, after the setup, so they measure the UI alone.
        const [since, from] = await page.evaluate(() => [
            performance.now(),
            window.berylLive?.log.length ?? 0,
        ]);
        // The gesture runs whole before any sample: a screenshot taken in the middle of a
        // drag can drop its pointer events. A failure ends the call once the motion probe
        // is stopped.
        const failed = await Promise.resolve()
            .then(action)
            .then(
                () => undefined,
                (e) => e ?? new Error("the action failed"),
            );
        const start = Date.now();
        if (!failed) {
            for (const ms of samples) {
                await page.waitForTimeout(
                    Math.max(0, ms - (Date.now() - start)),
                );
                shots[`${ms}ms`] = await shot(`${name}-${ms}ms`);
            }
        }
        const motion = await stop(0);
        if (failed) throw failed;
        let decision;
        if (wait) {
            decision = await probe
                .decision(null, timeout, from)
                .catch((e) => ({ timeout: String(e).split("\n")[0] }));
            shots.after = await shot(`${name}-after`);
        }
        // Latencies from the action, on the page's clock: the first view, and the decision.
        const views = await page.evaluate(
            ([since, from]) =>
                (window.berylLive?.log ?? [])
                    .slice(from)
                    .filter((m) => m.kind === "event")
                    .map((m) => ({
                        ms: Math.round(m.at - since),
                        deciding: ["deciding", "over"].includes(
                            m.payload.view.status.kind,
                        ),
                    })),
            [since, from],
        );
        return {
            firstViewMs: views[0]?.ms ?? null,
            decisionMs: views.find((v) => v.deciding)?.ms ?? null,
            shots,
            motion,
            decision,
            timeline: await probe.timeline({ since }),
        };
    },

    /** The motion audit (stage-audit.mjs) of whatever `action` sets going (an async function:
     * a click, or `spec.play`), as `by specs --motion` judges it: it holds the stage's clock
     * from here until `action` returns and then steps it, so `action` sets things going and
     * waits for nothing the stage plays. Returns the findings that are not ok (all of them
     * with `all`). */
    async audit(action, { all = false } = {}) {
        const watched = await watching(page);
        await action();
        const findings = await watched();
        return all ? findings : findings.filter((f) => f.verdict !== "ok");
    },

    /** Drags from `from` (a locator or selector: where its element shows, as a pointer would
     * find it) to `to` (one too, or a point `{ x, y }`) in `steps` moves, as a player's drag.
     * Returns how far the element followed the pointer at most, where it ended against where
     * it began, and the commands the page sent meanwhile. */
    async drag(from, to, { steps = 12 } = {}) {
        const source =
            typeof from === "string" ? page.locator(from).first() : from;
        const target = typeof to === "string" ? page.locator(to).first() : to;
        const sent = () =>
            page.evaluate(() =>
                (window.berylLive?.log ?? [])
                    .filter((m) => m.kind === "invoke")
                    .map((m) => m.command)
                    .concat(
                        (window.berylFixture?.calls ?? []).map(
                            (c) => c.command,
                        ),
                    ),
            );
        const before = (await sent()).length;
        const start = await source.boundingBox();
        const grab = await probe.pointAt(source);
        if (!start || !grab) throw new Error("nothing to drag shows on screen");
        const end =
            "x" in target && "y" in target && !("boundingBox" in target)
                ? target
                : await target
                      .boundingBox()
                      .then((b) =>
                          b
                              ? { x: b.x + b.width / 2, y: b.y + b.height / 2 }
                              : null,
                      );
        if (!end) throw new Error("nowhere to drop shows on screen");
        await page.mouse.down();
        let followed = 0;
        for (let i = 1; i <= steps; i++) {
            await page.mouse.move(
                grab.x + ((end.x - grab.x) * i) / steps,
                grab.y + ((end.y - grab.y) * i) / steps,
            );
            const box = await source.boundingBox().catch(() => null);
            if (box) {
                followed = Math.max(
                    followed,
                    Math.hypot(box.x - start.x, box.y - start.y),
                );
            }
        }
        await page.mouse.up();
        await page.waitForTimeout(600);
        const after = await source.boundingBox().catch(() => null);
        await page.mouse.move(1, 1);
        return {
            followedPx: Math.round(followed),
            endedPx: after
                ? Math.round(Math.hypot(after.x - start.x, after.y - start.y))
                : null,
            sent: (await sent()).slice(before),
        };
    },

    /** Crops each element (a locator, or a selector) out of the page at its size on screen,
     * with `pad` px around it, and lays the crops side by side, labelled, in one image: for
     * judging whether two states, or two things that should look alike, can be told apart at
     * a glance. Returns the image's path. */
    async compare(name, targets, options) {
        shots += 1;
        const path = join(
            out,
            `${String(shots).padStart(3, "0")}-${name}-compare.png`,
        );
        await compareSheet(page, targets, path, options);
        return path;
    },
};

/** Spec moments, for `by specs --drive`: seats open held, so a snippet watches from the
 * first frame, and `play` starts them together. */
const base = new URL("/", url).href;
const seats = {};
/** Each seat's timeline from the last `spec.audit`, for `spec.ordered`. */
const timelines = {};
/** The recorded fixtures `spec.fixture` opens, and the CLI that records them. */
const recordings = join(out, "fixtures");
const cli = process.env.BERYL_CLI;
const definitions = new URL("../fixtures.json", import.meta.url).pathname;
let fixtureContext;

/** The recorded fixture `name`, recorded once (`beryl_cli record-fixture`). Returns its
 * folder. */
async function recorded(name) {
    const dir = join(recordings, name);
    const file = join(dir, `${name}.json`);
    if (
        await access(file).then(
            () => true,
            () => false,
        )
    )
        return dir;
    if (!cli) throw new Error("no BERYL_CLI to record fixtures with");
    await mkdir(dir, { recursive: true });
    await promisify(execFile)(cli, ["record-fixture", definitions, dir, name], {
        maxBuffer: 1 << 26,
    });
    return dir;
}

const spec = {
    async playground(card) {
        const response = await fetch(
            new URL(`spec-api/playground/${card}`, base),
        );
        if (!response.ok) throw new Error(`No Playground for ${card}`);
        return response.json();
    },
    async checkpoints(card) {
        const response = await fetch(
            new URL(`spec-api/checkpoints/${card}`, base),
        );
        if (!response.ok) throw new Error(`No review checkpoints for ${card}`);
        return response.json();
    },
    async open(card, moment, seat = "yours", instance) {
        if (!seats[seat]) {
            const p = seat === "yours" ? first : await context.newPage();
            if (p !== first) watch(p, `${seat}: `);
            seats[seat] = { page: p };
        }
        const s = seats[seat];
        s.live = await openSeat(s.page, base, card, moment, seat, instance);
        current = s.page;
        return s.page;
    },
    async both(card, moment) {
        const instance = moment.startsWith("review-")
            ? `review-${Math.random().toString(36).slice(2)}`
            : undefined;
        const yours = await spec.open(card, moment, "yours", instance);
        const theirs = await spec.open(card, moment, "theirs", instance);
        current = yours;
        return { yours, theirs };
    },
    play() {
        return play(
            ...Object.values(seats)
                .map((s) => s.live)
                .filter(Boolean),
        );
    },
    /** Plays every open seat at once (or runs `action`, a live click, say) and audits
     * each seat's motion (stage-audit.mjs): the findings that are not ok, by seat. */
    async audit(action, { all = false } = {}) {
        const open = Object.entries(seats);
        const watched = await Promise.all(
            open.map(([, s]) => watching(s.page)),
        );
        await (action ?? (() => spec.play()))();
        // Every seat is watched to its end at once, as they play at once.
        const found = await Promise.all(watched.map((end) => end()));
        const out = {};
        for (const [i, [name]] of open.entries()) {
            const findings = found[i];
            timelines[name] = timeline(findings);
            out[name] = all
                ? findings
                : findings.filter((f) => f.verdict !== "ok");
        }
        return out;
    },
    /** What each seat showed in the last `audit` (or a fresh one, if none), in order: `{ at,
     * text }` by seat, or one seat's. */
    async timeline(seat) {
        if (Object.keys(timelines).length === 0) await spec.audit();
        return seat ? timelines[seat] : timelines;
    },
    /** Whether `seat` (the current one by default) showed what `patterns` match in that
     * order (audit.mjs's `ordered`: substrings or RegExps, each taking the first entry it
     * matches), in the last `audit`, or one run now with `action` (default: play every
     * seat). Returns `ok`, each pattern's match, why not, and the seat's whole timeline. */
    async ordered(patterns, { seat, action } = {}) {
        if (action || Object.keys(timelines).length === 0) {
            await spec.audit(action);
        }
        const name =
            seat ??
            Object.entries(seats).find(([, s]) => s.page === current)?.[0] ??
            "yours";
        const entries = timelines[name];
        if (!entries) throw new Error(`no audit of the ${name} seat`);
        return { seat: name, ...ordered(entries, patterns), timeline: entries };
    },
    seat(name) {
        if (!seats[name]) throw new Error(`no ${name} seat open`);
        current = seats[name].page;
        return current;
    },
    settled(p = current) {
        return settled(p);
    },
    /** Opens fixture `name` (ui/fixtures.json, as ui-test and ui-shots do) at its view `at`
     * (all of them if none) in a page of its own, which becomes `page`: recorded on first use.
     * `clock` installs Playwright's clock, as ui-test does, so `page.clock.runFor` paces it;
     * `viewport` sizes it (`{ width, height }`); it is open once `ready` shows. */
    async fixture(
        name,
        at,
        {
            clock = false,
            viewport = size,
            ready = "[data-life]",
            hash = "#/game",
        } = {},
    ) {
        const dir = await recorded(name);
        await fixtureContext?.close();
        fixtureContext = await browser.newContext({
            viewport,
            deviceScaleFactor: 1,
        });
        await fixtureContext.route("**/fixtures/*.json", (route) =>
            route.fulfill({
                path: join(
                    dir,
                    decodeURIComponent(
                        basename(new URL(route.request().url()).pathname),
                    ),
                ),
            }),
        );
        const p = await fixtureContext.newPage();
        watch(p, `${name}: `);
        if (clock) await p.clock.install();
        const query = at === undefined ? "" : `&at=${at}`;
        await p.goto(new URL(`/?fixture=${name}${query}${hash}`, base).href);
        await p.waitForSelector(ready);
        seats.fixture = { page: p };
        current = p;
        return p;
    },
    /** Fixture `name`'s views, each with its index for `at`, its turn and step, what it
     * asks and what happened in it (fixture-views.mjs). */
    async fixtureViews(name) {
        const dir = await recorded(name);
        return summary(
            JSON.parse(await readFile(join(dir, `${name}.json`), "utf8")),
        );
    },
    /** Drags each card the viewer holds as a player might: each of their permanents (at most
     * `most`) a short way up, which must not move it or send a command (a permanent never
     * drags: a click uses it), and each playable card in their hand to the table's middle,
     * which plays it. Reports each drag, and `faults`: the drags that went wrong. Leaves the
     * game where the last drag left it, so open the moment again after. */
    async gestures({ most = 6 } = {}) {
        const drags = [];
        const permanents = page.locator(".card-el-you[data-zone=battlefield]");
        const n = Math.min(most, await permanents.count());
        for (let i = 0; i < n; i++) {
            const card = permanents.nth(i);
            const name = await card.getAttribute("data-card");
            const box = await card.boundingBox();
            if (!box) continue;
            const r = await probe.drag(card, {
                x: box.x + box.width / 2,
                y: Math.max(10, box.y - 120),
            });
            drags.push({
                card: name,
                zone: "battlefield",
                ...r,
                fault:
                    r.followedPx > 30
                        ? "a permanent moved with the pointer"
                        : r.sent.includes("choose")
                          ? "a drag off a permanent acted as a click"
                          : undefined,
            });
        }
        const playable = page.locator(
            ".card-el-you.card-el-playable[data-zone=hand]",
        );
        if ((await playable.count()) > 0) {
            const card = playable.first();
            const name = await card.getAttribute("data-card");
            const r = await probe.drag(card, {
                x: size.width / 2,
                y: size.height * 0.45,
            });
            drags.push({
                card: name,
                zone: "hand",
                ...r,
                fault:
                    r.followedPx < 30
                        ? "a playable hand card did not follow the pointer"
                        : undefined,
            });
        }
        return { drags, faults: drags.filter((d) => d.fault) };
    },
};

/** Snippets run one at a time: two at once would act on the same page. */
let queue = Promise.resolve();

createServer((request, response) => {
    let body = "";
    request.on("data", (chunk) => (body += chunk));
    request.on("end", () => {
        queue = queue.then(() => answer(body, response));
    });
}).listen(Number(port), "127.0.0.1", () =>
    console.log(`drive: http://127.0.0.1:${port}/ driving ${url}`),
);

/** Runs one snippet and answers its request. */
async function answer(body, response) {
    let result;
    try {
        const run = new (Object.getPrototypeOf(async () => {}).constructor)(
            "page",
            "context",
            "shot",
            "probe",
            "errors",
            "spec",
            body,
        );
        result = { ok: await run(page, context, shot, probe, errors, spec) };
    } catch (e) {
        result = { error: String(e?.stack ?? e) };
    }
    response.writeHead(200, { "Content-Type": "application/json" });
    response.end(JSON.stringify(result, null, 1) + "\n");
}
