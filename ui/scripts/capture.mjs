// What the headless capture scripts share: opening a page at a fixed size and waiting until it has
// settled.
import { hangContext } from "./hang.mjs";
import { createHash } from "node:crypto";
import { appendFileSync, mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { chromium, webkit } from "playwright";

import { takeSlot } from "./slots.mjs";

/** The engine of the app's webview on this platform, as `by` names it (`BERYL_BROWSER`):
 * WebKit (WKWebView, macOS) unless Chromium (WebView2, Windows). */
export const launch = () =>
    (process.env.BERYL_BROWSER === "chromium" ? chromium : webkit).launch();

/** Reports one check's result to the run log (tools/src/beryl_tools/runlog.py) when `by check`
 * or `by ci` asked for results (`BERYL_CHECK_RESULTS`), to Ignite when it asked
 * (`IGNITE_RESULTS`, docs/ignite.md "Reporting results"), and what its page ran (`covered`,
 * from `coverage`) when `by coverage` asked for that; otherwise does nothing. `failure` is
 * what failed, from `failureOf`, which Ignite's line carries. A log that cannot be written
 * fails no check. */
export function recordResult(
    check,
    outcome,
    seconds,
    covered = null,
    failure = {},
) {
    const path = process.env.BERYL_CHECK_RESULTS;
    const line = { check, outcome, seconds: Math.round(seconds * 1000) / 1000 };
    try {
        if (path) appendFileSync(path, `${JSON.stringify(line)}\n`);
        const ignite = process.env.IGNITE_RESULTS;
        if (ignite) {
            const told = outcome === "fail" ? failure : {};
            appendFileSync(
                ignite,
                `${JSON.stringify({ check, outcome, ...told })}\n`,
            );
        }
        const folder = process.env.BERYL_COVERAGE;
        if (folder) {
            appendFileSync(
                join(folder, `checks-${process.pid}.jsonl`),
                `${JSON.stringify({ ...line, ...covered })}\n`,
            );
        }
    } catch (e) {
        console.error(`run log: not written: ${e.message}`);
    }
}

/** A place in the UI's source, `ui/<path>:<line>`, in a stack or message: a module the dev
 * server served the page (`http://127.0.0.1:5173/src/stage/x.ts?t=1:212:7`) or a script of
 * this folder (`file:///…/ui/scripts/kept-x.mjs:40:5`), or a place `pageError` already wrote
 * (`(at ui/src/stage/x.ts:212)`). */
const SOURCE_PLACE =
    /(?:https?:\/\/[^/\s]+\/|(?:^|[/\s(])ui\/)((?:src|scripts)\/[^?:\s()]+)(?:\?[^:\s()]*)?:(\d+)/;

/** The first place in the UI's source that `text` names, as `ui/<path>:<line>`; null if none. */
export function sourcePlace(text) {
    const found = SOURCE_PLACE.exec(text ?? "");
    return found ? `ui/${found[1]}:${found[2]}` : null;
}

/** A page error as a check reports it: its message, with the place in the app's source it was
 * thrown from where its stack names one. */
export function pageError(e) {
    const place = sourcePlace(e.stack);
    return place ? `${e.message} (at ${place})` : e.message;
}

/** The longest message a result carries to Ignite. */
const MESSAGE_LIMIT = 300;

/** What failed in a check, for `recordResult`: the first of `texts` (the page's errors, then
 * what the check threw, each a string or an error) as the message, and the first place in the
 * UI's source any of them names. */
export function failureOf(...texts) {
    const said = texts
        .flat()
        .filter(Boolean)
        .map((t) => (t instanceof Error ? (t.stack ?? t.message) : String(t)));
    if (said.length === 0) return {};
    const first = said[0].split("\n")[0].replace(/\s+/g, " ").trim();
    const message =
        first.length <= MESSAGE_LIMIT
            ? first
            : `${first.slice(0, MESSAGE_LIMIT - 1)}…`;
    const at = said.map(sourcePlace).find(Boolean);
    return at ? { message, at } : { message };
}

/** The sources `coverage` has saved in this process, by their file names. */
const saved = new Set();

/** Saves a script's or a stylesheet's text, as the page ran it, into `by coverage`'s folder once
 * per process: under `url` (a module's path on the dev server) or `file` (a stylesheet's path on
 * disk). Returns the file name the coverage records refer to it by. */
function saveSource(folder, where, text) {
    const name = `${createHash("sha1").update(JSON.stringify(where)).digest("hex")}.json`;
    if (!saved.has(name)) {
        mkdirSync(join(folder, "sources"), { recursive: true });
        writeFileSync(
            join(folder, "sources", name),
            JSON.stringify({ ...where, text }),
        );
        saved.add(name);
    }
    return name;
}

/** When `by coverage` asks (`BERYL_COVERAGE`, a folder; Chromium only), records what `page`
 * runs of the app's own code: the byte ranges of each module under src/ that ran (V8's block
 * coverage, nested, each with whether it ran), the rules of each stylesheet that matched, and
 * the fixtures the page fetched. Returns a function, to call before the page closes, that stops
 * recording and returns what ran, for `recordResult`; null when nobody asked, or when the page
 * could not tell. tools/src/beryl_tools/coverage.py maps the ranges to source lines. */
export async function coverage(page) {
    const folder = process.env.BERYL_COVERAGE;
    if (!folder) return async () => null;
    const fixtures = new Set();
    page.on("request", (r) => {
        const m = /\/fixtures\/([^/]+)\.json$/.exec(new URL(r.url()).pathname);
        if (m) fixtures.add(decodeURIComponent(m[1]));
    });
    // Modules import their CSS as <style> elements with no URL, which Playwright's CSS coverage
    // leaves out; the DevTools protocol reports their rules, and Vite names each element's file.
    const cdp = await page.context().newCDPSession(page);
    const sheets = new Map();
    cdp.on("CSS.styleSheetAdded", ({ header }) =>
        sheets.set(header.styleSheetId, header),
    );
    await cdp.send("DOM.enable");
    await cdp.send("CSS.enable");
    await cdp.send("CSS.startRuleUsageTracking");
    await page.coverage.startJSCoverage({ resetOnNavigation: false });
    return async () => {
        try {
            const js = [];
            for (const entry of await page.coverage.stopJSCoverage()) {
                const url = URL.canParse(entry.url) ? new URL(entry.url) : null;
                // A stylesheet's or a JSON file's module only hands its text to the page.
                if (!url?.pathname.startsWith("/src/")) continue;
                if (/\.(css|json)$/.test(url.pathname)) continue;
                js.push({
                    source: saveSource(
                        folder,
                        { url: url.pathname },
                        entry.source,
                    ),
                    ranges: entry.functions.flatMap((f) =>
                        f.ranges.map((r) => [
                            r.startOffset,
                            r.endOffset,
                            r.count > 0 ? 1 : 0,
                        ]),
                    ),
                });
            }
            const { ruleUsage } = await cdp.send("CSS.stopRuleUsageTracking");
            const used = Map.groupBy(
                ruleUsage.filter((r) => r.used),
                (r) => r.styleSheetId,
            );
            const css = [];
            for (const [id, rules] of used) {
                const owner = sheets.get(id)?.ownerNode;
                if (!owner) continue;
                const { node } = await cdp.send("DOM.describeNode", {
                    backendNodeId: owner,
                });
                const attributes = node.attributes ?? [];
                const at = attributes.indexOf("data-vite-dev-id");
                if (at < 0) continue;
                const { text } = await cdp.send("CSS.getStyleSheetText", {
                    styleSheetId: id,
                });
                css.push({
                    source: saveSource(
                        folder,
                        { file: attributes[at + 1] },
                        text,
                    ),
                    ranges: rules.map((r) => [r.startOffset, r.endOffset, 1]),
                });
            }
            return { fixtures: [...fixtures].sort(), js, css };
        } catch (e) {
            console.error(`coverage: not taken: ${e.message}`);
            return null;
        }
    };
}

/** Runs the tasks it is handed at most `BERYL_UI_WORKERS` (default `limit`) at a time, each on
 * its own browser context and holding the requested browser slots (one by default) while it
 * runs. Awaiting a hand-off waits for a free worker and slot, not for the task, so a script
 * reads on while its pages play; `drain` waits for every task. A task must not throw. */
export function workers(limit = 4) {
    limit = Number(process.env.BERYL_UI_WORKERS) || limit;
    const running = new Set();
    const run = async (task, pages = 1) => {
        while (running.size >= limit) await Promise.race(running);
        const release = await takeSlot(process.env, pages);
        const done = task().finally(() => {
            release();
            running.delete(done);
        });
        running.add(done);
    };
    run.drain = () => Promise.all(running);
    return run;
}

/** A page at `width`×`height` with device scale factor 1 and motion reduced, collecting the
 * page's errors and console errors into `errors`. */
export async function openPage(browser, { width, height, scale = 1 }) {
    const context = await hangContext(browser, {
        viewport: { width, height },
        deviceScaleFactor: scale,
        reducedMotion: "reduce",
    });
    const page = await context.newPage();
    const errors = [];
    page.on("pageerror", (e) => errors.push(pageError(e)));
    page.on("console", (m) => m.type() === "error" && errors.push(m.text()));
    return { context, page, errors };
}

/** Waits until the app has rendered (a lazily loaded screen shows nothing before its chunk
 * arrives), fonts and images have loaded, finite animations are done and two frames were drawn
 * after them, so measured layout (arrows) is final. */
export async function settle(page) {
    await page.waitForFunction(
        () => (document.getElementById("root")?.childElementCount ?? 0) > 0,
    );
    await page.evaluate(async () => {
        await document.fonts.ready;
        await Promise.all(
            [...document.images].map((i) =>
                i.complete ? undefined : i.decode().catch(() => undefined),
            ),
        );
        const finite = document
            .getAnimations()
            .filter(
                (a) => a.effect?.getComputedTiming().iterations !== Infinity,
            );
        await Promise.all(finite.map((a) => a.finished.catch(() => undefined)));
        await new Promise((r) =>
            requestAnimationFrame(() => requestAnimationFrame(r)),
        );
    });
}

/** Waits until every card holds still: the stage writes a card's place to its transform from
 * its own clock's frames (src/stage/clock.ts), after the render that brought the view, and
 * `settle` cannot see that. Done once the cards' transforms are the same three frames running.
 * `timeout` only guards against a hang: a loaded machine stretches a script's play well past
 * its stage time, so the default is far above a healthy wait. */
export async function still(page, timeout = 120000) {
    await page.evaluate(async (timeout) => {
        const frame = () => new Promise((r) => requestAnimationFrame(r));
        const sample = () =>
            [...document.querySelectorAll(".card-el")]
                .map((e) => e.style.transform)
                .join("|");
        const end = performance.now() + timeout;
        let last = sample();
        let same = 0;
        while (same < 3 && performance.now() < end) {
            await frame();
            const now = sample();
            same = now === last ? same + 1 : 0;
            last = now;
        }
    }, timeout);
}

/** Crops each element (a locator, or a selector) out of `page` at its size on screen, with
 * `pad` px around it, and lays the crops side by side, labelled, in one image saved to `path`:
 * for judging whether two states, or two things that should look alike, can be told apart at
 * a glance. Returns the targets not on screen. */
export async function compareSheet(page, targets, path, { pad = 12 } = {}) {
    const crops = [];
    for (const target of targets) {
        const locator =
            typeof target === "string" ? page.locator(target).first() : target;
        const box = await locator.boundingBox();
        if (!box) {
            crops.push({ label: String(target), missing: true });
            continue;
        }
        const view = page.viewportSize();
        const x = Math.max(0, box.x - pad);
        const y = Math.max(0, box.y - pad);
        const clip = {
            x,
            y,
            width: Math.min(view.width, box.x + box.width + pad) - x,
            height: Math.min(view.height, box.y + box.height + pad) - y,
        };
        const png = await page.screenshot({ clip });
        crops.push({
            label:
                typeof target === "string"
                    ? target
                    : await locator.evaluate(
                          (e) =>
                              e.getAttribute("data-card") ??
                              e.getAttribute("aria-label") ??
                              e.tagName.toLowerCase(),
                      ),
            src: `data:image/png;base64,${png.toString("base64")}`,
        });
    }
    const sheet = await page.context().newPage();
    await sheet.setContent(
        `<body style="margin:0;padding:12px;background:#777;display:inline-flex;gap:16px;align-items:flex-start;font:13px sans-serif">${crops
            .map(
                (c) =>
                    `<figure style="margin:0"><figcaption style="color:#fff;margin-bottom:4px">${c.label.replace(/</g, "&lt;")}</figcaption>${
                        c.missing ? "(not on screen)" : `<img src="${c.src}">`
                    }</figure>`,
            )
            .join("")}</body>`,
    );
    await sheet.locator("body").screenshot({ path });
    await sheet.close();
    return crops.filter((c) => c.missing).map((c) => c.label);
}

/** `cutText` once the page has held still: Motion's JavaScript-driven springs are not among
 * the animations `settle` waits for, so a cut is reported only if it is still there after a
 * few more settles. On a page whose clock Playwright drives, call `cutText` instead. */
export async function clippedText(page) {
    let found = [];
    for (let tries = 0; tries < 5; tries++) {
        if (tries > 0) {
            await page.waitForTimeout(200);
            await settle(page);
        }
        found = await cutText(page);
        if (!found.length) break;
    }
    return found;
}

/** Every visible text on the page that does not show in full: text overflowing its own
 * element (an ellipsis, a scrollWidth past its clientWidth), or straddling the edge of an
 * ancestor that clips it (overflow) or frames it (a border or a background), as a label
 * wrapping out of a fixed-height pill. Text wholly outside a frame is a label beside it, and a
 * line's glyph box may stand a little proud of its bar, so vertically only a fraction of a
 * line counts. A scrolling ancestor ends the search, since text scrolled out of view is
 * reachable, and so does a card's frame: a card may lie partly off the table or under another.
 * Text not on a card is also cut where a card on the table lies over it, as permanents once hid
 * the game log. Each is described by its text and how it is cut, for a failure message. It
 * first waits for the fonts: before they load, text shows in a fallback with other metrics (a
 * P/T's numerals standing out of their plaque), as on a machine busy enough to slow them. */
export function cutText(page) {
    return page.evaluate(async () => {
        // Laid out first, so every font the page shows has begun to load.
        void document.body.offsetWidth;
        await document.fonts.ready;
        const CARD = ".card-slot, .card-back";
        const scrolls = (v) => v === "auto" || v === "scroll";
        const boxed = (s) =>
            s.backgroundImage !== "none" ||
            !/rgba\(.*, 0\)|transparent/.test(s.backgroundColor) ||
            parseFloat(s.borderTopWidth) > 0 ||
            parseFloat(s.borderBottomWidth) > 0;
        /** Whether `t` crosses an edge of `b` along an axis by more than `slack`. */
        const straddles = (t, b, [lo, hi], slack) =>
            t[hi] > b[lo] + slack &&
            t[lo] < b[hi] - slack &&
            (t[lo] < b[lo] - slack || t[hi] > b[hi] + slack);
        /** Whether `el` shows turned an odd number of quarters, by its and its ancestors'
         * transforms and rotations. */
        const sideways = (el) => {
            let degrees = 0;
            for (let a = el; a; a = a.parentElement) {
                const s = getComputedStyle(a);
                if (s.transform !== "none") {
                    const m = new DOMMatrix(s.transform);
                    degrees += (Math.atan2(m.b, m.a) * 180) / Math.PI;
                }
                if (s.rotate && s.rotate !== "none") {
                    degrees += parseFloat(s.rotate.split(" ").pop());
                }
            }
            return Math.abs(Math.round(degrees / 90)) % 2 === 1;
        };
        const found = [];
        for (const el of document.querySelectorAll("#root *")) {
            const nodes = [...el.childNodes].filter(
                (n) => n.nodeType === Node.TEXT_NODE && n.textContent.trim(),
            );
            if (!nodes.length || !el.getClientRects().length) continue;
            const style = getComputedStyle(el);
            if (style.visibility !== "visible") continue;
            let shown = true;
            for (let a = el; a && shown; a = a.parentElement) {
                shown = parseFloat(getComputedStyle(a).opacity) > 0;
            }
            if (!shown) continue;
            // A symbol's glyph is a private character: its label names it.
            const text =
                el.getAttribute("aria-label") ??
                nodes
                    .map((n) => n.textContent.trim())
                    .join(" ")
                    .slice(0, 80);
            if (
                style.display !== "inline" &&
                el.scrollWidth > el.clientWidth + 1
            ) {
                // Card notes intentionally show a single line. The inspector shows the full text.
                if (
                    !el.matches(".card-note") ||
                    style.textOverflow !== "ellipsis"
                ) {
                    found.push(`"${text}" overflows its box`);
                }
                continue;
            }
            const range = document.createRange();
            range.setStartBefore(nodes[0]);
            range.setEndAfter(nodes[nodes.length - 1]);
            const t = range.getBoundingClientRect();
            if (t.width === 0 || t.height === 0) continue;
            // A line's glyph box stands proud across the line, which on a card turned a
            // quarter (tapped) runs across the screen's x axis, not its y.
            const line = 0.3 * parseFloat(style.fontSize);
            const [slackX, slackY] = sideways(el) ? [line, 1] : [1, line];
            let framed = false;
            for (
                let a = el.parentElement;
                a && a.id !== "root";
                a = a.parentElement
            ) {
                const s = getComputedStyle(a);
                const [x, y] = [s.overflowX, s.overflowY];
                const clips = x !== "visible" || y !== "visible";
                const frames = !framed && boxed(s);
                if (clips || frames) {
                    const b = a.getBoundingClientRect();
                    const cut =
                        (!scrolls(x) &&
                            straddles(t, b, ["left", "right"], slackX)) ||
                        (!scrolls(y) &&
                            straddles(t, b, ["top", "bottom"], slackY));
                    if (cut) {
                        const what = `${a.tagName.toLowerCase()}.${[...a.classList].join(".")}`;
                        found.push(`"${text}" is cut by ${what}`);
                        break;
                    }
                    framed ||= frames;
                }
                if (scrolls(x) || scrolls(y) || a.matches(CARD)) break;
            }
            // Text of the table's own (the log, a badge, a banner) that a permanent or a hand
            // card lies over, sampled at each line's ends and middle. Text on the cards and the
            // stack's items, which overlap by design, and on the inspector, drawn over the cards
            // but letting the pointer through to them, is not checked. Nor is text that takes
            // no pointer and is painted over the card under it (`paintedOver`), as a banner
            // passing over a phone's rows is. The one text of a card that is checked is the mana
            // cost of a card in your hand: the cards of a crowded hand cover each other's right
            // parts, and every cost must still show, as Arena's do above its fan.
            if (
                el.closest(".card-el, .stack-item, .inspect") &&
                !el.closest(".hand-cost")
            )
                continue;
            // Whether `text`, which lets the pointer through, is what shows at a point a card
            // was hit at: with the pointer given back to it, the hit is its own.
            const paintedOver = (text, x, y) => {
                if (getComputedStyle(text).pointerEvents !== "none")
                    return false;
                const before = text.style.pointerEvents;
                text.style.pointerEvents = "auto";
                const hit = document.elementFromPoint(x, y);
                text.style.pointerEvents = before;
                return hit !== null && text.contains(hit);
            };
            // Only where the text shows: not scrolled out of a list's view.
            let view = {
                left: 0,
                top: 0,
                right: innerWidth,
                bottom: innerHeight,
            };
            for (let a = el.parentElement; a; a = a.parentElement) {
                const s = getComputedStyle(a);
                if (s.overflowX === "visible" && s.overflowY === "visible")
                    continue;
                const b = a.getBoundingClientRect();
                view = {
                    left: Math.max(view.left, b.left),
                    top: Math.max(view.top, b.top),
                    right: Math.min(view.right, b.right),
                    bottom: Math.min(view.bottom, b.bottom),
                };
            }
            // A line is sampled at its ends and middle, halfway up. A mana symbol of a hand
            // card's cost is a disc far taller than its glyph, and a neighboring card tilted
            // in the fan reaches a disc's lower corner first, so the disc is sampled on a
            // grid over its box, inset a fifth so each point lies inside the disc.
            const points = [];
            const disc = el.matches(".hand-cost .mana-symbol")
                ? el.getBoundingClientRect()
                : undefined;
            if (disc) {
                for (const fy of [0.2, 0.5, 0.8]) {
                    for (const fx of [0.2, 0.5, 0.8]) {
                        points.push([
                            disc.left + fx * disc.width,
                            disc.top + fy * disc.height,
                        ]);
                    }
                }
            } else {
                for (const r of range.getClientRects()) {
                    const y = (r.top + r.bottom) / 2;
                    for (const x of [
                        r.left + 2,
                        (r.left + r.right) / 2,
                        r.right - 2,
                    ]) {
                        points.push([x, y]);
                    }
                }
            }
            covered: {
                for (const [x, y] of points) {
                    if (y < view.top || y > view.bottom) continue;
                    if (x < view.left || x > view.right) continue;
                    // A hit outside the element's own box is an unseen pseudo-element's.
                    const hit = document.elementFromPoint(x, y);
                    const b = hit?.getBoundingClientRect();
                    const inside =
                        b &&
                        x >= b.left &&
                        x <= b.right &&
                        y >= b.top &&
                        y <= b.bottom;
                    const over = inside && hit.closest(".card-layer .card-el");
                    // A card a prompt holds up lies over the tray's shade, which says that
                    // the table under it is not what is being read (src/stage/Tray.tsx).
                    const heldUp =
                        over &&
                        document.querySelector(".stage-tray") !== null &&
                        Number(over.style.zIndex) >= 850 &&
                        Number(over.style.zIndex) < 900;
                    // A card of the hand raised under the pointer (stage/dom.ts `raise`)
                    // covers what it likes until the pointer leaves it, as Arena's does:
                    // it is hover inspection, not where the layout put it.
                    const raised =
                        over &&
                        over.closest(".stage-node[data-raised]") !== null;
                    if (
                        over &&
                        !heldUp &&
                        !raised &&
                        !over.contains(el) &&
                        !paintedOver(el, x, y)
                    ) {
                        found.push(
                            `"${text}" lies under ${over.dataset.card ?? "a card"}`,
                        );
                        break covered;
                    }
                }
            }
        }
        return found;
    });
}

export { screenFaults } from "./screen-faults.mjs";

/** What a node of the stage (src/stage/dom.ts) carries that only the stage's clock may give it:
 * a CSS transition or animation, or a Web Animation, any of which would move it on the
 * browser's time, where no test can step it and nothing sequences it (src/stage/stage.ts).
 * Each is described for a failure message. Nothing on a page without the stage. */
export async function stageFaults(page) {
    return page.evaluate(() => {
        const faults = [];
        const timed = (durations) =>
            durations.split(",").some((d) => parseFloat(d) > 0);
        for (const node of document.querySelectorAll(".stage-node")) {
            const what = `the stage's node ${node.dataset.stage}`;
            const style = getComputedStyle(node);
            if (
                style.transitionProperty !== "none" &&
                timed(style.transitionDuration)
            ) {
                faults.push(`${what} has a CSS transition`);
            }
            if (style.animationName !== "none") {
                faults.push(`${what} has a CSS animation`);
            }
            if (node.getAnimations().length > 0) {
                faults.push(`${what} has an animation running`);
            }
        }
        return faults;
    });
}

/** The controls a player can't fully reach: a button or other control a click reaches (no
 * modal covers its middle) lying partly outside the window, or overlapping another such
 * control that is neither inside it nor around it, so that a click meant for one may land on
 * the other; or one of the table's fixtures (a step stop, a life orb, the dock's and the
 * corners' buttons) that a permanent or a spell on the stack lies over or under, as lands
 * crowding a life orb's stops on a narrow stage would; or what the open prompt asks lying
 * over a permanent. A menu or a screen opened over the
 * table (a dialog's buttons too), and a pile's own cards under its button, are meant to
 * overlap them. Cards may overlap
 * one another, as a fan or a pile does. Each is described for a failure message. */
export async function crowdedControls(page) {
    return page.evaluate(() => {
        const name = (e) =>
            e.getAttribute("aria-label") ||
            e.textContent?.trim().slice(0, 30) ||
            e.className ||
            e.tagName.toLowerCase();
        const faults = [];
        // The permanents and spells, by their frames' boxes (a card's element may reach past
        // its frame).
        const cards = [
            ...document.querySelectorAll(
                ".card-layer .card-el:is([data-zone=battlefield], [data-zone=stack])",
            ),
        ]
            .filter((c) => {
                const s = getComputedStyle(c);
                return s.visibility !== "hidden" && Number(s.opacity) > 0;
            })
            .map((c) => ({
                card: c,
                box: (
                    c.querySelector(".card-slot, .card-back") ?? c
                ).getBoundingClientRect(),
            }));
        const FIXTURES =
            ".turn-controls :is(button, [role=button]), .life-target, .dock button, .phone-dock button, .icon-button";
        /** A card whose frame lies over or under the fixture `e`'s `box` by more than a sliver:
         * either way the two collide. */
        const cardOver = (e, box) =>
            e.matches(FIXTURES) && !e.closest("[role=dialog]")
                ? cards.find(({ box: c }) => {
                      const w =
                          Math.min(box.right, c.right) -
                          Math.max(box.left, c.left);
                      const h =
                          Math.min(box.bottom, c.bottom) -
                          Math.max(box.top, c.top);
                      return w > 3 && h > 3;
                  })?.card
                : undefined;
        const controls = [
            ...document.querySelectorAll(
                "button, [role=button], [role=menu], a[href], input, select, .life-target",
            ),
        ].filter((e) => {
            const box = e.getBoundingClientRect();
            const style = getComputedStyle(e);
            if (
                box.width <= 0 ||
                box.height <= 0 ||
                style.visibility === "hidden" ||
                Number(style.opacity) <= 0 ||
                e.closest("[aria-hidden=true], .card-el")
            ) {
                return false;
            }
            const card = cardOver(e, box);
            if (card) {
                faults.push(
                    `${name(e)} collides with ${card.dataset.card ?? "a card"}`,
                );
                return false;
            }
            // The entire panel must fit, including when its middle is off screen.
            if (e.matches("[role=menu]")) return true;
            const hit = document.elementFromPoint(
                box.left + box.width / 2,
                box.top + box.height / 2,
            );
            return hit !== null && (e.contains(hit) || hit.contains(e));
        });
        // Prompts and proposal panels must leave the cards they refer to visible.
        // A prompt under the tray's shade (src/stage/Tray.tsx) may cover the table.
        const shaded = document.querySelector(".stage-tray") !== null;
        for (const asks of document.querySelectorAll(
            "[data-asks], [data-table-proposals]",
        )) {
            const proposal = asks.hasAttribute("data-table-proposals");
            if (shaded && !proposal) continue;
            const style = getComputedStyle(asks);
            if (style.visibility === "hidden" || Number(style.opacity) <= 0)
                continue;
            const box = asks.getBoundingClientRect();
            const under = cards.find(({ card, box: c }) => {
                if (!proposal && card.dataset.zone !== "battlefield")
                    return false;
                const w =
                    Math.min(box.right, c.right) - Math.max(box.left, c.left);
                const h =
                    Math.min(box.bottom, c.bottom) - Math.max(box.top, c.top);
                return w > 3 && h > 3;
            })?.card;
            if (under) {
                faults.push(
                    proposal
                        ? `the proposal panel lies over ${under.dataset.card ?? "a card"}`
                        : `what the prompt asks ("${asks.textContent?.trim().slice(0, 30)}") lies over ${under.dataset.card ?? "a permanent"}`,
                );
            }
        }
        const slack = 1;
        // A mark on a player (stage/face-mark.ts) follows the outline of their life orb's face:
        // its rim lies just outside the face's bezel all the way round, and its light stays on
        // the face. A mark drawn as an ellipse about the face once stood 11 units off it above
        // and below, and on the opponent's flipped orb sat 6 units above its middle; before
        // that, a card's halo spread past the orb onto the step stops. Measured in the orb
        // art's units (Player.tsx: 272 wide), a path's box without its stroke.
        for (const mark of document.querySelectorAll(
            "[data-life] > .stage-mark",
        )) {
            const orb = mark.parentElement?.querySelector(".life-orb");
            const face = orb?.querySelector(".life-orb-core");
            if (!orb || !face || Number(getComputedStyle(mark).opacity) <= 0)
                continue;
            const unit = orb.getBoundingClientRect().width / 272;
            const f = face.getBoundingClientRect();
            const m = (
                mark.querySelector(".stage-mark-face-rim") ?? mark
            ).getBoundingClientRect();
            // From the face's outline to the rim's middle, on each side: half the bezel (5) and
            // one more unit.
            const gaps = [
                f.left - m.left,
                f.top - m.top,
                m.right - f.right,
                m.bottom - f.bottom,
            ].map((gap) => gap / unit);
            const off = Math.max(...gaps.map((gap) => Math.abs(gap - 3.5)));
            if (off > 1) {
                faults.push(
                    `${mark.className} on ${mark.parentElement?.dataset.life} stands off its life orb's face by ${gaps.map((gap) => gap.toFixed(1)).join(", ")} units (left, top, right, bottom), not 3.5`,
                );
            }
        }
        const boxes = controls.map((e) => e.getBoundingClientRect());
        controls.forEach((e, i) => {
            const b = boxes[i];
            if (
                b.left < -slack ||
                b.top < -slack ||
                b.right > innerWidth + slack ||
                b.bottom > innerHeight + slack
            ) {
                faults.push(`${name(e)} reaches outside the window`);
            }
            for (let j = i + 1; j < controls.length; j++) {
                const other = controls[j];
                if (e.contains(other) || other.contains(e)) continue;
                const card = (c) => c.closest("[data-card]") !== null;
                if (card(e) && card(other)) continue;
                const o = boxes[j];
                const w = Math.min(b.right, o.right) - Math.max(b.left, o.left);
                const h = Math.min(b.bottom, o.bottom) - Math.max(b.top, o.top);
                if (w > slack && h > slack) {
                    faults.push(`${name(e)} overlaps ${name(other)}`);
                }
            }
        });
        return faults;
    });
}

export { phoneFaults } from "./phone-faults.mjs";
