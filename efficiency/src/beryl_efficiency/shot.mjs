// Renders one address of the efficiency page to a PNG for `just efficiency shot` (serve.py).
// Usage: node shot.mjs <ui-folder> <url> <out.png> <width> <light|dark>
// Playwright and the machine's shared browser slots come from the UI's own scripts, so this
// file needs no packages of its own. It opens every line of the page that has more detail,
// one level deep, only when the url asks with `#open`.
import { createRequire } from "node:module";
import { join } from "node:path";
import { pathToFileURL } from "node:url";

const [ui, url, out, width, scheme] = process.argv.slice(2);
const require = createRequire(join(ui, "package.json"));
const { webkit } = require("playwright");
const { takeSlot } = await import(
    pathToFileURL(join(ui, "scripts", "slots.mjs")).href
);

const release = await takeSlot(process.env, 1);
const browser = await webkit.launch();
try {
    const page = await browser.newPage({
        viewport: { width: Number(width), height: 800 },
        colorScheme: scheme,
    });
    const errors = [];
    page.on("pageerror", (e) => errors.push(String(e)));
    await page.goto(url, { waitUntil: "load" });
    if (new URL(url).hash === "#open") {
        // Only the lines the page had at first: a section that opens brings lines of its
        // own, which stay closed.
        await page.$$eval("details[data-src]", (lines) => {
            for (const d of lines) {
                d.classList.add("shot");
                d.open = true;
            }
        });
        await page.waitForFunction(
            () =>
                [...document.querySelectorAll("details.shot")].every(
                    (d) => d.querySelector(".open").childElementCount > 0,
                ),
            null,
            { timeout: 120_000 },
        );
    }
    await page.screenshot({ path: out, fullPage: true });
    if (errors.length) {
        console.error(errors.join("\n"));
        process.exitCode = 1;
    }
} finally {
    await browser.close();
    release();
}
