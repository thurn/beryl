// Renders one address of the efficiency page to a PNG for `just efficiency shot` (serve.py).
// Usage: node shot.mjs <beryl-cache> <url> <out.png> <width> <light|dark>
// A URL ending in #open opens the first level of detail before the capture.
import { existsSync, readdirSync } from "node:fs";
import { createRequire } from "node:module";
import { join, resolve } from "node:path";

const [cache, url, out, width, scheme] = process.argv.slice(2);
const require = createRequire(import.meta.url);
const timeout = 120_000;

function findPlaywright() {
    // Playwright installs playwright-core, which exposes the same browser API.
    // Beryl's shared cache stores both packages directly under a hashed folder.
    const packageName = "playwright-core";
    const installed = join(cache, "node_modules");
    const cached = existsSync(installed)
        ? readdirSync(installed).sort().map((name) => join(installed, name, packageName))
        : [];
    const candidates = [
        ...(process.env.PLAYWRIGHT_NODE_MODULES
            ? [resolve(process.env.PLAYWRIGHT_NODE_MODULES, packageName)]
            : []),
        "playwright",
        packageName,
        join(cache, "efficiency", "node_modules", packageName),
        join(installed, packageName),
        ...cached,
    ];
    for (const candidate of candidates) {
        try {
            return require.resolve(candidate);
        } catch (error) {
            if (error.code !== "MODULE_NOT_FOUND") throw error;
        }
    }
    throw new Error(
        "Install Playwright under the efficiency cache or set PLAYWRIGHT_NODE_MODULES to its node_modules folder.",
    );
}

const playwright = findPlaywright();
const browserCache = join(cache, "playwright");
if (!process.env.PLAYWRIGHT_BROWSERS_PATH && existsSync(browserCache)) {
    process.env.PLAYWRIGHT_BROWSERS_PATH = browserCache;
}
const { chromium, webkit } = require(playwright);
const engines = process.platform === "win32" ? [chromium, webkit] : [webkit, chromium];
const engine = engines.find((candidate) => existsSync(candidate.executablePath()));
if (!engine) {
    throw new Error(
        "Install a Playwright browser with `playwright install chromium` or `playwright install webkit` for this package.",
    );
}
const browser = await engine.launch(engine === chromium ? { channel: "chromium" } : {});
try {
    const page = await browser.newPage({
        viewport: { width: Number(width), height: 800 },
        colorScheme: scheme,
    });
    const errors = [];
    page.on("pageerror", (e) => errors.push(String(e)));
    const response = await page.goto(url, { waitUntil: "load", timeout });
    if (!response?.ok()) {
        throw new Error(`The efficiency page returned HTTP ${response?.status()} for ${url}`);
    }
    if (new URL(url).hash === "#open") {
        // Only the lines the page had at first: a section that opens brings lines of its
        // own, which stay closed.
        await page.$$eval("details[data-src]", (lines) => {
            for (const d of lines) {
                d.classList.add("shot");
                d.open = true;
            }
        });
    }
    await page.waitForFunction(
        () =>
            [...document.querySelectorAll("details.shot, details[open][data-src]")].every(
                (d) => d.querySelector(".open").childElementCount > 0,
            ),
        null,
        { timeout },
    );
    await page.screenshot({ path: out, fullPage: true });
    if (errors.length) {
        console.error(errors.join("\n"));
        process.exitCode = 1;
    }
} finally {
    await browser.close();
}
