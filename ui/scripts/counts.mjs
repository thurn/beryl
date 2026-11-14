// Counts the work the page does to play recorded fixtures, in units that do not depend on how
// fast or how loaded the machine is: React components rendered, DOM mutations, and Chromium's
// style and layout work. Where perf.mjs times frames on the browser's own clock, this plays
// each fixture on Playwright's fake clock, one 60 Hz frame at a time, so the same tree does the
// same work on every run. Usage:
//   node scripts/counts.mjs <base-url> <fixtures-dir> [--json out.json] [--components]
//        [--layouts desktop,phone] name...
// Prints one row per fixture and layout; `--json` also writes {"<fixture>/<layout>": {metric:
// count}}, and `--components` lists the components that rendered most. `by ui-counts` records
// the fixtures, serves the UI and holds the counts to `data/ui-counts.json`.
//
// Each frame advances the paused fake clock by one frame, which runs the page's animation-frame
// callbacks (the stage's clock and Motion's) and its timers, and then forces a style and
// layout pass by reading the layout, as the browser's own frame would. The browser's real
// frames then find nothing dirty, so every style and layout pass happens at a point this
// script chose. Three other things run on real time, and each is pinned to the frame:
// - CSS animations and transitions run on the document's timeline, which no fake clock stops,
//   so they are finished at once (`settle` below) before the frame's layout.
// - React renders in tasks it posts on a MessageChannel, which Playwright's clock does not
//   own; each frame waits for the tasks already posted before and after its layout.
// - The browser delivers ResizeObserver notifications in its real rendering steps, so the page
//   gets an observer of the same interface that this script notifies after each frame's
//   layout instead (`resizes` below). The page's observers re-measure for themselves and
//   read nothing from the entries but `target`.
// Every font face is loaded before counting starts, so no font arrives during a run.
//
// The metrics:
// - `renders`: function and class components React rendered, summed over commits. Read from
//   React's DevTools hook, which production and development builds both call; a component
//   counts when its fiber has React's PerformedWork flag or was just mounted, as DevTools
//   itself decides.
// - `commits`: React commits.
// - `mutations`: DOM mutation records (attributes, children, text) under the document.
// - `styled`: elements whose style Chromium recalculated (`UpdateLayoutTree`'s elementCount).
// - `laidOut`: layout objects Chromium laid out (`Layout`'s dirtyObjects).
// - `styles` and `layouts`: style and layout passes (CDP's RecalcStyleCount, LayoutCount).
import { writeFile } from "node:fs/promises";
import { chromium } from "playwright";
import { option } from "./moments.mjs";
import { serveRecordings } from "./fixture-views.mjs";
import { takeSlot } from "./slots.mjs";

const FRAME = 1000 / 60;
/** Frames played after the stage goes idle, for Motion's springs and timers to finish. */
const TAIL = 60;
/** The most frames one view may take: a stage busy longer than this is stuck. */
const MOST = 3000;

/** Installs, before the page's scripts run, the counters that live in the page. */
function counters() {
    const tally = { renders: 0, commits: 0, mutations: 0 };
    const components = new Map();
    window.berylCounts = { tally, components };
    // React's function, class, forwardRef and memo components (ReactWorkTags).
    const counted = new Set([0, 1, 11, 14, 15]);
    const PerformedWork = 1;
    const nameOf = (fiber) => {
        const type = fiber.type?.render ?? fiber.type?.type ?? fiber.type;
        return type?.displayName ?? type?.name ?? "(anonymous)";
    };
    // A fiber whose child list is the one it had before the commit was not reconciled, so
    // nothing under it rendered: DevTools skips such subtrees the same way.
    const walk = (next, prev) => {
        if (counted.has(next.tag) && (!prev || next.flags & PerformedWork)) {
            tally.renders += 1;
            const name = nameOf(next);
            components.set(name, (components.get(name) ?? 0) + 1);
        }
        if (prev && next.child === prev.child) return;
        for (let child = next.child; child; child = child.sibling) {
            walk(child, child.alternate);
        }
    };
    window.__REACT_DEVTOOLS_GLOBAL_HOOK__ = {
        supportsFiber: true,
        isDisabled: false,
        renderers: new Map(),
        inject() {
            return 1;
        },
        checkDCE() {},
        onScheduleFiberRoot() {},
        onCommitFiberUnmount() {},
        onPostCommitFiberRoot() {},
        onCommitFiberRoot(_id, root) {
            tally.commits += 1;
            walk(root.current, root.current.alternate);
        },
    };
    // ResizeObserver, notified by `settle` when an observed element's border box changed
    // since it was last notified, and once when first observed, as the real one is.
    const observers = new Set();
    window.ResizeObserver = class {
        #callback;
        #sizes = new Map();
        constructor(callback) {
            this.#callback = callback;
        }
        observe(target) {
            if (!this.#sizes.has(target)) this.#sizes.set(target, undefined);
            observers.add(this);
        }
        unobserve(target) {
            this.#sizes.delete(target);
        }
        disconnect() {
            this.#sizes.clear();
            observers.delete(this);
        }
        /** Calls back with the targets whose size changed; whether any did. */
        notify() {
            const entries = [];
            for (const [target, last] of this.#sizes) {
                const box = target.getBoundingClientRect();
                const size = `${box.width}x${box.height}`;
                if (size === last) continue;
                this.#sizes.set(target, size);
                entries.push({ target, contentRect: box });
            }
            if (entries.length) this.#callback(entries, this);
            return entries.length > 0;
        }
    };
    window.berylCountsResizes = () => {
        // The real observer stops after notifications that keep resizing; so does this.
        for (let depth = 0; depth < 8; depth++) {
            let any = false;
            for (const observer of [...observers])
                any = observer.notify() || any;
            if (!any) return;
        }
    };
    new MutationObserver((records) => {
        tally.mutations += records.length;
    }).observe(document, {
        subtree: true,
        attributes: true,
        childList: true,
        characterData: true,
    });
}

/** Sums the style and layout work in a Chromium trace. */
function traced(buffer) {
    const parsed = JSON.parse(buffer.toString("utf8"));
    const events = Array.isArray(parsed) ? parsed : parsed.traceEvents;
    let styled = 0;
    let laidOut = 0;
    for (const e of events) {
        if (e.name === "UpdateLayoutTree" && e.ph === "X") {
            styled += e.args?.elementCount ?? 0;
        } else if (e.name === "Layout" && (e.ph === "X" || e.ph === "B")) {
            laidOut += e.args?.beginData?.dirtyObjects ?? 0;
        }
    }
    return { styled, laidOut };
}

/** One frame's end in the page: React's posted tasks run, CSS animations finished, a forced
 * style and layout, and the resize notifications that layout calls for. Whether the stage is
 * still playing. It runs in the page, so it defines what it calls. */
async function settle() {
    /** Finishes the running CSS animations and transitions, and forces a style and layout, until
     * that layout starts none: a transition left running would tick in the browser's real frames
     * between this script's. */
    function finished() {
        for (let round = 0; round < 8; round++) {
            void document.documentElement.getBoundingClientRect();
            const running = document
                .getAnimations()
                .filter((animation) => animation.playState === "running");
            if (running.length === 0) return;
            for (const animation of running) {
                try {
                    animation.finish();
                } catch {
                    // An infinite animation cannot finish; it keeps its current frame.
                    animation.pause();
                }
            }
        }
    }
    const drained = () =>
        new Promise((resolve) => {
            const channel = new MessageChannel();
            channel.port1.onmessage = () => resolve();
            channel.port2.postMessage(undefined);
        });
    await drained();
    finished();
    window.berylCountsResizes();
    await drained();
    finished();
    return Boolean(window.berylStage?.shown.busy);
}

async function count(browser, base, fixtures, name, { phone = false } = {}) {
    const context = await browser.newContext({
        viewport: phone
            ? { width: 393, height: 759 }
            : { width: 1600, height: 900 },
        deviceScaleFactor: 2,
    });
    await serveRecordings(context, fixtures);
    await context.addInitScript(counters);
    const page = await context.newPage();
    // An installed clock still follows real time until it is paused; paused, it moves only
    // in `runFor`. The date is fixed too, so nothing the page derives from it varies.
    const start = new Date("2026-01-01T00:00:00Z");
    await page.clock.install({ time: start });
    await page.clock.pauseAt(new Date(start.getTime() + 1000));
    const errors = [];
    page.on("pageerror", (e) => errors.push(e.message));
    await page.goto(
        new URL(`/?${phone ? "phone&" : ""}fixture=${name}&at=1#/game`, base)
            .href,
    );
    // Loading runs on real tasks (module fetches, fonts); the fake clock steps only once the
    // stage and the fixture transport exist.
    for (let i = 0; i < MOST; i++) {
        await page.clock.runFor(FRAME);
        if (await page.evaluate(() => window.berylStage && window.berylFixture))
            break;
    }
    await page.evaluate(() =>
        Promise.all(
            [...document.fonts].map((face) =>
                face.load().catch(() => undefined),
            ),
        ).then(() => undefined),
    );
    const frames = async () => {
        let played = 0;
        let quiet = 0;
        while (quiet < TAIL) {
            if (played++ > MOST)
                throw new Error(
                    `${name}: the stage stayed busy for ${MOST} frames`,
                );
            await page.clock.runFor(FRAME);
            quiet = (await page.evaluate(settle)) ? 0 : quiet + 1;
        }
        return played;
    };
    // The opening view plays before counting starts: its work is the page's load.
    await frames();
    const cdp = await context.newCDPSession(page);
    await cdp.send("Performance.enable");
    const metric = async () =>
        Object.fromEntries(
            (await cdp.send("Performance.getMetrics")).metrics.map((m) => [
                m.name,
                m.value,
            ]),
        );
    await browser.startTracing(page, {
        categories: [
            "devtools.timeline",
            "disabled-by-default-devtools.timeline",
        ],
    });
    await page.evaluate(() => {
        const { tally, components } = window.berylCounts;
        for (const key of Object.keys(tally)) tally[key] = 0;
        components.clear();
    });
    const before = await metric();
    let played = 0;
    let views = 0;
    // Answers each decision as recorded, as perf.mjs does, until an answer releases no new
    // view. The view is compared inside the page: a copy brought out to Node is never
    // identical to it.
    for (;;) {
        await page.evaluate(() => {
            const replay = window.berylFixture;
            window.berylCountsView = replay.view();
            window.berylCountsSent = replay
                .invoke("choose", { request: {} })
                .then(() => undefined);
        });
        played += await frames();
        const ended = await page.evaluate(async () => {
            await window.berylCountsSent;
            return window.berylFixture.view() === window.berylCountsView;
        });
        if (ended) break;
        views += 1;
    }
    const after = await metric();
    const { styled, laidOut } = traced(await browser.stopTracing());
    const { tally, components } = await page.evaluate(() => ({
        tally: window.berylCounts.tally,
        components: [...window.berylCounts.components],
    }));
    await context.close();
    const delta = (key) => Math.round((after[key] ?? 0) - (before[key] ?? 0));
    return {
        counts: {
            renders: tally.renders,
            commits: tally.commits,
            mutations: tally.mutations,
            styled,
            laidOut,
            styles: delta("RecalcStyleCount"),
            layouts: delta("LayoutCount"),
        },
        views,
        frames: played,
        components: components.sort((a, b) => b[1] - a[1]),
        errors,
    };
}

const METRICS = [
    "renders",
    "commits",
    "mutations",
    "styled",
    "laidOut",
    "styles",
    "layouts",
];

const args = process.argv.slice(2);
const json = option(args, "--json");
const components = option(args, "--components", true);
const layouts = (option(args, "--layouts") ?? "desktop").split(",");
const [base, fixtures, ...names] = args;
if (
    !base ||
    !fixtures ||
    names.length === 0 ||
    layouts.some((l) => l !== "desktop" && l !== "phone")
) {
    console.error(
        "usage: node scripts/counts.mjs <base-url> <fixtures-dir> [--json out] [--components] [--layouts desktop,phone] name...",
    );
    process.exit(2);
}
// The full Chromium, as perf.mjs launches it: the app's webview on Windows is Chromium, and
// only Chromium reports the style and layout counts.
const browser = await chromium.launch({ channel: "chromium" });
const out = {};
let failed = false;
console.log(
    `${"fixture".padEnd(26)} views frames ${METRICS.map((m) => m.padStart(9)).join(" ")}`,
);
for (const name of names) {
    for (const layout of layouts) {
        const key = `${name}/${layout}`;
        // Each play holds one of the machine's browser slots while its page is open.
        const release = await takeSlot();
        const r = await count(browser, base, fixtures, name, {
            phone: layout === "phone",
        }).finally(release);
        out[key] = r.counts;
        console.log(
            `${key.padEnd(26)} ${String(r.views).padStart(5)} ${String(r.frames).padStart(6)} ${METRICS.map((m) => String(r.counts[m]).padStart(9)).join(" ")}`,
        );
        if (components) {
            for (const [component, n] of r.components.slice(0, 15))
                console.log(`    ${String(n).padStart(7)}  ${component}`);
        }
        for (const e of r.errors) {
            failed = true;
            console.log(`  error: ${e}`);
        }
    }
}
await browser.close();
if (json) await writeFile(json, `${JSON.stringify(out, null, 2)}\n`);
process.exit(failed ? 1 : 0);
