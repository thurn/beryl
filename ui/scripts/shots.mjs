// Captures UI states with headless Playwright WebKit; `by ui-shots` (tools/) serves the UI and
// runs this. Usage: node scripts/shots.mjs <base-url> <out-dir> <shots.json> [name...]
// Captures several at once (capture.mjs' workers) and prints each screenshot's path; exits
// non-zero if a page logs an error, cuts off text (capture.mjs' clippedText), draws the game
// otherwise than its fixture's view has it (screenFaults), crowds a control (crowdedControls),
// moves a node of the stage on the browser's time (stageFaults) or never settles; a phone
// screen (`?phone`) also if it shows text too small to read or a control too small to touch
// (phoneFaults). A desktop game screen is checked again, without a screenshot, at the windows
// of RESIZED: the stage's narrowest and widest (src/game/Stage.tsx fitStage), where its layout
// differs most from the shots' 16:9. A phone screen has a shot at each phone size instead.
// With `BERYL_STAGE` set (`by ui-shots --stage`) every page is served on the stage (plan K25):
// a shot's `staged`, if an object, then replaces those of its fields that name what only the
// old board draws, and if a string says why the shot is not taken there (the stage's own shot
// of that state, by name), until the cut-over leaves the stage's shots alone.
import { readFile } from "node:fs/promises";
import { join } from "node:path";
import {
    clippedText,
    compareSheet,
    coverage,
    crowdedControls,
    launch,
    openPage,
    phoneFaults,
    recordResult,
    screenFaults,
    settle,
    stageFaults,
    still,
    workers,
} from "./capture.mjs";
import { toldErrors } from "./told.mjs";

/** The other windows a game screen is checked at. Between the stage's narrowest and widest
 * every position slides linearly with its width, so these two stand for every window: a small
 * one squarer than 16:10, as a player might leave it, which the stage meets at its narrowest
 * with room above and below it; and a 21:9 one, which it fills at its widest. */
const RESIZED = [
    { width: 1180, height: 820 },
    { width: 2560, height: 1080 },
];

const [base, out, list, ...only] = process.argv.slice(2);
if (!base || !out || !list) {
    console.error(
        "usage: node scripts/shots.mjs <base-url> <out-dir> <shots.json> [name...]",
    );
    process.exit(2);
}

/** Whether every page is on the stage (`by ui-shots --stage`). */
const STAGED = Boolean(process.env.BERYL_STAGE);
const listed = JSON.parse(await readFile(list, "utf8")).shots;
/** The shots as this run takes them: on the stage, each as its `staged` has it, and none that
 * the stage shoots under another name. */
const shots = STAGED
    ? listed.flatMap(({ staged, ...shot }) => {
          if (typeof staged === "string") {
              if (only.includes(shot.name)) {
                  console.log(
                      `ui-shots: ${shot.name} is not the stage's: ${staged}`,
                  );
              }
              return [];
          }
          return [{ ...shot, ...staged }];
      })
    : listed;
const chosen = only.length ? shots.filter((s) => only.includes(s.name)) : shots;
const unknown = only.filter((name) => !listed.some((s) => s.name === name));
if (unknown.length) {
    console.error(`ui-shots: no shot named ${unknown.join(", ")}`);
    process.exit(2);
}

/** Captures `shot` into `out`, logging its errors: whether it had none, and what its page ran
 * when `by coverage` asked (capture.mjs' coverage). */
async function capture(browser, shot) {
    const { context, page, errors } = await openPage(browser, shot);
    const finish = await coverage(page);
    let covered = null;
    try {
        await page.goto(new URL(shot.route, base).href, { waitUntil: "load" });
        // A large fixture is still being fetched when the page first renders.
        if (shot.route.includes("fixture=")) {
            await page.waitForFunction(() => window.berylFixture);
        }
        // A state the page comes to on its own, such as the coin's flip ending.
        if (shot.waitFor) await page.waitForSelector(shot.waitFor);
        await settle(page);
        // One selector, or several clicked in turn; then the pointer leaves, so that no card
        // it rests on is inspected over the table.
        const clicks = [shot.click ?? []].flat();
        for (const click of clicks) {
            // A selector, or one with the point of its element to click: a card that shows
            // only its top edge has its middle off the screen.
            const { selector, position } =
                typeof click === "string" ? { selector: click } : click;
            await page.click(selector, { position });
            await settle(page);
        }
        if (clicks.length > 1) {
            await page.mouse.move(0, 0);
            await settle(page);
        }
        // Then the pointer rests on what `hover` selects, as a player's would to see its tip.
        if (shot.hover) {
            await page.hover(shot.hover);
            await settle(page);
        }
        // What the shot is of, where no audit of the screen would miss it: the card a
        // pointer rests on shown whole beside the table, a viewer opened.
        if (
            shot.expect &&
            !(await page.locator(shot.expect).first().isVisible())
        ) {
            errors.push(`not on screen: ${shot.expect}`);
        }
        const path = join(out, `${shot.name}.png`);
        // A comparison crops what `compare` selects side by side, rather than the whole page.
        if (shot.compare) {
            const missing = await compareSheet(page, shot.compare, path);
            for (const m of missing) errors.push(`not on screen: ${m}`);
        } else {
            await page.screenshot({ path, fullPage: true });
        }
        for (const told of await toldErrors(page))
            errors.push(`the screen tells of an error: ${told}`);
        for (const cut of await clippedText(page))
            errors.push(`text cut: ${cut}`);
        for (const fault of await screenFaults(page))
            errors.push(`screen: ${fault}`);
        for (const fault of await crowdedControls(page))
            errors.push(`controls: ${fault}`);
        for (const fault of await stageFaults(page))
            errors.push(`stage: ${fault}`);
        const phone = new URL(shot.route, base).searchParams.has("phone");
        if (phone) {
            for (const fault of await phoneFaults(page))
                errors.push(`phone: ${fault}`);
        }
        const resized = shot.route.includes("fixture=") && !phone;
        // At another size the pointer would rest on whatever has moved under it, as a hand
        // card that rises over the life total: the resized checks are of the layout alone.
        if (resized && shot.hover) await page.mouse.move(0, 0);
        for (const size of resized ? RESIZED : []) {
            await page.setViewportSize(size);
            await settle(page);
            // The table is placed afresh for the stage's new width.
            await still(page);
            const at = `at ${size.width}×${size.height}`;
            for (const cut of await clippedText(page))
                errors.push(`text cut ${at}: ${cut}`);
            for (const fault of await screenFaults(page))
                errors.push(`screen ${at}: ${fault}`);
            for (const fault of await crowdedControls(page))
                errors.push(`controls ${at}: ${fault}`);
            for (const fault of await stageFaults(page))
                errors.push(`stage ${at}: ${fault}`);
        }
        console.log(path);
    } catch (e) {
        errors.push(e.stack ?? String(e));
    } finally {
        covered = await finish();
        await context.close();
    }
    // Faults the shot is known to show, until the screen is fixed: each must still be seen.
    for (const known of shot.known ?? []) {
        const at = errors.indexOf(known);
        if (at >= 0) errors.splice(at, 1);
        else
            errors.push(
                `known fault no longer seen, so drop it from shots.json: ${known}`,
            );
    }
    if (errors.length) {
        console.error(`ui-shots: ${shot.name}: ${errors.join("\n  ")}`);
    }
    return { ok: !errors.length, covered };
}

const browser = await launch();
const run = workers();
let failed = false;
try {
    for (const shot of chosen) {
        await run(async () => {
            const started = performance.now();
            const { ok, covered } = await capture(browser, shot);
            if (!ok) failed = true;
            recordResult(
                `shots:${shot.name}`,
                ok ? "pass" : "fail",
                (performance.now() - started) / 1000,
                covered,
            );
        });
    }
    await run.drain();
} finally {
    await browser.close();
}
process.exit(failed ? 1 : 0);
