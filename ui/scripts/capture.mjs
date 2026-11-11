// What the headless capture scripts share: opening a page at a fixed size and waiting until it has
// settled.
import { createHash } from "node:crypto";
import { appendFileSync, mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { chromium, webkit } from "playwright";

/** The engine of the app's webview on this platform, as `by` names it (`BERYL_BROWSER`):
 * WebKit (WKWebView, macOS) unless Chromium (WebView2, Windows). */
export const launch = () =>
    (process.env.BERYL_BROWSER === "chromium" ? chromium : webkit).launch();

/** Reports one check's result to the run log (tools/src/beryl_tools/runlog.py) when `by check`
 * or `by ci` asked for results (`BERYL_CHECK_RESULTS`), and what its page ran (`covered`, from
 * `coverage`) when `by coverage` asked for that; otherwise does nothing. A log that cannot be
 * written fails no check. */
export function recordResult(check, outcome, seconds, covered = null) {
    const path = process.env.BERYL_CHECK_RESULTS;
    const line = { check, outcome, seconds: Math.round(seconds * 1000) / 1000 };
    try {
        if (path) appendFileSync(path, `${JSON.stringify(line)}\n`);
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
 * its own browser context. Awaiting a hand-off waits for a free worker, not for the task, so a
 * script reads on while its pages play; `drain` waits for every task. A task must not throw. */
export function workers(limit = 4) {
    limit = Number(process.env.BERYL_UI_WORKERS) || limit;
    const running = new Set();
    const run = async (task) => {
        while (running.size >= limit) await Promise.race(running);
        const done = task().finally(() => running.delete(done));
        running.add(done);
    };
    run.drain = () => Promise.all(running);
    return run;
}

/** A page at `width`×`height` with device scale factor 1 and motion reduced, collecting the
 * page's errors and console errors into `errors`. */
export async function openPage(browser, { width, height, scale = 1 }) {
    const context = await browser.newContext({
        viewport: { width, height },
        deviceScaleFactor: scale,
        reducedMotion: "reduce",
    });
    const page = await context.newPage();
    const errors = [];
    page.on("pageerror", (e) => errors.push(e.message));
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

/** Waits until every card holds still: Motion writes a card's place to its transform from its
 * own frame loop (CardLayer's useGlide), a frame or two after the render that moved it, even
 * when motion is reduced, and `settle` cannot see that. Done once the cards' transforms are the
 * same three frames running. `timeout` only guards against a hang: a loaded machine stretches
 * the glide well past its half second. */
export async function still(page, timeout = 20000) {
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
            const text = nodes
                .map((n) => n.textContent.trim())
                .join(" ")
                .slice(0, 80);
            if (
                style.display !== "inline" &&
                el.scrollWidth > el.clientWidth + 1
            ) {
                found.push(`"${text}" overflows its box`);
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
            // but letting the pointer through to them, is not checked.
            if (el.closest(".card-el, .stack-item, .inspect")) continue;
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
            covered: for (const r of range.getClientRects()) {
                const y = (r.top + r.bottom) / 2;
                if (y < view.top || y > view.bottom) continue;
                for (const x of [
                    r.left + 2,
                    (r.left + r.right) / 2,
                    r.right - 2,
                ]) {
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
                    if (over && !over.contains(el)) {
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

/** Where the game screen disagrees with the view the fixture last released
 * (`window.berylFixture.view()`, src/ipc.ts), once it has settled: each permanent, spell and
 * card in the viewer's hand drawn exactly once, in its zone, under its name; a permanent's
 * power and toughness (less marked damage), loyalty, counters and orientation; both life
 * totals; and no card drawn that the view does not have.
 * Each is described for a failure message. Nothing on a page without a fixture. */
export async function screenFaults(page) {
    return page.evaluate(() => {
        const view = window.berylFixture?.view?.();
        // A screen other than the game's, as the menu, draws no table to check.
        if (!view || !document.querySelector("[data-life]")) return [];
        const phone = document.querySelector(".phone-board") !== null;
        const faults = [];
        const drawn = new Map();
        for (const e of document.querySelectorAll(".card-el[data-slot]")) {
            const slot = Number(e.dataset.slot);
            drawn.set(slot, [...(drawn.get(slot) ?? []), e]);
        }
        const objects = new Map(view.objects.map((o) => [o.slot, o]));
        const text = (e) => e?.textContent?.replace(/\s+/g, "") ?? "nothing";
        for (const o of view.objects) {
            const shown =
                o.zone === "battlefield" ||
                o.zone === "stack" ||
                (o.zone === "hand" && o.owner === "you");
            if (!shown) continue;
            const what = `${o.name ?? "a hidden card"} (slot ${o.slot}, ${o.zone})`;
            const els = drawn.get(o.slot) ?? [];
            // A resolving spell may be drawn beside the choice it asks rather than on the table:
            // as a choice overlay's source card, known by its name.
            const beside =
                o.zone === "stack" &&
                els.length === 0 &&
                (document.querySelector(`[data-slot="${o.slot}"]`) ||
                    [...document.querySelectorAll(".shown-card")].some(
                        (e) => o.name && e.textContent?.includes(o.name),
                    ));
            if (beside) continue;
            if (els.length !== 1) {
                faults.push(`${what} drawn ${els.length} times`);
                continue;
            }
            const e = els[0];
            if (e.dataset.zone !== o.zone) {
                faults.push(`${what} drawn in the ${e.dataset.zone}`);
            }
            if (o.name && e.dataset.card !== o.name) {
                faults.push(`${what} drawn as ${e.dataset.card ?? "no card"}`);
            }
            if (o.zone !== "battlefield") continue;
            const stats = text(e.querySelector(".card-pt, .card-loyalty b"));
            const loyalty = o.counters.find((c) => c.kind === "loyalty");
            // A phone draws a face-up creature's scan alone: its printed P/T stands.
            const printed = phone && o.facing === "up";
            const want =
                o.pt && !printed
                    ? `${o.pt.power}/${Math.max(0, o.pt.toughness - o.damage)}`
                    : loyalty && String(loyalty.count);
            if (want !== undefined && stats !== want) {
                faults.push(`${what} shows ${stats} for ${want}`);
            }
            for (const c of o.counters) {
                if (c.kind === "loyalty") continue;
                // +1/+1 and -1/-1 counters show as one pill naming the kind, with a count
                // past one.
                const pill = e.querySelector(".card-pt-counter");
                const badge =
                    c.kind === "plusOne" || c.kind === "minusOne"
                        ? pill
                            ? (text(pill.querySelector("b"))?.slice(1) ?? "1")
                            : undefined
                        : text(e.querySelector(`.mark-${c.kind} b`));
                if (badge !== String(c.count)) {
                    faults.push(
                        `${what} shows ${badge} ${c.kind} counters for ${c.count}`,
                    );
                }
            }
            // A tapped card lies turned a quarter, wider than tall.
            const box = e.getBoundingClientRect();
            const tapped = o.tapped;
            if (box.width > box.height !== tapped) {
                faults.push(`${what} drawn ${tapped ? "untapped" : "tapped"}`);
            }
        }
        // Each emblem is drawn as a card, under a slot no object has (layout's emblemSlot).
        for (const side of ["you", "opponent"]) {
            for (const name of view.players[side].emblems) {
                const card = [...drawn].some(
                    ([slot, els]) =>
                        slot < 0 && els.some((e) => e.dataset.card === name),
                );
                if (!card) faults.push(`${side}'s emblem ${name} not drawn`);
            }
        }
        for (const slot of drawn.keys()) {
            if (slot >= 0 && !objects.has(slot))
                faults.push(`slot ${slot} drawn but not in the view`);
        }
        for (const side of ["you", "opponent"]) {
            const total = document.querySelector(
                `[data-life="${side}"] .life-orb-total`,
            );
            const life = String(view.players[side].life);
            if (total && text(total) !== life) {
                faults.push(`${side} at ${text(total)} life for ${life}`);
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
 * crowding a life orb's stops on a narrow stage would. A menu or a screen opened over the
 * table, and a pile's own cards under its button, are meant to overlap them. Cards may overlap
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
            e.matches(FIXTURES)
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
                "button, [role=button], a[href], input, select, .life-target",
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
            const hit = document.elementFromPoint(
                box.left + box.width / 2,
                box.top + box.height / 2,
            );
            return hit !== null && (e.contains(hit) || hit.contains(e));
        });
        const slack = 1;
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

/** What a phone screen (`?phone`) must not show, as DESIGN.md of the phone prototype sets it:
 * UI text under 12px (the P/T slash may be smaller, and a card's own printing, drawn where its
 * scan has not loaded, is the card's: a scan's is texture); a control smaller than about 40×40,
 * too small for a finger; the page scrolling sideways; and a card or control reaching outside
 * the window, but for the hand's, which the screen's edges cut off. Each is described for a
 * failure message. */
export async function phoneFaults(page) {
    return page.evaluate(() => {
        const faults = [];
        const shown = (e) => {
            if (!e.getClientRects().length) return false;
            if (getComputedStyle(e).visibility !== "visible") return false;
            for (let a = e; a; a = a.parentElement) {
                if (parseFloat(getComputedStyle(a).opacity) <= 0) return false;
            }
            return true;
        };
        const name = (e) =>
            e.getAttribute("aria-label") ||
            e.textContent?.trim().slice(0, 30) ||
            e.className ||
            e.tagName.toLowerCase();
        for (const el of document.querySelectorAll("#root *")) {
            const text = [...el.childNodes]
                .filter((n) => n.nodeType === Node.TEXT_NODE)
                .map((n) => n.textContent.trim())
                .join(" ")
                .trim();
            if (!text || el.closest(".card-rim, .card-pt i") || !shown(el))
                continue;
            const size = parseFloat(getComputedStyle(el).fontSize);
            if (size < 11.95) {
                faults.push(`"${text.slice(0, 40)}" at ${size.toFixed(1)}px`);
            }
        }
        const controls = document.querySelectorAll(
            "button, [role=button], a[href], input, select",
        );
        for (const e of controls) {
            if (!shown(e)) continue;
            const b = e.getBoundingClientRect();
            if (b.width < 39.5 || b.height < 39.5) {
                faults.push(
                    `${name(e)} is ${Math.round(b.width)}×${Math.round(b.height)}`,
                );
            }
        }
        const page = document.documentElement;
        if (page.scrollWidth > innerWidth + 1) {
            faults.push(`the page scrolls sideways to ${page.scrollWidth}px`);
        }
        for (const e of document.querySelectorAll(".card-el, button")) {
            if (!shown(e) || e.closest("[data-zone=hand]")) continue;
            const b = e.getBoundingClientRect();
            if (
                b.left < -1 ||
                b.top < -1 ||
                b.right > innerWidth + 1 ||
                b.bottom > innerHeight + 1
            ) {
                faults.push(
                    `${e.dataset.card ?? name(e)} reaches outside the window`,
                );
            }
        }
        return faults;
    });
}
