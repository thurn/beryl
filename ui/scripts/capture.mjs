// What the headless capture scripts share: opening a page at a fixed size and waiting until it has
// settled.
import { webkit } from "playwright";

export const launch = () => webkit.launch();

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
 * Each is described by its text and how it is cut, for a failure message. */
export function cutText(page) {
    return page.evaluate(() => {
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
            const line = 0.3 * parseFloat(style.fontSize);
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
                            straddles(t, b, ["left", "right"], 1)) ||
                        (!scrolls(y) &&
                            straddles(t, b, ["top", "bottom"], line));
                    if (cut) {
                        const what = `${a.tagName.toLowerCase()}.${[...a.classList].join(".")}`;
                        found.push(`"${text}" is cut by ${what}`);
                        break;
                    }
                    framed ||= frames;
                }
                if (scrolls(x) || scrolls(y) || a.matches(CARD)) break;
            }
        }
        return found;
    });
}
