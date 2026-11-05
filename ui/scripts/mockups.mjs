// The mockup harness behind `by ui-compare`, `by ui-probe` and `by ui-fonts` (tools/): headless
// Playwright WebKit renders the replicas (`?replica=<mockup>`, src/replica/) and a blank page's
// canvas crops and composites, so no image library is needed. docs/ui_mockups/regions.json names
// each mockup's parts and art windows.
//
//   node scripts/mockups.mjs art <regions.json> <out-dir>
//   node scripts/mockups.mjs compare <base-url> <regions.json> <out-dir> <zoom> <part>...
//   node scripts/mockups.mjs probe <png> <x> <y> [<w> <h>] [--threshold N] [--grid <out.png> --zoom N]
//   node scripts/mockups.mjs fonts <base-url> <out.png> <spec.json>
//
// Every output path is printed; a page error fails the command.
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import {
    imagePage,
    launch,
    loadImage,
    openPage,
    pngBytes,
    settle,
} from "./capture.mjs";

const fail = (message) => {
    console.error(message);
    process.exit(2);
};

/** The region file with shorthands expanded: every part `{ rect, masks }` (its own masks plus the
 * mockup-wide ones it overlaps), every art window `{ rect, rotate }`. */
async function readRegions(path) {
    const raw = JSON.parse(await readFile(path, "utf8"));
    const mockups = {};
    for (const [name, m] of Object.entries(raw.mockups)) {
        const shared = m.masks ?? [];
        const parts = {};
        for (const [part, spec] of Object.entries(m.parts)) {
            const rect = Array.isArray(spec) ? spec : spec.rect;
            const own = Array.isArray(spec) ? [] : (spec.masks ?? []);
            parts[part] = {
                rect,
                masks: [...own, ...shared.filter((s) => overlaps(s, rect))],
            };
        }
        const art = {};
        for (const [card, spec] of Object.entries(m.art ?? {})) {
            art[card] = Array.isArray(spec)
                ? { rect: spec, rotate: 0 }
                : { rect: spec.rect, rotate: spec.rotate ?? 0 };
        }
        mockups[name] = {
            ...m,
            path: join(dirname(path), m.file),
            parts,
            art,
        };
    }
    return mockups;
}

const overlaps = ([ax, ay, aw, ah], [bx, by, bw, bh]) =>
    ax < bx + bw && bx < ax + aw && ay < by + bh && by < ay + ah;

/** Cuts every art window out of its mockup into <out>/<mockup>/<card>.png. */
async function art(regionsPath, out) {
    const mockups = await readRegions(regionsPath);
    const browser = await launch();
    try {
        const page = await imagePage(browser);
        for (const [name, m] of Object.entries(mockups)) {
            await loadImage(page, name, await readFile(m.path));
            await mkdir(join(out, name), { recursive: true });
            for (const [card, { rect, rotate }] of Object.entries(m.art)) {
                const url = await page.evaluate(
                    ([n, r, a]) => window.kit.cut(n, r, a),
                    [name, rect, rotate],
                );
                const path = join(out, name, `${card}.png`);
                await writeFile(path, pngBytes(url));
            }
        }
    } finally {
        await browser.close();
    }
}

/** Renders each named part's mockup's replica and writes the part's comparison images. */
async function compare(base, regionsPath, out, zoom, names) {
    const mockups = await readRegions(regionsPath);
    const byMockup = new Map();
    for (const full of names) {
        const [mockup, part] = full.split("/");
        if (!mockups[mockup]?.parts[part]) fail(`ui-compare: no part ${full}`);
        byMockup.set(mockup, [...(byMockup.get(mockup) ?? []), part]);
    }
    const browser = await launch();
    let failed = false;
    try {
        const kit = await imagePage(browser);
        for (const [mockup, parts] of byMockup) {
            const m = mockups[mockup];
            await loadImage(kit, "mockup", await readFile(m.path));
            const [width, height] = await kit.evaluate(() =>
                window.kit.size("mockup"),
            );
            const { context, page, errors } = await openPage(browser, {
                width,
                height,
            });
            await page.goto(
                new URL(`/?replica=${encodeURIComponent(mockup)}`, base).href,
                { waitUntil: "load" },
            );
            await settle(page);
            await loadImage(kit, "replica", await page.screenshot());
            await context.close();
            if (errors.length) {
                failed = true;
                console.error(`ui-compare: ${mockup}: ${errors.join("\n  ")}`);
            }
            // The region file drawn over the mockup (art windows in yellow) and over the replica.
            const regions = Object.entries(m.parts).map(([label, p]) => ({
                label,
                ...p,
            }));
            const windows = Object.entries(m.art).map(([slot, a]) => ({
                label: `art:${slot}`,
                rect: a.rect,
                color: "#ff0",
            }));
            for (const [image, file, list] of [
                ["mockup", "mockup-regions.png", [...regions, ...windows]],
                ["replica", "replica-regions.png", regions],
            ]) {
                const url = await kit.evaluate(
                    ([n, ps]) => window.kit.outline(n, ps),
                    [image, list],
                );
                const path = join(out, mockup, file);
                await mkdir(dirname(path), { recursive: true });
                await writeFile(path, pngBytes(url));
                console.log(path);
            }
            for (const part of parts) {
                const { rect, masks } = m.parts[part];
                const images = await kit.evaluate(
                    ([r, k, z]) =>
                        window.kit.compare("mockup", "replica", r, k, z),
                    [rect, masks, zoom],
                );
                const dir = join(out, mockup, part);
                await mkdir(dir, { recursive: true });
                const files = {
                    "mockup.png": images.mockup,
                    "replica.png": images.replica,
                    "side-by-side.png": images.side,
                    "onion.png": images.onion,
                    "diff.png": images.diff,
                };
                for (const [file, url] of Object.entries(files)) {
                    await writeFile(join(dir, file), pngBytes(url));
                    console.log(join(dir, file));
                }
                await writeFile(
                    join(dir, "flip.html"),
                    flipPage(`${mockup}/${part}`),
                );
                console.log(join(dir, "flip.html"));
            }
        }
    } finally {
        await browser.close();
    }
    if (failed) process.exit(1);
}

/** A page alternating the mockup crop and the replica crop in place; a click pauses it. */
const flipPage = (title) => `<!doctype html>
<meta charset="utf-8">
<title>${title}</title>
<style>
  body { margin: 0; padding: 16px; background: #111; color: #ddd; font: 13px Menlo, monospace; }
  .flip { position: relative; display: inline-block; outline: 1px solid #f0f; }
  .flip img { display: block; image-rendering: pixelated; }
  .flip img + img { position: absolute; inset: 0; animation: flip 1s steps(1) infinite; }
  body.paused .flip img + img { animation-play-state: paused; }
  @keyframes flip { 0% { opacity: 0; } 50% { opacity: 1; } }
  .which::after { content: "mockup"; animation: label 1s steps(1) infinite; }
  body.paused .which::after { animation-play-state: paused; }
  @keyframes label { 0% { content: "mockup"; } 50% { content: "replica"; } }
</style>
<p>${title}: <b class="which"></b> (click to pause)</p>
<div class="flip"><img src="mockup.png" alt="mockup"><img src="replica.png" alt="replica"></div>
<script>document.body.onclick = () => document.body.classList.toggle("paused");</script>
`;

const hex = ([r, g, b]) =>
    "#" + [r, g, b].map((v) => v.toString(16).padStart(2, "0")).join("");
const describe = (c) => `${hex(c)} rgb(${c.join(", ")})`;

/** Where the color changes along a line of pixels: runs of steps above `threshold` (the largest
 * channel difference between neighbors), each with the colors on either side. */
function edges(line, start, threshold) {
    const found = [];
    for (let i = 1; i < line.length; i++) {
        const step = Math.max(
            ...line[i].map((v, k) => Math.abs(v - line[i - 1][k])),
        );
        if (step <= threshold) continue;
        const last = found[found.length - 1];
        if (last && last.end === i - 1) {
            last.end = i;
            last.after = line[i];
        } else {
            found.push({
                begin: i,
                end: i,
                before: line[i - 1],
                after: line[i],
            });
        }
    }
    return found.map(
        (e) =>
            `  ${start + e.begin}${e.end > e.begin ? `-${start + e.end}` : ""}: ${hex(e.before)} -> ${hex(e.after)}`,
    );
}

async function probe(path, args) {
    const flags = {};
    const nums = [];
    for (let i = 0; i < args.length; i++) {
        if (args[i].startsWith("--")) flags[args[i].slice(2)] = args[++i];
        else nums.push(Number(args[i]));
    }
    if (
        ![2, 4].includes(nums.length) ||
        nums.some((n) => !Number.isInteger(n))
    ) {
        fail("ui-probe: give <x> <y> or <x> <y> <w> <h> in whole pixels");
    }
    const [x, y, w = 1, h = 1] = nums;
    const threshold = Number(flags.threshold ?? 24);
    const browser = await launch();
    try {
        const kit = await imagePage(browser);
        const [W, H] = await loadImage(kit, "img", await readFile(path)).then(
            () => kit.evaluate(() => window.kit.size("img")),
        );
        if (x < 0 || y < 0 || x + w > W || y + h > H) {
            fail(
                `ui-probe: ${x},${y} ${w}x${h} is outside the ${W}x${H} image`,
            );
        }
        const px = (r) =>
            kit.evaluate(([rr]) => window.kit.pixels("img", rr), [r]);
        const [point] = await px([x, y, 1, 1]);
        console.log(`point ${x},${y}: ${describe(point)}`);
        if (w > 1 || h > 1) {
            const all = await px([x, y, w, h]);
            const mean = [0, 1, 2].map((k) =>
                Math.round(all.reduce((s, c) => s + c[k], 0) / all.length),
            );
            console.log(`mean of ${x},${y} ${w}x${h}: ${describe(mean)}`);
            const row = y + Math.floor(h / 2);
            const col = x + Math.floor(w / 2);
            if (w > 1) {
                console.log(`edges along row ${row}, x ${x}..${x + w - 1}:`);
                const lines = edges(await px([x, row, w, 1]), x, threshold);
                console.log(lines.length ? lines.join("\n") : "  none");
            }
            if (h > 1) {
                console.log(`edges along column ${col}, y ${y}..${y + h - 1}:`);
                const lines = edges(await px([col, y, 1, h]), y, threshold);
                console.log(lines.length ? lines.join("\n") : "  none");
            }
        }
        if (flags.grid) {
            const zoom = Number(flags.zoom ?? 8);
            const step = Number(flags.step ?? (zoom >= 6 ? 2 : 10));
            const url = await kit.evaluate(
                ([r, z, s]) => window.kit.grid("img", r, z, s),
                [[x, y, w, h], zoom, step],
            );
            await mkdir(dirname(flags.grid), { recursive: true });
            await writeFile(flags.grid, pngBytes(url));
            console.log(flags.grid);
        }
    } finally {
        await browser.close();
    }
}

/** Renders the font sheet route with `spec` (see src/replica/FontSheet.tsx). */
async function fonts(base, out, specPath) {
    const spec = JSON.parse(await readFile(specPath, "utf8"));
    const browser = await launch();
    let failed = false;
    try {
        const kit = await imagePage(browser);
        await loadImage(kit, "mockup", await readFile(spec.mockup));
        spec.sample = await kit.evaluate(
            ([r]) => window.kit.cut("mockup", r, 0),
            [spec.rect],
        );
        // Wide enough for the three boxes of a row: the crop, the overlay and the text alone.
        const { context, page, errors } = await openPage(browser, {
            width: Math.max(1200, 3 * spec.rect[2] * spec.zoom + 48),
            height: 800,
        });
        await page.addInitScript((s) => {
            window.__berylFontSheet = s;
        }, spec);
        await page.goto(new URL("/?replica=fonts", base).href, {
            waitUntil: "load",
        });
        await page.waitForSelector("[data-ready]");
        await settle(page);
        await mkdir(dirname(out), { recursive: true });
        await page.screenshot({ path: out, fullPage: true });
        await context.close();
        if (errors.length) {
            failed = true;
            console.error(`ui-fonts: ${errors.join("\n  ")}`);
        }
        console.log(out);
    } finally {
        await browser.close();
    }
    if (failed) process.exit(1);
}

const [command, ...rest] = process.argv.slice(2);
switch (command) {
    case "art":
        await art(resolve(rest[0]), resolve(rest[1]));
        break;
    case "compare":
        await compare(
            rest[0],
            resolve(rest[1]),
            resolve(rest[2]),
            Number(rest[3]),
            rest.slice(4),
        );
        break;
    case "probe":
        await probe(resolve(rest[0]), rest.slice(1));
        break;
    case "fonts":
        await fonts(rest[0], resolve(rest[1]), resolve(rest[2]));
        break;
    default:
        fail("usage: node scripts/mockups.mjs art|compare|probe|fonts ...");
}
