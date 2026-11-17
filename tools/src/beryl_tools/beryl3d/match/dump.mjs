// The web side of `just b3d match snapshot` (snapshot.py): opens a fixture page in the app's
// browser engine on Playwright's paused clock, runs the clock to the moment Beryl 3D shows, and
// writes the page's screenshot and its elements as the records `web.json` holds.
//
// Usage, from ui/ (where Playwright is installed): node <this file> <job.json>
// The job is {base, route, fixture, width, height, scale, offset_ms, png, json, roles, target,
// art_dir}: `fixture` is {name, file}, the file that answers the page's request for
// /fixtures/<name>.json; `offset_ms` is how long after the fixture's last view arrived to stop
// the clock, or null for the view at rest; `roles` is data/beryl3d/match/roles.json's `roles`.
//
// Node resolves a module from the file that imports it, and this file is outside ui/, so the
// capture helpers (and through them Playwright) are imported from ui/scripts by path.
import { readFile, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { pathToFileURL } from "node:url";

const scripts = (name) =>
    import(pathToFileURL(join(process.cwd(), "scripts", name)).href);
const { launch } = await scripts("capture.mjs");
const { hangContext } = await scripts("hang.mjs");
const { takeSlot } = await scripts("slots.mjs");

/** One frame of the stage's clock, in milliseconds, as `ui/scripts/counts.mjs` steps it. */
const FRAME = 1000 / 60;
/** Frames in a row the stage must be idle for the view to count as at rest. */
const TAIL = 30;
/** The most frames to run before giving up on the page loading or coming to rest. */
const MOST = 3000;

const job = JSON.parse(await readFile(process.argv[2], "utf8"));

/** Finishes the page's CSS animations and transitions and drains React's posted tasks, as
 * counts.mjs's `settle` does; whether the stage is still playing. It runs in the page. */
async function settle() {
    for (let round = 0; round < 8; round++) {
        void document.documentElement.getBoundingClientRect();
        const running = document
            .getAnimations()
            .filter((animation) => animation.playState === "running");
        if (running.length === 0) break;
        for (const animation of running) {
            try {
                animation.finish();
            } catch {
                animation.pause();
            }
        }
    }
    await new Promise((resolve) => {
        const channel = new MessageChannel();
        channel.port1.onmessage = () => resolve();
        channel.port2.postMessage(undefined);
    });
    return Boolean(window.berylStage?.shown.busy);
}

/** The page's elements as the contract's records. It runs in the page, so it defines what it
 * calls. */
function records(roles) {
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
    /** The color at the start of `text` and the rest of it. */
    const leadingColor = (text) => {
        const m = text.match(/^(rgba?\([^)]*\)|color\([^)]*\)|transparent)\s*/);
        if (m) return [color(m[1]), text.slice(m[0].length)];
        const tail = text.match(/\s*(rgba?\([^)]*\)|color\([^)]*\))$/);
        if (tail) return [color(tail[1]), text.slice(0, -tail[0].length)];
        return [null, text];
    };
    const px = (text) => round(parseFloat(text));
    /** One `box-shadow` or `drop-shadow` as {inset, x, y, blur, spread, color}. */
    const shadow = (text) => {
        const inset = /\binset\b/.test(text);
        const [c, rest] = leadingColor(text.replace(/\binset\b/, "").trim());
        const lengths = rest.trim().split(/\s+/).filter(Boolean).map(px);
        const [x = 0, y = 0, blur = 0, spread = 0] = lengths;
        return { inset, x, y, blur, spread, color: c };
    };
    /** A gradient or image of `background-image` or `mask-image`. */
    const image = (text) => {
        const m = text.match(/^([a-z-]+)\((.*)\)$/s);
        if (!m) return { kind: "unknown", raw: text };
        const [, fn, inner] = m;
        if (fn === "url") return { kind: "url", url: inner.replace(/^["']|["']$/g, "") };
        const kind = fn.replace(/^-webkit-/, "").replace(/-gradient$/, "");
        const parts = commas(inner);
        const out = { kind, repeating: kind.startsWith("repeating-") };
        if (parts.length && !leadingColor(parts[0])[0]) out.shape = parts.shift();
        out.stops = parts.map((part) => {
            const [c, rest] = leadingColor(part);
            const at = rest.trim().split(/\s+/).filter(Boolean);
            return { color: c, at: at.length ? at : null };
        });
        return out;
    };
    const filter = (text) => {
        const out = [];
        for (const m of text.matchAll(/([a-z-]+)\(((?:[^()]|\([^()]*\))*)\)/g)) {
            const [, fn, args] = m;
            out.push(
                fn === "drop-shadow"
                    ? { fn, shadow: shadow(args) }
                    : { fn, value: args.trim() },
            );
        }
        return out;
    };
    /** Four sides' values as one when they are equal. */
    const sides = (values) =>
        values.every((v) => JSON.stringify(v) === JSON.stringify(values[0]))
            ? values[0]
            : values;
    const family = (text) =>
        commas(text)[0]
            ?.replace(/^["']|["']$/g, "")
            .replace(/ Variable$/, "") ?? text;

    // `opacity` is the element's with its ancestors', the opacity it is seen at, as Beryl 3D
    // resolves it.
    const styleOf = (s, opacity) => ({
        "background-color": color(s.backgroundColor),
        "background-image":
            s.backgroundImage === "none" ? "none" : commas(s.backgroundImage).map(image),
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
            ].map(px),
        ),
        "box-shadow": s.boxShadow === "none" ? "none" : commas(s.boxShadow).map(shadow),
        filter: s.filter === "none" ? "none" : filter(s.filter),
        opacity: round(opacity, 3),
        color: color(s.color),
        "font-family": family(s.fontFamily),
        "font-size": px(s.fontSize),
        "font-weight": Number(s.fontWeight),
        "mask-image": (() => {
            const mask = s.maskImage ?? s.webkitMaskImage ?? "none";
            return !mask || mask === "none" ? "none" : commas(mask).map(image);
        })(),
        transform:
            s.transform === "none"
                ? "none"
                : (s.transform.match(/-?[\d.e+-]+/g) ?? []).map((n) => round(Number(n), 4)),
    });

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
    const roleOf = (el) => {
        for (const { selector, role } of roles) {
            try {
                if (el.matches(selector)) return role;
            } catch {
                // A selector WebKit cannot read gives no role.
            }
        }
        const aria = el.getAttribute("role");
        if (aria && ARIA[aria]) return ARIA[aria];
        return TAGS[el.tagName] ?? (ownText(el) ? "text" : "region");
    };
    const words = (text) => text.replace(/\s+/g, " ").trim();
    const nameOf = (el) => {
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
        return words(el.textContent ?? "").slice(0, 200);
    };
    const objectOf = (el) => {
        const stage = el.dataset.stage;
        if (stage !== undefined && /^-?\d+$/.test(stage)) return Number(stage);
        return stage ?? null;
    };

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

    const elements = [];
    const indexOf = new Map();
    const walk = (el, parent, seen) => {
        const s = getComputedStyle(el);
        if (s.display === "none") return;
        const opacity = seen * Number(s.opacity);
        const rect = el.getBoundingClientRect();
        const shown = visible(el, s, rect);
        const role = roleOf(el);
        const kept =
            shown &&
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
            for (const node of el.childNodes) {
                if (node.nodeType === Node.TEXT_NODE && node.data.trim()) {
                    text.push(...linesOf(node, s));
                }
            }
            elements.push({
                key: null,
                role,
                name: nameOf(el),
                object: objectOf(el),
                box: [round(rect.x), round(rect.y), round(rect.width), round(rect.height)],
                z: here,
                parent,
                style: styleOf(s, opacity),
                text,
            });
        }
        for (const child of el.children) walk(child, here, opacity);
    };
    for (const child of document.body.children) walk(child, null, 1);
    return elements;
}

// The page takes one of the machine's browser page slots, which every session's pages share.
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
    await page.evaluate(() =>
        Promise.all([...document.fonts].map((f) => f.load().catch(() => undefined))),
    );
    if (job.offset_ms === null) {
        let quiet = 0;
        for (let played = 0; quiet < TAIL && played < MOST; played++) {
            await page.clock.runFor(FRAME);
            quiet = (await page.evaluate(settle)) ? 0 : quiet + 1;
        }
    } else {
        await page.clock.runFor(Math.max(0, job.offset_ms));
    }
    await page.screenshot({ path: job.png });
    const elements = await page.evaluate(records, job.roles);
    await writeFile(
        job.json,
        `${JSON.stringify({ target: job.target, scale: job.scale, elements }, null, 1)}\n`,
    );
    for (const error of errors) console.log(`page error: ${error}`);
    await context.close();
} catch (e) {
    console.error(e.stack ?? String(e));
    failed = true;
} finally {
    await browser.close();
    release();
}
process.exit(failed ? 1 : 0);
