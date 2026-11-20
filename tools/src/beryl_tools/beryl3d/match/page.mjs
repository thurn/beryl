// What the web side of `just b3d match` shares between its scripts, dump.mjs (one moment) and
// motion.mjs (every frame of a span): the page of a fixture opened on Playwright's paused
// clock, and the function that reads the page's elements as the contract's records.
//
// Node resolves a module from the file that imports it, and this file is outside ui/, so the
// capture helpers (and through them Playwright) are imported from ui/scripts by path. The
// scripts run with ui/ as the working directory.
import { readFile } from "node:fs/promises";
import { join } from "node:path";
import { pathToFileURL } from "node:url";

const scripts = (name) =>
    import(pathToFileURL(join(process.cwd(), "scripts", name)).href);
const { launch } = await scripts("capture.mjs");
const { hangContext } = await scripts("hang.mjs");
const { takeSlot } = await scripts("slots.mjs");

/** One frame of the stage's clock, in milliseconds, as `ui/scripts/counts.mjs` steps it. */
export const FRAME = 1000 / 60;
/** The most frames to run before giving up on the page loading or coming to rest. */
export const MOST = 3000;

/** Opens the job's fixture page and runs `use(page, errors)` on it, then closes it; whether it
 * failed. The job is {base, route, click, fixture, width, height, scale, art_dir}: `click`, if
 * set, is the selector of a button to press once the page has loaded, for a screen the app
 * opens over another one (snapshot.py's `web_page`); `fixture` is {name, file}, the file that
 * answers the page's request for /fixtures/<name>.json, and `art_dir`, if set, the scan cache
 * card images load from. The page's clock is paused once the
 * stage and the fixture transport exist and every font has loaded, so it moves only in
 * `page.clock.runFor`. `errors` collects the page's uncaught errors. */
export async function withPage(job, use) {
    // The page takes one of the machine's browser page slots, which every session's pages
    // share.
    const release = await takeSlot();
    const browser = await launch();
    let failed = false;
    try {
        const context = await hangContext(browser, {
            viewport: { width: job.width, height: job.height },
            deviceScaleFactor: job.scale,
        });
        // The page fetches its fixture from the server (`fixtureTransport` in ui/src/ipc.ts); it
        // comes from the job's file instead, so the server needs no file of its own.
        const fixturePath = `/fixtures/${encodeURIComponent(job.fixture.name)}.json`;
        await context.route(
            (url) => url.pathname === fixturePath,
            async (route) => {
                await route.fulfill({
                    body: await readFile(job.fixture.file),
                    contentType: "application/json",
                });
            },
        );
        // With `art_dir`, card images load from the app's scan cache, which Beryl 3D reads too: it
        // holds each image under its Scryfall path (rust/crates/beryl_art), and the card backs
        // under `backs/`. An image it lacks is not fetched, and is listed.
        if (job.art_dir) {
            const hosts = {
                "cards.scryfall.io": job.art_dir,
                "backs.scryfall.io": join(job.art_dir, "backs"),
            };
            await context.route(/^https:\/\/(cards|backs)\.scryfall\.io\//, async (route) => {
                const url = new URL(route.request().url());
                const file = join(hosts[url.host], decodeURIComponent(url.pathname));
                try {
                    await route.fulfill({ body: await readFile(file) });
                } catch {
                    console.log(`scan missing: ${url.href}`);
                    await route.fulfill({ status: 404 });
                }
            });
        }
        const page = await context.newPage();
        const errors = [];
        page.on("pageerror", (e) => errors.push(e.message));
        // An installed clock follows real time until it is paused; paused, it moves only in
        // `runFor`, so the stage's animations stand where the offset puts them.
        const start = new Date("2026-01-01T00:00:00Z");
        await page.clock.install({ time: start });
        await page.clock.pauseAt(new Date(start.getTime() + 1000));
        await page.goto(new URL(job.route, job.base).href);
        // Loading runs on real tasks (module fetches, fonts); the fake clock steps once the stage
        // and the fixture transport exist.
        for (let i = 0; i < MOST; i++) {
            if (await page.evaluate(() => window.berylStage && window.berylFixture)) break;
            await page.clock.runFor(FRAME);
        }
        // The button is pressed from inside the page: Playwright's own click waits for animation
        // frames, which the paused clock never gives.
        if (job.click) {
            let pressed = false;
            for (let i = 0; i < MOST && !pressed; i++) {
                pressed = await page.evaluate((selector) => {
                    const button = document.querySelector(selector);
                    button?.click();
                    return button !== null;
                }, job.click);
                if (!pressed) await page.clock.runFor(FRAME);
            }
            if (!pressed) throw new Error(`no element matches ${job.click}`);
        }
        await page.evaluate(() =>
            Promise.all([...document.fonts].map((f) => f.load().catch(() => undefined))),
        );
        await use(page, errors);
        await context.close();
    } catch (e) {
        console.error(e.stack ?? String(e));
        failed = true;
    } finally {
        await browser.close();
        release();
    }
    return failed;
}

/** The text the page draws in a face that is not the first of its `font-family` list, one line
 * per family, as lines for report.txt; empty when every line of text is drawn in the
 * face its CSS asks for first. It runs in the page. A face counts as present when it is a
 * loaded `@font-face`, or, for an installed font, when text set in it measures differently from
 * text in at least one of the three generic families alone (a missing face falls back to the
 * generic one and measures the same as it). */
export function fontFallbacks() {
    const GENERIC = new Set(["serif", "sans-serif", "monospace", "cursive", "fantasy", "system-ui"]);
    const unquote = (name) => name.trim().replace(/^["']|["']$/g, "");
    const canvas = document.createElement("canvas").getContext("2d");
    const width = (font) => {
        canvas.font = font;
        return canvas.measureText("Hamburgefonstiv 0123 mmmmmm").width;
    };
    const present = new Map();
    const isPresent = (name) => {
        if (!present.has(name)) {
            const declared = [...document.fonts].filter((f) => unquote(f.family) === name);
            present.set(
                name,
                declared.length > 0
                    ? declared.some((f) => f.status === "loaded")
                    : ["serif", "sans-serif", "monospace"].some(
                          (base) => width(`16px "${name}", ${base}`) !== width(`16px ${base}`),
                      ),
            );
        }
        return present.get(name);
    };
    const found = new Map();
    const walk = (el) => {
        const s = getComputedStyle(el);
        if (s.display === "none") return;
        const hasText = [...el.childNodes].some(
            (n) => n.nodeType === Node.TEXT_NODE && n.data.trim(),
        );
        if (hasText && s.visibility !== "hidden") {
            const list = s.fontFamily.split(",").map(unquote);
            const first = list[0];
            if (first && !GENERIC.has(first) && !isPresent(first)) {
                const text = el.textContent.replace(/\s+/g, " ").trim().slice(0, 30);
                const seen = found.get(first) ?? { list, count: 0, text };
                seen.count++;
                found.set(first, seen);
            }
        }
        for (const child of el.children) walk(child);
    };
    walk(document.body);
    return [...found].map(
        ([first, { list, count, text }]) =>
            `fonts: ${count} element(s) of text such as ${JSON.stringify(text)} ` +
            `fall back from ${first} to the next face of ${list.join(", ")}`,
    );
}

/** The page's elements as the contract's records. It runs in the page, so it defines what it
 * calls. With `only`, an index into the records, it marks that element's node with
 * `data-b3d-only` (dump.mjs's `ONLY`). With `light`, for one frame of many (motion.mjs), it
 * reads no lines of text, which is most of its work, and gives each record a `track`: a number
 * that names the same DOM node in every frame of the page, kept in a map on `window` so that
 * no node is changed. */
export function records({ roles, only, light = false }) {
    const ARIA = {
        button: "button",
        link: "button",
        menuitem: "menuitem",
        checkbox: "toggle",
        switch: "toggle",
        slider: "slider",
        textbox: "field",
        list: "list",
        listbox: "list",
        img: "icon",
        dialog: "region",
        region: "region",
        group: "region",
    };
    const TAGS = {
        BUTTON: "button",
        INPUT: "field",
        TEXTAREA: "field",
        IMG: "icon",
        svg: "icon",
        UL: "list",
        OL: "list",
    };
    const round = (n, places = 2) => Math.round(n * 10 ** places) / 10 ** places;

    /** A CSS color as [r, g, b, a], channels 0 to 255 and alpha 0 to 1; null if unreadable. */
    const color = (text) => {
        const m = text.match(/rgba?\(([^)]*)\)/);
        if (m) {
            const parts = m[1].split(/[\s,/]+/).filter(Boolean).map(Number);
            return [parts[0], parts[1], parts[2], round(parts[3] ?? 1, 3)];
        }
        const srgb = text.match(/color\(srgb ([^)]*)\)/);
        if (srgb) {
            const parts = srgb[1].split(/[\s/]+/).filter(Boolean).map(Number);
            return [
                Math.round(parts[0] * 255),
                Math.round(parts[1] * 255),
                Math.round(parts[2] * 255),
                round(parts[3] ?? 1, 3),
            ];
        }
        if (text === "transparent") return [0, 0, 0, 0];
        return null;
    };
    /** `text` split at the commas that are not inside parentheses. */
    const commas = (text) => {
        const out = [];
        let depth = 0;
        let start = 0;
        for (let i = 0; i < text.length; i++) {
            if (text[i] === "(") depth++;
            else if (text[i] === ")") depth--;
            else if (text[i] === "," && depth === 0) {
                out.push(text.slice(start, i).trim());
                start = i + 1;
            }
        }
        out.push(text.slice(start).trim());
        return out.filter(Boolean);
    };
    const px = (text) => round(parseFloat(text));
    /** Four sides' values as one when they are equal. */
    const sides = (values) =>
        values.every((v) => JSON.stringify(v) === JSON.stringify(values[0]))
            ? values[0]
            : values;
    const family = (text) =>
        commas(text)[0]
            ?.replace(/^["']|["']$/g, "")
            .replace(/ Variable$/, "") ?? text;

    /**
     * A corner's radius as drawn, in points: a percentage of the element's size, and no more
     * than half its shorter side, as CSS scales radii that do not fit down (so the usual
     * `999px` of a pill is half its height). Of an elliptical corner, the horizontal radius.
     */
    const radius = (text, [width, height]) => {
        const first = text.trim().split(/\s+/)[0] ?? "0";
        const shorter = Math.min(width, height);
        const value = first.endsWith("%")
            ? (parseFloat(first) / 100) * shorter
            : parseFloat(first) || 0;
        return round(Math.min(value, shorter / 2));
    };
    /**
     * The element's whole transform, as the numbers of a `matrix(...)` (6) or `matrix3d(...)`
     * (16), or "none". CSS draws an element through its `translate`, `rotate` and `scale`
     * properties, in that order, and then its `transform`, so all four are composed here. The
     * stage turns a tapping card through `rotate` (stage.css `--turn`) on top of the face's own
     * `transform: rotate(90deg)` of a tapped card, so `transform` alone misses how far it is.
     */
    const transformOf = (s) => {
        const none = (v) => !v || v === "none";
        if ([s.translate, s.rotate, s.scale, s.transform].every(none)) return "none";
        let m = new DOMMatrix();
        if (!none(s.translate)) {
            const [x = 0, y = 0, z = 0] = s.translate.split(/\s+/).map(parseFloat);
            m = m.translate(x, y, z);
        }
        if (!none(s.rotate)) {
            // "45deg", "x 45deg" or "1 0 0 45deg": an angle about z, a named axis or a vector.
            const parts = s.rotate.trim().split(/\s+/);
            const angle = parseFloat(parts.pop() ?? "0");
            const named = { x: [1, 0, 0], y: [0, 1, 0], z: [0, 0, 1] };
            const axis =
                parts.length === 0
                    ? [0, 0, 1]
                    : parts.length === 1
                      ? (named[parts[0]] ?? [0, 0, 1])
                      : parts.map(parseFloat);
            m = m.rotateAxisAngle(axis[0], axis[1], axis[2], angle);
        }
        if (!none(s.scale)) {
            const [x = 1, y = x, z = 1] = s.scale.split(/\s+/).map(parseFloat);
            m = m.scale(x, y, z);
        }
        if (!none(s.transform)) m = m.multiply(new DOMMatrix(s.transform));
        const numbers = m.is2D ? [m.a, m.b, m.c, m.d, m.e, m.f] : [...m.toFloat64Array()];
        // `+ 0` turns a -0 into 0.
        return numbers.map((n) => round(n, 4) + 0);
    };
    // `opacity` is the element's with its ancestors', the opacity it is seen at, as Beryl 3D
    // resolves it.
    // `size` is the element's laid-out width and height, before any transform.
    const styleOf = (s, opacity, size) => ({
        "background-color": color(s.backgroundColor),
        // WebKit's text of the four layered properties, which css.py parses into structures.
        "background-image": s.backgroundImage,
        "border-width": sides(
            [s.borderTopWidth, s.borderRightWidth, s.borderBottomWidth, s.borderLeftWidth].map(
                px,
            ),
        ),
        "border-color": sides(
            [s.borderTopColor, s.borderRightColor, s.borderBottomColor, s.borderLeftColor].map(
                color,
            ),
        ),
        "border-radius": sides(
            [
                s.borderTopLeftRadius,
                s.borderTopRightRadius,
                s.borderBottomRightRadius,
                s.borderBottomLeftRadius,
            ].map((r) => radius(r, size)),
        ),
        "box-shadow": s.boxShadow,
        filter: s.filter,
        opacity: round(opacity, 3),
        color: color(s.color),
        "font-family": family(s.fontFamily),
        "font-size": px(s.fontSize),
        "font-weight": Number(s.fontWeight),
        "mask-image": s.maskImage || s.webkitMaskImage || "none",
        transform: transformOf(s),
    });

    const laidOut = (el, rect) =>
        el instanceof HTMLElement ? [el.offsetWidth, el.offsetHeight] : [rect.width, rect.height];
    const visible = (el, s, rect) =>
        s.display !== "none" &&
        s.visibility !== "hidden" &&
        rect.width > 0 &&
        rect.height > 0;
    const paints = (s) =>
        (color(s.backgroundColor)?.[3] ?? 0) > 0 ||
        s.backgroundImage !== "none" ||
        s.boxShadow !== "none" ||
        (parseFloat(s.borderTopWidth) > 0 && (color(s.borderTopColor)?.[3] ?? 0) > 0);
    const ownText = (el) =>
        [...el.childNodes].some((n) => n.nodeType === Node.TEXT_NODE && n.data.trim());
    /** The first entry of roles.json that `el` matches, or undefined. */
    const ruleOf = (el) =>
        roles.find(({ selector }) => {
            try {
                return el.matches(selector);
            } catch {
                // A selector WebKit cannot read matches nothing.
                return false;
            }
        });
    const roleOf = (el) => {
        const rule = ruleOf(el);
        if (rule) return rule.role;
        const aria = el.getAttribute("role");
        if (aria && ARIA[aria]) return ARIA[aria];
        return TAGS[el.tagName] ?? (ownText(el) ? "text" : "region");
    };
    const words = (text) => text.replace(/\s+/g, " ").trim();
    /** The words inside `el`, as an accessible name is computed from content: a child with a
     * label of its own (such as a mana symbol labelled "{U}") gives its label in place of its
     * text, and a hidden child gives nothing. */
    const content = (el) =>
        [...el.childNodes]
            .map((node) => {
                if (node.nodeType === Node.TEXT_NODE) return node.data;
                if (node.nodeType !== Node.ELEMENT_NODE) return "";
                if (node.getAttribute("aria-hidden") === "true") return "";
                return node.getAttribute("aria-label") ?? content(node);
            })
            .join("");
    /** `el`'s name: its roles.json entry's `called` where it has one, for an element with no
     * accessible name that Beryl 3D names; otherwise its name cut to the first group of the
     * entry's `name` pattern where it has one, so that "Your library, 32 cards" can be named
     * "Your library" as Beryl 3D names it. */
    const nameOf = (el) => {
        const rule = ruleOf(el);
        if (rule?.called !== undefined) return rule.called;
        // A mark's outline is named by its class (`stage-mark-eligible`), as it has no name.
        const name = rule?.from === "class" ? el.className : fullName(el);
        const pattern = rule?.name;
        return (pattern && new RegExp(pattern).exec(name)?.[1]) ?? name;
    };
    const fullName = (el) => {
        const label = el.getAttribute("aria-label");
        if (label) return words(label);
        const by = el.getAttribute("aria-labelledby");
        if (by) {
            const text = by
                .split(/\s+/)
                .map((id) => document.getElementById(id)?.textContent ?? "")
                .join(" ");
            if (words(text)) return words(text);
        }
        const alt = el.getAttribute("alt") ?? el.getAttribute("title");
        if (alt) return words(alt);
        if (el.dataset.card) return el.dataset.card;
        return words(content(el)).slice(0, 200);
    };
    // An effect (ui/src/stage/effects.ts) is a `.stage-effect stage-effect-<name>
    // stage-effect-<layer>` node inside the node of what it is on, or inside a player's life
    // total (`data-life`, the player's side): its name is the effect's and its object what it is
    // on, as Beryl 3D's records name an effect.
    const LAYERS = new Set(["stage-effect", "stage-effect-under", "stage-effect-over"]);
    const effectName = (el) =>
        [...el.classList]
            .find((c) => c.startsWith("stage-effect-") && !LAYERS.has(c))
            ?.slice("stage-effect-".length) ?? "";
    const effectOn = (el) => {
        const on = el.parentElement;
        if (!on) return null;
        if (on.dataset.life !== undefined) return on.dataset.life;
        return objectOf(on);
    };
    const objectOf = (el) => {
        const stage = el.dataset.stage ?? openingSlot(el);
        if (stage !== undefined && /^-?\d+$/.test(stage)) return Number(stage);
        return stage ?? null;
    };
    // During the opening (ui/src/screens/Opening.tsx) the web draws each card of the hand
    // twice: the stage's card, which carries `data-stage` and which the desktop board moves
    // below the window, and the opening's own copy in its row (`.opening-card`, a `card` in
    // roles.json), whose `data-slot` is the card's object. Beryl 3D draws one card, in the
    // opening's row, so the copy is named by that object too, and the matcher pairs Beryl 3D's
    // card with whichever of the two is on the screen.
    const openingSlot = (el) =>
        roleOf(el) === "card" ? el.closest(".opening [data-slot]")?.dataset.slot : undefined;

    // Each line of a text node: its words, its box and its baseline. A character's box from a
    // Range is its font's content area, so the baseline lies the font's ascent below its top.
    const canvas = document.createElement("canvas").getContext("2d");
    const ascent = (s) => {
        canvas.font = `${s.fontStyle} ${s.fontWeight} ${s.fontSize} ${s.fontFamily}`;
        return canvas.measureText("Hg").fontBoundingBoxAscent;
    };
    const linesOf = (node, s) => {
        const lines = [];
        const range = document.createRange();
        const up = ascent(s);
        for (let i = 0; i < node.data.length; i++) {
            range.setStart(node, i);
            range.setEnd(node, i + 1);
            const box = range.getClientRects()[0];
            if (!box || box.width === 0) {
                if (lines.length) lines[lines.length - 1].string += node.data[i];
                continue;
            }
            const last = lines[lines.length - 1];
            if (last && Math.abs(last.top - box.top) < 1) {
                last.string += node.data[i];
                last.right = Math.max(last.right, box.right);
                last.bottom = Math.max(last.bottom, box.bottom);
            } else {
                lines.push({
                    string: node.data[i],
                    left: box.left,
                    top: box.top,
                    right: box.right,
                    bottom: box.bottom,
                });
            }
        }
        return lines
            .map((line) => ({
                string: line.string.replace(/\s+/g, " ").trim(),
                family: family(s.fontFamily),
                size: px(s.fontSize),
                weight: Number(s.fontWeight),
                color: color(s.color),
                baseline: round(line.top + up),
                box: [
                    round(line.left),
                    round(line.top),
                    round(line.right - line.left),
                    round(line.bottom - line.top),
                ],
            }))
            .filter((line) => line.string);
    };

    window.berylMatchTracks ??= { next: 0, of: new WeakMap() };
    const tracks = window.berylMatchTracks;
    const trackOf = (el) => {
        if (!tracks.of.has(el)) tracks.of.set(el, tracks.next++);
        return tracks.of.get(el);
    };

    const elements = [];
    const indexOf = new Map();
    const walk = (el, parent, seen) => {
        const s = getComputedStyle(el);
        if (s.display === "none") return;
        const opacity = seen * Number(s.opacity);
        const rect = el.getBoundingClientRect();
        const shown = visible(el, s, rect);
        const role = roleOf(el);
        // A mark that has not come up, or that the prompt hides (`[data-pay]`), draws nothing.
        const kept =
            shown &&
            !(role === "mark" && opacity === 0) &&
            (el.hasAttribute("role") ||
                el.hasAttribute("aria-label") ||
                el.dataset.stage !== undefined ||
                ownText(el) ||
                paints(s) ||
                role !== "region");
        let here = parent;
        if (kept) {
            here = elements.length;
            indexOf.set(el, here);
            const text = [];
            for (const node of light ? [] : el.childNodes) {
                if (node.nodeType === Node.TEXT_NODE && node.data.trim()) {
                    text.push(...linesOf(node, s));
                }
            }
            elements.push({
                key: null,
                role,
                name: role === "effect" ? effectName(el) : nameOf(el),
                // A mark's object is what it rings, named as an effect's is.
                object: role === "effect" || role === "mark" ? effectOn(el) : objectOf(el),
                box: [round(rect.x), round(rect.y), round(rect.width), round(rect.height)],
                z: here,
                parent,
                style: styleOf(s, opacity, laidOut(el, rect)),
                text,
                ...(light ? { track: trackOf(el) } : {}),
            });
        }
        for (const child of el.children) walk(child, here, opacity);
    };
    for (const child of document.body.children) walk(child, null, 1);
    // `shake` moves the whole table (effects.css `[data-shaking]`), not a node of its own: it is
    // recorded as an effect on `table` whose box is how far the table is moved from where it
    // rests, of no size, as Beryl 3D records the offset of its picture. Where it rests is read
    // with the attribute taken off for a moment, which no tick of the stage sees.
    const shaking = document.querySelector(".stage[data-shaking], .phone-board[data-shaking]");
    if (shaking) {
        const centre = () => {
            const r = shaking.getBoundingClientRect();
            return [r.x + r.width / 2, r.y + r.height / 2];
        };
        const [x, y] = centre();
        shaking.removeAttribute("data-shaking");
        const [restX, restY] = centre();
        shaking.setAttribute("data-shaking", "");
        elements.push({
            key: null,
            role: "effect",
            name: "shake",
            object: "table",
            box: [round(x - restX), round(y - restY), 0, 0],
            z: elements.length,
            parent: null,
            style: styleOf(getComputedStyle(shaking), 1, [0, 0]),
            text: [],
            ...(light ? { track: "shake" } : {}),
        });
    }
    for (const [el, index] of indexOf) {
        if (index === only) el.setAttribute("data-b3d-only", "");
    }
    return elements;
}
