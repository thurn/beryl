// A headless WebKit page an agent drives one step at a time: `by ui-live` (tools/) starts it
// beside the live game server and Vite. Usage:
//   node scripts/drive.mjs <port> <app-url> <out-dir> [width] [height]
// Each `POST /` body is the body of an async JavaScript function, run in Node with these in
// scope, whose return value comes back as JSON:
//   page, context   the Playwright page (opened at <app-url>) and its context
//   shot(name)      screenshots the page to <out-dir>/<n>-<name>.png and returns the path
//   probe           the measuring helpers below (`act`, `compare` and `sweep` first)
//   errors          the page's errors and console errors so far
// Restart `by ui-live` to load a change to this file; changes under src/ reload in the page.
// Motion is not reduced, unlike ui-shots: animation and layout shift are what a player sees.
import { createServer } from "node:http";
import { mkdir } from "node:fs/promises";
import { join } from "node:path";
import { compareSheet, launch } from "./capture.mjs";

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

const browser = await launch();
// Playwright closes the browser on a signal, but the server below would keep Node running:
// exit, so `by ui-live` stopping leaves no driver holding its port.
for (const signal of ["SIGTERM", "SIGINT", "SIGHUP"]) {
    process.on(signal, () => {
        browser.close().finally(() => process.exit(0));
    });
}
const context = await browser.newContext({
    viewport: { width: Number(width), height: Number(height) },
});
const page = await context.newPage();
const errors = [];
page.on("pageerror", (e) => errors.push(e.message));
page.on("console", (m) => m.type() === "error" && errors.push(m.text()));
await page.goto(url);

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

createServer((request, response) => {
    let body = "";
    request.on("data", (chunk) => (body += chunk));
    request.on("end", async () => {
        let answer;
        try {
            const run = new (Object.getPrototypeOf(async () => {}).constructor)(
                "page",
                "context",
                "shot",
                "probe",
                "errors",
                body,
            );
            answer = { ok: await run(page, context, shot, probe, errors) };
        } catch (e) {
            answer = { error: String(e?.stack ?? e) };
        }
        response.writeHead(200, { "Content-Type": "application/json" });
        response.end(JSON.stringify(answer, null, 1) + "\n");
    });
}).listen(Number(port), "127.0.0.1", () =>
    console.log(`drive: http://127.0.0.1:${port}/ driving ${url}`),
);
