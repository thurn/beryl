// Drives the game screen through recorded fixtures with headless Playwright WebKit, as a
// player would: `by ui-test` (tools/) serves the UI and runs this. Usage:
//   node scripts/interactions.mjs <base-url> <fixtures-dir>
// The fixture `greedy-turn-9` ends on a target decision for a spell the human had just cast
// from their hand; the page starts one view earlier, at the priority decision, and the fixture
// transport plays the recorded next view once the page calls `choose` (src/ipc.ts). The combat
// fixtures end on the human's attack declaration (`combat-attack`) and on the attacker for a
// blocker just declared (`combat-block`). Animation is measured on Playwright's clock, so the
// checks do not depend on the machine's speed. The `choice-*` fixtures end on decisions the
// choice overlays ask. Exits non-zero on the first failed check or any
// page error.
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

/** A recorded fixture's views, oldest first. */
async function load(name) {
    const fixture = JSON.parse(
        await readFile(join(fixtures, `${name}.json`), "utf8"),
    );
    return fixture.messages
        .filter((m) => m.kind === "event")
        .map((m) => m.payload.view);
}

const FIXTURE = "greedy-turn-9";
const views = await load(FIXTURE);
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

/** The center of a bounding box. */
const center = (box) => ({
    x: box.x + box.width / 2,
    y: box.y + box.height / 2,
});

const browser = await launch();
const errors = [];
let failed = false;

/** Runs `body` on a fresh page of a fixture's first `at` views, once `ready` matches. */
async function check(
    title,
    body,
    {
        fixture = FIXTURE,
        at = views.length - 1,
        ready = `${card}[data-zone="hand"]`,
    } = {},
) {
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
        const route = `/?fixture=${fixture}&at=${at}#/game`;
        await page.goto(new URL(route, base).href, { waitUntil: "load" });
        await page.waitForSelector(ready);
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
        const grab = { x: box.x + box.width / 2, y: box.y + 40 };
        await page.mouse.move(grab.x, grab.y);
        // Hovered, the card rises under the pointer; then it is picked up there.
        await page.clock.runFor(1000);
        const held = center(await handle.boundingBox());
        // From here on time passes only when the test says so.
        await page.clock.pauseAt(Date.now() + 60_000);
        await page.mouse.down();
        const drop = { x: 800, y: 380 };
        for (let i = 1; i <= 10; i++) {
            await page.mouse.move(
                grab.x + ((drop.x - grab.x) * i) / 10,
                grab.y + ((drop.y - grab.y) * i) / 10,
            );
        }
        await page.clock.runFor(100);
        // It follows the pointer rather than sinking back into the fan as it stops being
        // hovered. (It straightens and shrinks back from its hover scale, hence the slack.)
        const followed = center(await handle.boundingBox());
        assert.ok(
            Math.hypot(
                followed.x - held.x - (drop.x - grab.x),
                followed.y - held.y - (drop.y - grab.y),
            ) < 20,
            `the dragged card follows the pointer: moved by ` +
                `${followed.x - held.x}, ${followed.y - held.y} for a pointer ` +
                `moved by ${drop.x - grab.x}, ${drop.y - grab.y}`,
        );
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

// The first view in which the opponent plays a card from their hand onto the table, at the
// human's priority, and that card.
const opponentPlay = views.findIndex(
    (v, i) =>
        views[i - 1]?.status.decision?.kind === "priority" &&
        v.events.some((e) => played(v, e)),
);
assert.ok(opponentPlay > 0, "the opponent plays a card from their hand");
const back = `[data-slot="${views[opponentPlay].events.find((e) => played(views[opponentPlay], e)).slot}"]`;

/** Whether event `e` of view `v` moves an opponent's card from their hand to the table. */
function played(v, e) {
    return (
        e.kind === "zoneChange" &&
        e.from === "hand" &&
        ["battlefield", "graveyard", "stack"].includes(e.to) &&
        v.objects.find((o) => o.slot === e.slot)?.owner === "opponent"
    );
}

await check(
    "an opponent's card grows out of its back rather than jumping in size",
    async (page) => {
        await page.clock.runFor(2000);
        const handle = await page.$(back);
        const { width } = await handle.boundingBox();
        await page.clock.pauseAt(Date.now() + 60_000);
        await page.getByRole("button", { name: /^(Next|End Turn)/ }).click();
        await page.waitForSelector(`${back}:not([data-zone="hand"])`);
        // A frame in, it has barely begun to turn and grow (a jump would nearly double it).
        await page.clock.runFor(16);
        const first = await handle.boundingBox();
        assert.ok(
            Math.abs(first.width / width - 1) < 0.1,
            `as it leaves the hand it is ${first.width} wide, as its back was ${width}`,
        );
        await page.clock.runFor(3000);
        const landed = await handle.boundingBox();
        assert.ok(
            Math.abs(landed.width - width) > 10,
            "it then takes its new size",
        );
    },
    { at: opponentPlay, ready: `${back}[data-zone="hand"]` },
);

/** Drags the element `from` matches by its center to `to`'s center, or by `by`, in steps. */
async function drag(page, from, { to, by }) {
    const start = center(await page.locator(from).boundingBox());
    const end = to
        ? center(await page.locator(to).boundingBox())
        : { x: start.x + by.x, y: start.y + by.y };
    await page.mouse.move(start.x, start.y);
    await page.mouse.down();
    for (let i = 1; i <= 10; i++) {
        await page.mouse.move(
            start.x + ((end.x - start.x) * i) / 10,
            start.y + ((end.y - start.y) * i) / 10,
        );
    }
    await page.mouse.up();
}

/** The `choose` commands sent once the page has had time to answer, at most `count`. */
async function settled(page, count) {
    for (let i = 0; i < 20 && (await chosen(page)).length < count; i++) {
        await page.clock.runFor(100);
    }
    return chosen(page);
}

// The attack declaration: the creature that may attack, and what declares it.
const attackViews = await load("combat-attack");
const attackDecision = attackViews.at(-1).status.decision;
const attackIndex = attackDecision.candidates.findIndex(
    (c) => c.option.kind === "attack",
);
assert.ok(attackIndex >= 0, "the attack fixture offers an attacker");
const attacker = `[data-slot="${attackDecision.candidates[attackIndex].option.slot}"]`;
const attackRun = {
    fixture: "combat-attack",
    at: attackViews.length,
    ready: attacker,
};

await check(
    "dragging a creature forward declares it an attacker",
    async (page) => {
        await page.clock.runFor(2000);
        // Not far enough: it goes back to its place.
        await drag(page, attacker, { by: { x: 0, y: -20 } });
        assert.deepEqual(
            await settled(page, 1),
            [],
            "a nudge declares nothing",
        );
        await page.clock.runFor(2000);
        await drag(page, attacker, { by: { x: 0, y: -150 } });
        assert.deepEqual(await settled(page, 1), [attackIndex], "attacks");
    },
    attackRun,
);

await check(
    "clicking a creature that may attack declares it",
    async (page) => {
        await page.clock.runFor(2000);
        assert.ok(
            await page.$(`${attacker} .card-target`),
            "the creature that may attack glows",
        );
        await page.click(attacker);
        assert.deepEqual(await settled(page, 1), [attackIndex]);
    },
    attackRun,
);

await check(
    "the turn button finishes the attack declaration",
    async (page) => {
        await page.getByRole("button", { name: "No Attacks" }).click();
        const done = attackDecision.candidates.findIndex(
            (c) => c.option.kind === "done",
        );
        assert.deepEqual(await settled(page, 1), [done]);
    },
    attackRun,
);

// The block declaration: the blocker the stand-in declared, and the attackers it may block.
const blockViews = await load("combat-block");
const [blockBefore, blockAfter] = blockViews.slice(-2);
const blockerSlot = blockAfter.objects.find(
    (o) => o.controller === "you" && o.combat?.kind === "blocking",
).slot;
const blockIndex = blockBefore.status.decision.candidates.findIndex(
    (c) => c.option.kind === "block" && c.option.slot === blockerSlot,
);
assert.ok(blockIndex >= 0, "the block fixture declares a blocker");
const blockTargets = blockAfter.status.decision.candidates;
const blocked = blockTargets.length - 1;
const blocker = `[data-slot="${blockerSlot}"]`;
const blockedCard = `[data-slot="${blockTargets[blocked].option.slot}"]`;
const blockRun = {
    fixture: "combat-block",
    at: blockViews.length - 1,
    ready: blocker,
};

await check(
    "dragging a blocker onto an attacker blocks that attacker",
    async (page) => {
        await page.clock.runFor(2000);
        await drag(page, blocker, { to: blockedCard });
        assert.deepEqual(
            await settled(page, 2),
            [blockIndex, blocked],
            "declared the blocker, then picked the attacker it was dropped on",
        );
    },
    blockRun,
);

await check(
    "clicking a blocker, then an attacker, blocks it",
    async (page) => {
        await page.clock.runFor(2000);
        await page.click(blocker);
        assert.deepEqual(await settled(page, 1), [blockIndex]);
        await page.clock.runFor(2000);
        assert.ok(
            await page.$(`${blockedCard} .card-target`),
            "the attackers it may block glow",
        );
        await page.click(blockedCard);
        assert.deepEqual(await settled(page, 2), [blockIndex, blocked]);
    },
    blockRun,
);

// The choice overlays: a scry, Jace's put-back and a pay-or-counter prompt, each the last view
// of its fixture. An overlay sends the first pick of its plan at once; the rest wait for the
// decisions they answer, which these fixtures don't record.
/** A fixture's last decision, and the candidate index of each card it offers, by slot. */
async function lastDecision(name) {
    const views = await load(name);
    const decision = views.at(-1).status.decision;
    const bySlot = new Map(
        decision.candidates.flatMap((c, i) =>
            c.option.kind === "object" ? [[c.option.slot, i]] : [],
        ),
    );
    return { views, decision, bySlot };
}
const tray = (slot) => `.choice-zone [data-slot="${slot}"]`;

const scry = await lastDecision("choice-scry");
const [scryTop, scryNext] = scry.views
    .at(-1)
    .players.you.library.slice(-2)
    .reverse();
await check(
    "clicking a scried card sends it to the bottom",
    async (page) => {
        await page.clock.runFor(2000);
        const before = await page.locator(tray(scryTop)).boundingBox();
        await page.click(tray(scryTop));
        await page.clock.runFor(2000);
        const after = await page.locator(tray(scryTop)).boundingBox();
        const library = await page.locator(".choice-library").boundingBox();
        assert.ok(
            after.x + after.width / 2 < library.x,
            "the card moved left of the library, to the bottom",
        );
        assert.ok(after.x < before.x, "the card moved left");
        await page.getByRole("button", { name: "Done" }).click();
        assert.deepEqual(
            await settled(page, 1),
            [scry.bySlot.get(scryTop)],
            "the first pick puts that card on the bottom",
        );
    },
    { fixture: "choice-scry", at: scry.views.length, ready: tray(scryNext) },
);

const jace = await lastDecision("choice-jace");
const [first, second] = [...jace.bySlot.keys()];
await check(
    "dragging two cards onto the library puts them back in order",
    async (page) => {
        await page.clock.runFor(2000);
        const done = page.getByRole("button", { name: "Done" });
        assert.ok(await done.isDisabled(), "Done waits for two cards");
        const zone = await page.locator(".choice-zone").boundingBox();
        const end = {
            x: zone.x + zone.width - 30,
            y: zone.y + zone.height / 2,
        };
        for (const slot of [first, second]) {
            // By its left edge, which the fanned cards after it leave showing.
            const box = await page.locator(tray(slot)).boundingBox();
            const grab = { x: box.x + 20, y: box.y + box.height / 2 };
            await page.mouse.move(grab.x, grab.y);
            await page.mouse.down();
            for (let i = 1; i <= 10; i++) {
                await page.mouse.move(
                    grab.x + ((end.x - grab.x) * i) / 10,
                    grab.y,
                );
            }
            await page.mouse.up();
            await page.clock.runFor(2000);
        }
        assert.ok(await done.isEnabled(), "two cards are on the library");
        await done.click();
        assert.deepEqual(
            await settled(page, 1),
            [jace.bySlot.get(first)],
            "the card under the top one goes back first",
        );
    },
    { fixture: "choice-jace", at: jace.views.length, ready: tray(first) },
);

const pay = await lastDecision("choice-pay");
const decline = pay.decision.candidates.findIndex(
    (c) => c.option.kind === "no",
);
await check(
    "a pay-or-counter prompt answers from its buttons",
    async (page) => {
        await page.getByRole("button", { name: "Don't Pay" }).click();
        assert.deepEqual(await settled(page, 1), [decline]);
    },
    { fixture: "choice-pay", at: pay.views.length, ready: ".choice" },
);

await check(
    "View Battlefield puts the overlay aside until asked back",
    async (page) => {
        await page.getByRole("button", { name: "View Battlefield" }).click();
        assert.equal(await page.$(".choice"), null, "the overlay is aside");
        assert.equal(
            await page.$(".table-dimmed"),
            null,
            "the battlefield is clear",
        );
        await page.click(".overlay-return");
        await page.waitForSelector(".choice");
        assert.deepEqual(await chosen(page), [], "peeking answers nothing");
    },
    { fixture: "choice-pay", at: pay.views.length, ready: ".choice" },
);

await browser.close();
if (errors.length) {
    failed = true;
    console.error(`page errors:\n  ${errors.join("\n  ")}`);
}
process.exit(failed ? 1 : 0);
