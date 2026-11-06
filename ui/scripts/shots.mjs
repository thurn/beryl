// Captures UI states with headless Playwright WebKit; `by ui-shots` (tools/) serves the UI and
// runs this. Usage: node scripts/shots.mjs <base-url> <out-dir> <shots.json> [name...]
// Prints each screenshot's path; exits non-zero if a page logs an error, cuts off text
// (capture.mjs' clippedText) or never settles.
import { readFile } from "node:fs/promises";
import { join } from "node:path";
import { clippedText, launch, openPage, settle } from "./capture.mjs";

const [base, out, list, ...only] = process.argv.slice(2);
if (!base || !out || !list) {
    console.error(
        "usage: node scripts/shots.mjs <base-url> <out-dir> <shots.json> [name...]",
    );
    process.exit(2);
}

const { shots } = JSON.parse(await readFile(list, "utf8"));
const chosen = only.length ? shots.filter((s) => only.includes(s.name)) : shots;
const unknown = only.filter((name) => !shots.some((s) => s.name === name));
if (unknown.length) {
    console.error(`ui-shots: no shot named ${unknown.join(", ")}`);
    process.exit(2);
}

const browser = await launch();
let failed = false;
try {
    for (const shot of chosen) {
        const { context, page, errors } = await openPage(browser, shot);
        await page.goto(new URL(shot.route, base).href, { waitUntil: "load" });
        // A large fixture is still being fetched when the page first renders.
        if (shot.route.includes("fixture=")) {
            await page.waitForFunction(() => window.berylFixture);
        }
        await settle(page);
        // One selector, or several clicked in turn; then the pointer leaves, so that no card
        // it rests on is inspected over the table.
        const clicks = [shot.click ?? []].flat();
        for (const selector of clicks) {
            await page.click(selector);
            await settle(page);
        }
        if (clicks.length > 1) {
            await page.mouse.move(0, 0);
            await settle(page);
        }
        const path = join(out, `${shot.name}.png`);
        await page.screenshot({ path, fullPage: true });
        for (const cut of await clippedText(page))
            errors.push(`text cut: ${cut}`);
        if (errors.length) {
            failed = true;
            console.error(`ui-shots: ${shot.name}: ${errors.join("\n  ")}`);
        }
        console.log(path);
        await context.close();
    }
} finally {
    await browser.close();
}
process.exit(failed ? 1 : 0);
