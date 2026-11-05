// Drives the game screen through recorded fixtures with headless Playwright WebKit, as a
// player would: `by ui-test` (tools/) serves the UI and runs this. Usage:
//   node scripts/interactions.mjs <base-url> <fixtures-dir>
// The fixture `greedy-turn-9` ends on a target decision for a spell the human had just cast
// from their hand; the page starts one view earlier, at the priority decision, and the fixture
// transport plays the recorded views up to the next decision once the page calls `choose`
// (src/ipc.ts), which the page plays back paced (src/game/playback.ts) on Playwright's clock.
// `opponent-turn` plays out an opponent's turn to the session's halts. The combat
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
        viewport = { width: 1600, height: 900 },
    } = {},
) {
    const context = await browser.newContext({
        viewport,
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
        // What the card draws: it rises inside its element.
        const look = await handle.$(":scope > div");
        const held = center(await look.boundingBox());
        await page.mouse.down();
        const drop = { x: 800, y: 380 };
        for (let i = 1; i <= 10; i++) {
            await page.mouse.move(
                grab.x + ((drop.x - grab.x) * i) / 10,
                grab.y + ((drop.y - grab.y) * i) / 10,
            );
        }
        await page.clock.runFor(100);
        // It follows the pointer, still raised, rather than sinking back into the fan as it
        // stops being hovered.
        const followed = center(await look.boundingBox());
        assert.ok(
            Math.hypot(
                followed.x - held.x - (drop.x - grab.x),
                followed.y - held.y - (drop.y - grab.y),
            ) < 2,
            `the dragged card follows the pointer: moved by ` +
                `${followed.x - held.x}, ${followed.y - held.y} for a pointer ` +
                `moved by ${drop.x - grab.x}, ${drop.y - grab.y}`,
        );
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

await check("clicking a hand card opens its cast step", async (page) => {
    await page.clock.runFor(2000);
    await page.click(card);
    const step = page.getByRole("dialog", { name: `Cast ${name}` });
    await step.getByRole("button", { name: "Cast", exact: true }).click();
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

/** Where every card is drawn, by slot: the box of what its element draws (a raised hand card
 * rises inside its element), and whether a layout animation is moving its frame. */
const drawn = (page) =>
    page.evaluate(() =>
        Object.fromEntries(
            [...document.querySelectorAll(".card-el")].flatMap((e) => {
                const look = e.firstElementChild;
                if (!look) return [];
                const r = look.getBoundingClientRect();
                const frame = e.querySelector(".card-slot")?.style.transform;
                return [
                    [
                        e.dataset.slot,
                        {
                            box: [r.left, r.top, r.right, r.bottom],
                            animating: !!frame && frame !== "none",
                        },
                    ],
                ];
            }),
        ),
    );

const fieldCard = `[data-slot="${
    before.objects.find(
        (o) => o.zone === "battlefield" && !o.types.includes("land"),
    ).slot
}"]`;
for (const [title, target] of [
    ["a hand card", card],
    ["a battlefield card", fieldCard],
]) {
    await check(
        `hovering ${title} moves no other card and starts no layout animation`,
        async (page) => {
            await page.clock.runFor(2000);
            const still = await drawn(page);
            const slot = await page.$eval(target, (e) => e.dataset.slot);
            const box = await page.locator(target).boundingBox();
            const at = { x: box.x + box.width / 2, y: box.y + 40 };
            await page.mouse.move(at.x, at.y);
            for (let t = 0; t < 20; t++) {
                await page.clock.runFor(50);
                const now = await drawn(page);
                for (const [other, { box, animating }] of Object.entries(now)) {
                    assert.ok(!animating, `slot ${other} animates its layout`);
                    if (other === slot) continue;
                    const moved = box.map((v, i) =>
                        Math.abs(v - still[other].box[i]),
                    );
                    assert.ok(
                        Math.max(...moved) <= 1,
                        `slot ${other} moved by ${moved} at ${t * 50} ms`,
                    );
                }
            }
            const top = (await drawn(page))[slot].box[1];
            if (target === card) {
                assert.ok(
                    still[slot].box[1] - top > 40,
                    "the hovered hand card rises",
                );
            } else {
                assert.equal(top, still[slot].box[1], "a field card stays");
            }
            // The card is drawn raised inside its element, which stays put: wherever the pointer
            // found the card, it still finds it, so the hover holds rather than flickering.
            const held = await page.locator(target).boundingBox();
            for (const k of ["x", "y", "width", "height"]) {
                assert.ok(
                    Math.abs(held[k] - box[k]) <= 1,
                    `the hovered card's element moved: ${k} ${box[k]} to ${held[k]}`,
                );
            }
            assert.equal(
                await page.evaluate(
                    ({ x, y }) =>
                        document.elementFromPoint(x, y)?.closest("[data-slot]")
                            ?.dataset.slot,
                    at,
                ),
                slot,
                "the pointer is still over the hovered card",
            );
            assert.ok(await page.$(".inspect"), "the card is inspected");
        },
    );
}

// A quiet table with a row of permanents a side: nothing on the stack, no combat.
const quietViews = await load("combat-blockers");
const nonlands = (view, side) =>
    view.objects.filter(
        (o) =>
            o.zone === "battlefield" &&
            o.controller === side &&
            !o.types.includes("land"),
    );
const quiet = quietViews.findIndex(
    (v) =>
        v.status.decision?.kind === "priority" &&
        v.stack.length === 0 &&
        !v.objects.some((o) => o.combat) &&
        nonlands(v, "you").length >= 3 &&
        nonlands(v, "opponent").length >= 3,
);
assert.ok(quiet >= 0, "the blockers fixture has a quiet, crowded table");
for (const viewport of [
    { width: 1280, height: 800 },
    { width: 1600, height: 900 },
]) {
    await check(
        `each battlefield row is centered on the table at ${viewport.width} wide`,
        async (page) => {
            await page.clock.runFor(2000);
            const stage = await page.locator(".stage").boundingBox();
            const middle = stage.x + stage.width / 2;
            const boxes = async (objects) =>
                Promise.all(
                    objects.map((o) =>
                        page.locator(`[data-slot="${o.slot}"]`).boundingBox(),
                    ),
                );
            for (const side of ["you", "opponent"]) {
                const whose = side === "you" ? "your" : "the opponent's";
                const row = await boxes(nonlands(quietViews[quiet], side));
                assert.ok(row.length > 0, `${whose} row is empty`);
                const left = Math.min(...row.map((b) => b.x));
                const right = Math.max(...row.map((b) => b.x + b.width));
                assert.ok(
                    Math.abs((left + right) / 2 - middle) <= 3,
                    `${whose} row spans ${left} to ${right}, centered on ${middle}`,
                );
            }
            await assertClearOfLands(page);
        },
        {
            fixture: "combat-blockers",
            at: quiet + 1,
            ready: `[data-slot="${nonlands(quietViews[quiet], "you")[0].slot}"]`,
            viewport,
        },
    );
}

/** Asserts that no permanent on the table covers a land: a long row of permanents may pass
 * over the land piles' side of the table, but in a row of its own. */
async function assertClearOfLands(page) {
    const boxes = await page.evaluate(() =>
        [...document.querySelectorAll('[data-zone="battlefield"]')].map((e) => {
            const r = e.getBoundingClientRect();
            return {
                name: e.dataset.card,
                land: e.classList.contains("card-el-land"),

                x: r.x,
                y: r.y,
                right: r.right,
                bottom: r.bottom,
            };
        }),
    );
    for (const land of boxes.filter((b) => b.land)) {
        for (const b of boxes.filter((b) => !b.land)) {
            const across =
                Math.min(land.right, b.right) - Math.max(land.x, b.x);
            const down =
                Math.min(land.bottom, b.bottom) - Math.max(land.y, b.y);
            assert.ok(
                across <= 1 || down <= 1,
                `${b.name} covers ${land.name} by ${across} by ${down} px`,
            );
        }
    }
}

/** Whether view `v` waits for the human: the views of the game playing on without them
 * follow one another unasked. */
const awaits = (v) =>
    v.status.kind !== "thinking" && v.status.kind !== "playing";

// The first pass of the human's priority after which the opponent plays a card from their
// hand onto the table before the human next decides, as the view the pass leads to; and that
// card.
let opponentPlay = -1;
let playedSlot;
for (let i = 1; i < views.length && opponentPlay < 0; i++) {
    if (views[i - 1].status.decision?.kind !== "priority") continue;
    for (let j = i; j < views.length; j++) {
        const play = views[j].events.find((e) => played(views[j], e));
        if (play) {
            opponentPlay = i;
            playedSlot = play.slot;
            break;
        }
        if (awaits(views[j])) break;
    }
}
assert.ok(opponentPlay > 0, "the opponent plays a card from their hand");
const back = `[data-slot="${playedSlot}"]`;

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
        // The views play back paced: a frame at a time until the card leaves the hand.
        await until(page, `${back}:not([data-zone="hand"])`, 16);
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

/** Runs the page's clock `step` milliseconds at a time until `selector` matches, for at most
 * a minute of page time; returns how long that took. */
async function until(page, selector, step = 100) {
    for (let ran = 0; ran <= 60_000; ran += step) {
        if (await page.$(selector)) return ran;
        await page.clock.runFor(step);
    }
    throw new Error(`${selector} never appeared`);
}

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

// The opponent's turn played back: `opponent-turn` ends on the session's halt at the human's
// precombat main (End Turn), after one at the opponent's end step (Continue) that closes a turn
// in which the opponent plays a land and casts spells. The page starts at the human's last
// decision before that turn, a look at the top of their library.
const turnViews = await load("opponent-turn");
const halts = turnViews.flatMap((v, i) => (v.status.decision?.halt ? [i] : []));
const mainHalt = turnViews.length - 1;
const endHalt = halts.at(-2);
assert.deepEqual(
    [halts.at(-1), turnViews[endHalt]?.turn.active],
    [mainHalt, "opponent"],
    "the fixture ends on a halt at the human's main, after one in the opponent's turn",
);
const lookAt = turnViews.findLastIndex((v, i) => i < endHalt && awaits(v));
assert.equal(
    turnViews[lookAt].status.decision?.ask.kind,
    "look",
    "the human looks at their library before the opponent's turn",
);
const opponentLand = turnViews
    .slice(lookAt + 1, endHalt + 1)
    .flatMap((v) =>
        v.events.filter(
            (e) =>
                e.kind === "zoneChange" &&
                e.reason === "play" &&
                v.objects.find((o) => o.slot === e.slot)?.owner === "opponent",
        ),
    )[0];
assert.ok(opponentLand, "the opponent plays a land that turn");
const landName = turnViews[endHalt].objects.find(
    (o) => o.slot === opponentLand.slot,
).name;
// The spells that resolve into a graveyard that turn, which would otherwise vanish unseen.
const vanished = turnViews
    .slice(lookAt + 1, endHalt + 1)
    .flatMap((v) =>
        v.events.flatMap((e) =>
            e.kind === "zoneChange" &&
            e.from === "stack" &&
            e.to === "graveyard"
                ? [v.objects.find((o) => o.slot === e.slot).name]
                : [],
        ),
    );
assert.ok(vanished.length > 0, "a spell resolves into a graveyard");

await check(
    "the opponent's turn plays out at a pace, then the turn ends",
    async (page) => {
        await page.clock.runFor(2000);
        await page.clock.pauseAt(Date.now() + 60_000);
        await page.getByRole("button", { name: "Take Nothing" }).click();
        assert.deepEqual(await chosen(page), [
            turnViews[lookAt].status.decision.candidates.findIndex(
                (c) => c.option.kind === "no",
            ),
        ]);
        // The overlay gives way to the table as the game plays on.
        await until(page, ".table:not(.table-dimmed)");
        assert.equal(await page.$(".choice"), null, "the overlay is gone");

        // A tenth of a second at a time, what the board says as the turn plays out.
        const pills = new Set();
        let thinking = false;
        let landBeforeHalt = false;
        const announced = new Set();
        let took = 0;
        const land = `[data-slot="${opponentLand.slot}"][data-zone="battlefield"]`;
        const halt = page.getByRole("button", { name: "Continue" });
        while (!(await halt.isVisible())) {
            assert.ok(took < 60_000, "the opponent's turn ends");
            const pill = await page.$(".priority-pill");
            if (pill) pills.add(await pill.textContent());
            thinking ||= (await page.$(".player-badge-activity")) !== null;
            landBeforeHalt ||= (await page.$(land)) !== null;
            for (const card of await page.$$eval(".announced", (all) =>
                all.map((e) => e.dataset.card),
            )) {
                announced.add(card);
            }
            await page.clock.runFor(100);
            took += 100;
        }
        assert.ok(
            pills.has("Opponent is thinking") && thinking,
            `the opponent is seen thinking: ${[...pills].join(", ")}`,
        );
        assert.ok(
            pills.size >= 5,
            `the steps are seen passing: ${[...pills].join(", ")}`,
        );
        assert.ok(
            landBeforeHalt,
            "the land is seen played before the turn ends",
        );
        assert.ok(took >= 3000, `the turn took ${took} ms, not a jump`);
        assert.deepEqual(
            vanished.filter((card) => !announced.has(card)),
            [],
            "each spell that resolved into a graveyard was shown large",
        );
        const log = await page.locator(".game-log").textContent();
        assert.ok(
            log.includes(`Opponent played ${landName}`),
            `the log says the land was played: ${log}`,
        );

        // Continue goes on to the human's turn, which halts at their main: End Turn.
        assert.ok(await halt.isEnabled(), "Continue is the turn button");
        await halt.click();
        await until(page, "button:has-text('End Turn'):not([disabled])");
        await page.getByRole("button", { name: "End Turn" }).click();
        assert.deepEqual(
            (await chosen(page)).slice(1),
            [0, 0],
            "continued, then ended the turn",
        );
    },
    { fixture: "opponent-turn", at: lookAt + 1, ready: ".choice" },
);

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

// Blockers declared: each blocker stands across from its attacker, and the rest of its row
// makes room rather than lying under it.
const blockersViews = await load("combat-blockers");
const blockersView = blockersViews.at(-1);
const defender = blockersView.objects.find(
    (o) => o.combat?.kind === "blocking",
)?.controller;
assert.ok(defender, "the blockers fixture has a blocker");
// The defending side's nonland permanents.
const standing = blockersView.objects.filter(
    (o) =>
        o.zone === "battlefield" &&
        o.controller === defender &&
        !o.types.includes("land"),
);

await check(
    "a blocker stands across from its attacker, covering no other card",
    async (page) => {
        await page.clock.runFor(2000);
        const box = async (slot) =>
            page.locator(`[data-slot="${slot}"]`).boundingBox();
        for (const o of standing) {
            if (o.combat?.kind !== "blocking") continue;
            const mine = await box(o.slot);
            const theirs = center(await box(o.combat.attacker));
            const blockers = standing.filter(
                (b) => b.combat?.attacker === o.combat.attacker,
            ).length;
            assert.ok(
                Math.abs(center(mine).x - theirs.x) < 120 * blockers,
                `${o.name} stands across from what it blocks`,
            );
            for (const other of standing) {
                if (other.slot === o.slot) continue;
                const them = await box(other.slot);
                const overlap =
                    Math.min(mine.x + mine.width, them.x + them.width) -
                    Math.max(mine.x, them.x);
                assert.ok(
                    overlap < them.width / 2,
                    `${o.name} covers ${other.name} by ${Math.round(overlap)} px`,
                );
            }
        }
    },
    {
        fixture: "combat-blockers",
        at: blockersViews.length,
        ready: `[data-slot="${standing[0].slot}"]`,
    },
);

// A crowded row (five permanents a side, attackers raised over their neighbors) spreads into
// the table's clear right rather than laying its cards over one another's numbers.
await check(
    "every creature on a crowded table shows its power and toughness",
    async (page) => {
        await page.clock.runFor(2000);
        const hidden = await page.evaluate(() =>
            [...document.querySelectorAll('[data-zone="battlefield"]')]
                .filter((el) => !el.querySelector(".card-under"))
                .flatMap((el) => {
                    const pt = el.querySelector(".card-pt");
                    if (!pt) return [];
                    const r = pt.getBoundingClientRect();
                    const top = document
                        .elementFromPoint(r.x + r.width / 2, r.y + r.height / 2)
                        ?.closest("[data-slot]");
                    return top === el ? [] : [el.dataset.card];
                }),
        );
        assert.deepEqual(hidden, [], "power and toughness covered");
        await assertClearOfLands(page);
    },
    {
        fixture: "combat-blockers",
        at: blockersViews.length,
        ready: `[data-slot="${standing[0].slot}"]`,
    },
);

/** Drags a hand card by `by`: hovered first, so it rises clear of its neighbors in the fan,
 * then picked up there. */
async function dragFromHand(page, selector, by) {
    const box = await page.locator(selector).boundingBox();
    await page.mouse.move(box.x + box.width / 2, box.y + 40);
    await page.clock.runFor(1000);
    const raised = await page.locator(selector).boundingBox();
    const grab = { x: raised.x + raised.width / 2, y: raised.y + 40 };
    await page.mouse.move(grab.x, grab.y);
    await page.mouse.down();
    for (let i = 1; i <= 10; i++) {
        await page.mouse.move(
            grab.x + (by.x * i) / 10,
            grab.y + (by.y * i) / 10,
        );
    }
}

/** The slots of the permanents lit as the payment's, ascending. */
const lit = (page) =>
    page.$$eval(".card-paying", (els) =>
        els.map((e) => Number(e.closest("[data-slot]").dataset.slot)).sort(),
    );
const ascending = (slots) => [...slots].sort();

/** Your permanents tapped in `after` and not in `before`, ascending. */
function newlyTapped(before, after) {
    const was = new Set(
        before.objects.filter((o) => o.tapped).map((o) => o.slot),
    );
    return ascending(
        after.objects
            .filter(
                (o) =>
                    o.zone === "battlefield" &&
                    o.controller === "you" &&
                    o.tapped &&
                    !was.has(o.slot),
            )
            .map((o) => o.slot),
    );
}

// Two cards alike in hand are one candidate to the engine (architecture §7.2): both glow, and
// either plays.
const pairViews = await load("hand-pair");
const pairView = pairViews.at(-1);
const nameAt = (v, slot) => v.objects.find((o) => o.slot === slot)?.name;
const hand = pairView.objects.filter(
    (o) => o.zone === "hand" && o.owner === "you",
);
const playable = new Map(
    pairView.status.decision.candidates.flatMap((c, i) =>
        ["cast", "playLand", "activate"].includes(c.option.kind) &&
        hand.some((o) => o.slot === c.option.slot)
            ? [[nameAt(pairView, c.option.slot), { index: i, ...c }]]
            : [],
    ),
);
const twin = hand.find(
    (o) =>
        playable.get(o.name)?.option.kind === "playLand" &&
        playable.get(o.name).option.slot !== o.slot,
);
assert.ok(twin, "the pair fixture holds a land twice");

await check(
    "every hand card with something to play glows, twins included",
    async (page) => {
        await page.clock.runFor(2000);
        for (const o of hand) {
            const glows = await page.$(
                `[data-slot="${o.slot}"] .card-playable`,
            );
            assert.equal(
                glows !== null,
                playable.has(o.name),
                `${o.name} in slot ${o.slot} glows as playable`,
            );
        }
        // The twin the engine did not name plays the land too.
        await dragFromHand(page, `[data-slot="${twin.slot}"]`, {
            x: 0,
            y: -320,
        });
        await page.mouse.up();
        assert.deepEqual(await settled(page, 1), [
            playable.get(twin.name).index,
        ]);
    },
    {
        fixture: "hand-pair",
        at: pairViews.length,
        ready: `[data-slot="${twin.slot}"][data-zone="hand"]`,
    },
);

// Thieving Skydiver kicked: the cast with kicker and its X in one step, where the engine asks
// the cast and then X.
const kickViews = await load("cast-kicker");
const [kickBefore, kickAt] = kickViews.slice(-2);
const kickCasts = kickBefore.status.decision.candidates;
const kicked = kickCasts.findIndex(
    (c) => c.option.kind === "cast" && c.option.costs.length > 0,
);
assert.ok(kicked >= 0, "the kicker fixture offers the kicked cast");
const skydiver = `[data-slot="${kickCasts[kicked].option.slot}"]`;
const xs = kickAt.status.decision.candidates;
// One more than the least X, as far as the mana goes.
const wantX = xs[Math.min(1, xs.length - 1)].option.n;

await check(
    "Thieving Skydiver is cast with kicker and X in one step",
    async (page) => {
        await page.clock.runFor(2000);
        await dragFromHand(page, skydiver, { x: 0, y: -320 });
        await page.mouse.up();
        const step = page.getByRole("dialog", {
            name: "Cast Thieving Skydiver",
        });
        await step.waitFor();
        assert.deepEqual(await chosen(page), [], "the drop asks first");
        await step.getByRole("radio", { name: /^Kicker/ }).click();
        for (let x = xs[0].option.n; x < wantX; x++) {
            await step.getByRole("button", { name: "Increase X" }).click();
        }
        assert.equal(
            await step.locator(".cast-x-value").textContent(),
            String(wantX),
        );
        const taps = kickCasts[kicked].payments.find((p) => p.x === wantX).taps;
        assert.deepEqual(await lit(page), ascending(taps), "lit to pay");
        await step.getByRole("button", { name: "Cast", exact: true }).click();
        assert.deepEqual(
            await settled(page, 2),
            [kicked, xs.findIndex((c) => c.option.n === wantX)],
            "cast with kicker, then X",
        );
        assert.equal(
            await page.getByText("Choose X").count(),
            0,
            "X is not asked again",
        );
    },
    { fixture: "cast-kicker", at: kickViews.length - 1, ready: skydiver },
);

/** A fixture stopped at a payment plan and the view after: the cast before it, the plan the
 * stand-in chose and the permanents that tapped for it. */
async function payment(name) {
    const views = await load(name);
    const at = views.findIndex(
        (v) => v.status.decision?.kind === "paymentPlan",
    );
    const [plans, after] = [views[at], views[at + 1]];
    let castAt = at - 1;
    while (views[castAt].status.decision?.kind !== "priority") castAt--;
    const before = views[castAt];
    const spell = plans.stack.at(-1);
    const castIndex = before.status.decision.candidates.findIndex(
        (c) => c.option.kind === "cast" && c.option.slot === spell.slot,
    );
    const tapped = newlyTapped(plans, after);
    const chosenPlan = plans.status.decision.candidates.findIndex(
        (c) =>
            JSON.stringify(ascending(c.payments[0].taps)) ===
            JSON.stringify(tapped),
    );
    assert.ok(chosenPlan >= 0, `${name}: the tapped lands are a plan's`);
    return {
        views,
        castAt,
        spell: `[data-slot="${spell.slot}"]`,
        cast: before.status.decision.candidates[castIndex],
        castIndex,
        between: views
            .slice(castAt + 1, at)
            .filter((v) => v.status.kind === "deciding"),
        chosenPlan,
        tapped,
    };
}

const byDefault = await payment("pay-default");
assert.equal(byDefault.chosenPlan, 0, "the stand-in paid the default way");
assert.equal(byDefault.between.length, 0, "nothing asked before payment");
await check(
    "dragging a card lights the lands it will tap, then taps them unasked",
    async (page) => {
        await page.clock.runFor(2000);
        await dragFromHand(page, byDefault.spell, { x: 0, y: -320 });
        assert.deepEqual(
            await lit(page),
            byDefault.tapped,
            "lit while dragged: the lands that tap",
        );
        await page.mouse.up();
        assert.deepEqual(
            await settled(page, 2),
            [byDefault.castIndex, 0],
            "cast, then paid the default way",
        );
        assert.equal(
            await page.getByText(/Payment \d/).count(),
            0,
            "no payment prompt",
        );
    },
    {
        fixture: "pay-default",
        at: byDefault.castAt + 1,
        ready: byDefault.spell,
    },
);

const byChoice = await payment("pay-other");
assert.notEqual(byChoice.chosenPlan, 0, "the stand-in paid another way");
const otherTaps = byChoice.cast.payments.find(
    (p) => p.plan === byChoice.chosenPlan,
).taps;
const clicked = otherTaps.find(
    (slot) => !byChoice.cast.payments[0].taps.includes(slot),
);
await check(
    "the cast step pays with a permanent clicked on the board",
    async (page) => {
        await page.clock.runFor(2000);
        await page.click(byChoice.spell);
        const step = page.getByRole("dialog", { name: /^Cast / });
        await step.waitFor();
        assert.deepEqual(
            await lit(page),
            ascending(byChoice.cast.payments[0].taps),
            "the default payment lit first",
        );
        await page.click(`[data-slot="${clicked}"]`);
        assert.deepEqual(await lit(page), byChoice.tapped, "the clicked way");
        await step.getByRole("button", { name: "Cast", exact: true }).click();
        // The decisions before the payment (its targets) are the player's, on the board.
        const picks = [byChoice.castIndex];
        for (const v of byChoice.between) {
            const next = byChoice.views[byChoice.views.indexOf(v) + 1];
            const target = next.stack.at(-1).targets.at(-1);
            const index = v.status.decision.candidates.findIndex(
                (c) => c.option.slot === target.slot,
            );
            assert.deepEqual(await settled(page, picks.length), picks);
            await page.clock.runFor(2000);
            await page.click(`[data-slot="${target.slot}"]`);
            picks.push(index);
        }
        assert.deepEqual(
            await settled(page, picks.length + 1),
            [...picks, byChoice.chosenPlan],
            "paid the clicked way when asked",
        );
        assert.equal(await page.getByText(/Payment \d/).count(), 0);
    },
    {
        fixture: "pay-other",
        at: byChoice.castAt + 1,
        ready: byChoice.spell,
    },
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

// The scry's source, Aang's Iceberg, has four paragraphs of rules: they shrink to fit its box.
await check(
    "long rules text fits its card rather than being cut off",
    async (page) => {
        await page.evaluate(() => document.fonts.ready.then(() => {}));
        await page.clock.runFor(2000);
        const cut = await page.evaluate(() =>
            [...document.querySelectorAll(".card-rules p")]
                .filter((p) => {
                    const box = p.parentElement.getBoundingClientRect();
                    return p.getBoundingClientRect().bottom > box.bottom + 1;
                })
                .map((p) => p.textContent.slice(0, 30)),
        );
        assert.deepEqual(cut, [], "rules text overflowing its box");
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
