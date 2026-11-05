// Drives the game screen through a recorded fixture with headless Playwright WebKit, as a
// player would: `by ui-test` (tools/) serves the UI and runs this. Usage:
//   node scripts/interactions.mjs <base-url> <fixtures-dir>
// The fixture `greedy-turn-9` ends on a target decision for a spell the human had just cast
// from their hand; the page starts one view earlier, at the priority decision, and the fixture
// transport plays the recorded next view once the page calls `choose` (src/ipc.ts). Animation is
// measured on Playwright's clock, so the checks do not depend on the machine's speed.
// Exits non-zero on the first failed check or any page error.
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { join } from "node:path";
import { launch } from "./capture.mjs";

const [base, fixtures] = process.argv.slice(2);
if (!base || !fixtures) {
    console.error(
        "usage: node scripts/interactions.mjs <base-url> <fixtures-dir>",
    );
    process.exit(2);
}

const FIXTURE = "greedy-turn-9";
const fixture = JSON.parse(
    await readFile(join(fixtures, `${FIXTURE}.json`), "utf8"),
);
const views = fixture.messages
    .filter((m) => m.kind === "event")
    .map((m) => m.payload.view);
const [before, after] = views.slice(-2);

// The spell cast between the last two views, and the candidate that cast it.
const cast = after.events.find(
    (e) => e.kind === "zoneChange" && e.from === "hand" && e.to === "stack",
);
assert.ok(cast, "the fixture's last view follows a cast from the hand");
const slot = cast.slot;
const decision = before.status.decision;
const castIndex = decision.candidates.findIndex(
    (c) => c.option.kind === "cast" && c.option.slot === slot,
);
assert.ok(castIndex >= 0, "the view before offers the cast");
const castLabel = decision.candidates[castIndex].label;
const name = before.objects.find((o) => o.slot === slot).name;
const route = `/?fixture=${FIXTURE}&at=${views.length - 1}#/game`;
const card = `[data-slot="${slot}"]`;

/** The `choose` commands the page has sent, as recorded by the fixture transport. */
const chosen = (page) =>
    page.evaluate(() =>
        window.berylFixture.calls
            .filter((c) => c.command === "choose")
            .map((c) => c.args.request.index),
    );

/** Where an element's box is, rounded to whole stage pixels. */
const where = (handle) =>
    handle.evaluate((e) => {
        const r = e.getBoundingClientRect();
        return [Math.round(r.left), Math.round(r.top)];
    });

const browser = await launch();
const errors = [];
let failed = false;

async function check(title, body) {
    const context = await browser.newContext({
        viewport: { width: 1600, height: 900 },
        deviceScaleFactor: 1,
        reducedMotion: "no-preference",
    });
    const page = await context.newPage();
    page.on("pageerror", (e) => errors.push(`${title}: ${e.message}`));
    page.on(
        "console",
        (m) => m.type() === "error" && errors.push(`${title}: ${m.text()}`),
    );
    try {
        await page.clock.install();
        await page.goto(new URL(route, base).href, { waitUntil: "load" });
        await page.waitForSelector(`${card}[data-zone="hand"]`);
        await body(page);
        console.log(`ok: ${title}`);
    } catch (e) {
        failed = true;
        console.error(`FAILED: ${title}\n${e.stack ?? e}`);
    } finally {
        await context.close();
    }
}

await check(
    "dragging a hand card onto the battlefield casts it",
    async (page) => {
        const handle = await page.$(card);
        // Let the hand settle into its fan before picking the card up.
        await page.clock.runFor(2000);
        const box = await handle.boundingBox();
        await page.mouse.move(box.x + box.width / 2, box.y + 40);
        await page.mouse.down();
        for (let i = 1; i <= 10; i++) {
            await page.mouse.move(
                box.x +
                    box.width / 2 +
                    ((800 - box.x - box.width / 2) * i) / 10,
                box.y + 40 + ((380 - box.y - 40) * i) / 10,
            );
        }
        await page.clock.runFor(100);
        // From here on time passes only when the test says so.
        await page.clock.pauseAt(Date.now() + 60_000);
        await page.mouse.up();
        assert.deepEqual(
            await chosen(page),
            [castIndex],
            `chose "${castLabel}"`,
        );

        // The card keeps its element as it moves to the stack, and travels there over time.
        await page.waitForSelector(`${card}[data-zone="stack"]`);
        assert.ok(
            await handle.evaluate(
                (e) => e.isConnected && e.dataset.zone === "stack",
            ),
            "the cast card is the same element, now on the stack",
        );
        const dropped = await where(handle);
        await page.clock.runFor(80);
        const moving = await where(handle);
        await page.clock.runFor(5000);
        const landed = await where(handle);
        assert.notDeepEqual(
            moving,
            dropped,
            "the card has left the drop point",
        );
        assert.notDeepEqual(
            moving,
            landed,
            "the card is still on its way after 80 ms",
        );
        await page.clock.runFor(1000);
        assert.deepEqual(await where(handle), landed, "the card comes to rest");

        // The spell asks for a target: its card glows gold, the candidates cyan, and a click on
        // one picks it.
        assert.ok(
            await page.$(`${card} .card-source`),
            "the spell on the stack glows as the source",
        );
        const targets = after.status.decision.candidates;
        const target = targets.findIndex(
            (c) =>
                c.option.kind === "object" &&
                after.objects.find((o) => o.slot === c.option.slot)?.zone ===
                    "battlefield",
        );
        const targetCard = `[data-slot="${targets[target].option.slot}"]`;
        assert.ok(
            await page.$(`${targetCard} .card-target`),
            "the target glows",
        );
        await page.click(targetCard);
        assert.deepEqual(
            await chosen(page),
            [castIndex, target],
            "picked the target",
        );
    },
);

await check("clicking a hand card lists what it can do", async (page) => {
    await page.clock.runFor(2000);
    await page.click(card);
    const item = page.getByRole("menuitem", { name: castLabel, exact: true });
    await item.click();
    assert.deepEqual(await chosen(page), [castIndex], `chose "${castLabel}"`);
    await page.waitForSelector(`${card}[data-zone="stack"]`);
});

await check("hovering a card shows it whole, with its artist", async (page) => {
    await page.clock.runFor(2000);
    await page.hover(card);
    const cards = JSON.parse(
        await readFile(
            new URL("../../data/cards.json", import.meta.url),
            "utf8",
        ),
    );
    const artist = cards.cards.find((c) => c.name === name).artist;
    await page
        .locator(".inspect")
        .getByText(`Illustrated by ${artist}`)
        .waitFor();
});

await browser.close();
if (errors.length) {
    failed = true;
    console.error(`page errors:\n  ${errors.join("\n  ")}`);
}
process.exit(failed ? 1 : 0);
