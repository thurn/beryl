// Drives the game screen through recorded fixtures with headless Playwright WebKit, as a
// player would: `by ui-test` (tools/) serves the UI and runs this. Usage:
//   node scripts/interactions.mjs <base-url> <fixtures-dir>
// The fixture `greedy-turn-9` ends on a target decision for a spell the human had just cast
// from their hand; the page starts one view earlier, at the priority decision, and the fixture
// transport plays the recorded views up to the next decision once the page calls `choose`
// (src/ipc.ts), which the page plays back paced (src/game/playback.ts) on Playwright's clock.
// `opponent-turn` plays out an opponent's turn to the session's halts. The combat fixtures
// `combat-declare` and `combat-double-block` end past the human's attack and block
// declarations, which the page makes whole and sends one pick at a time. Animation is
// measured on Playwright's clock, so the checks do not depend on the machine's speed, nor on
// how many run at once. The `choice-*` fixtures end on decisions the choice overlays ask. Exits
// non-zero if a check fails or a page logs an error.
import assert from "node:assert/strict";
import { readdir, readFile, writeFile } from "node:fs/promises";
import { basename, join } from "node:path";
import { slotOf as offeredSlot, twins } from "../src/game/decisions.ts";
import { cutText, launch, workers } from "./capture.mjs";

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

// Every fixture recorded for this run, before this script writes one of its own.
const recorded = (await readdir(fixtures))
    .filter((f) => f.endsWith(".json"))
    .map((f) => f.slice(0, -".json".length));

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

const queue = workers();

/** Runs `body` on a fresh page of a fixture's first `at` views, once `ready` matches, alongside
 * other checks (capture.mjs' workers): awaiting it waits only for a free worker. The page's
 * fixtures come from `fixtures`, not from what Vite serves. */
async function check(
    title,
    body,
    {
        fixture = FIXTURE,
        at = views.length - 1,
        ready = `${card}[data-zone="hand"]`,
        viewport = { width: 1600, height: 900 },
        reducedMotion = "no-preference",
        hash = "#/game",
    } = {},
) {
    await queue(async () => {
        const context = await browser.newContext({
            viewport,
            deviceScaleFactor: 1,
            reducedMotion,
        });
        await context.route("**/fixtures/*.json", (route) =>
            route.fulfill({
                path: join(
                    fixtures,
                    decodeURIComponent(
                        basename(new URL(route.request().url()).pathname),
                    ),
                ),
            }),
        );
        const page = await context.newPage();
        page.on("pageerror", (e) => errors.push(`${title}: ${e.message}`));
        page.on(
            "console",
            (m) => m.type() === "error" && errors.push(`${title}: ${m.text()}`),
        );
        try {
            await page.clock.install();
            const route = `/?fixture=${fixture}&at=${at}${hash}`;
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
    });
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

/** The `cancel` commands the page has sent. */
const cancels = (page) =>
    page.evaluate(
        () =>
            window.berylFixture.calls.filter((c) => c.command === "cancel")
                .length,
    );

assert.equal(
    decision.candidates[castIndex].payments.length,
    1,
    "the cast is paid one way: nothing for a cast step to ask",
);
await check(
    "a click casts a card with nothing to choose, and Escape takes it back",
    async (page) => {
        await page.clock.runFor(2000);
        await page.click(card);
        assert.deepEqual(
            await chosen(page),
            [castIndex],
            `chose "${castLabel}"`,
        );
        await page.waitForSelector(`${card}[data-zone="stack"]`);
        // It asks for its target; Escape takes the cast back, and the card is in hand again.
        const targeting = page.getByRole("dialog", {
            name: `Choose a target for ${name}`,
        });
        await targeting.waitFor();
        await page.keyboard.press("Escape");
        await page.waitForSelector(`${card}[data-zone="hand"]`);
        assert.equal(await cancels(page), 1, "cancelled");
        assert.equal(await targeting.count(), 0, "the target is not asked");
        // Cast again, its prompt's Cancel button takes it back too.
        await page.clock.runFor(2000);
        await page.click(card);
        await targeting
            .getByRole("button", { name: "Cancel", exact: true })
            .click();
        await page.waitForSelector(`${card}[data-zone="hand"]`);
        assert.equal(await cancels(page), 2, "cancelled again");
        assert.deepEqual(await chosen(page), [castIndex, castIndex]);
    },
);

await check(
    "the spell being cast, dragged back to the hand, is taken back",
    async (page) => {
        await page.clock.runFor(2000);
        await page.click(card);
        const onStack = `${card}[data-zone="stack"]`;
        await page.waitForSelector(onStack);
        await page
            .getByRole("dialog", { name: `Choose a target for ${name}` })
            .waitFor();
        await page.clock.runFor(1000);
        const from = center(await page.locator(onStack).boundingBox());
        await drag(page, onStack, { by: { x: 700 - from.x, y: 870 - from.y } });
        await page.waitForSelector(`${card}[data-zone="hand"]`);
        assert.equal(await cancels(page), 1, "cancelled");
    },
);

await check("Space passes priority", async (page) => {
    await page.clock.runFor(2000);
    await page.keyboard.press("Space");
    const pass = decision.candidates.findIndex((c) => c.option.kind === "pass");
    assert.deepEqual(await chosen(page), [pass], "passed");
});

await check(
    "the turn button is reached by Tab, with a visible ring",
    async (page) => {
        await page.clock.runFor(2000);
        const turn = page.locator(".turn-action button");
        for (let i = 0; i < 40; i++) {
            if (await turn.evaluate((b) => b === document.activeElement)) break;
            await page.keyboard.press("Tab");
        }
        assert.ok(
            await turn.evaluate((b) => b === document.activeElement),
            "Tab reaches the turn button",
        );
        const ring = await turn.evaluate(
            (b) => getComputedStyle(b).outlineStyle,
        );
        assert.notEqual(ring, "none", "the focused turn button shows a ring");
    },
);

await check("Escape closes the settings", async (page) => {
    await page.clock.runFor(2000);
    await page.getByRole("button", { name: "Settings" }).click();
    const panel = page.locator(".settings");
    await panel.waitFor();
    await page.keyboard.press("Escape");
    assert.equal(await panel.count(), 0, "closed");
});

await check("dragging across the table selects no text", async (page) => {
    await page.clock.runFor(2000);
    const selected = () =>
        page.evaluate(() => window.getSelection()?.toString() ?? "");
    // From the opponent's name, over the log and the table, to the near corner.
    const badge = await page
        .locator(".player-badge")
        .getByText("Opponent", { exact: true })
        .boundingBox();
    await page.mouse.move(badge.x + 2, badge.y + badge.height / 2);
    await page.mouse.down();
    await page.mouse.move(1580, 880, { steps: 25 });
    await page.mouse.up();
    assert.equal(await selected(), "", "nothing selected by a drag");
    // A hand card picked up, carried over the log and put back.
    const box = await page.locator(card).boundingBox();
    await page.mouse.move(box.x + box.width / 2, box.y + 40);
    await page.mouse.down();
    await page.mouse.move(60, 300, { steps: 15 });
    await page.mouse.move(box.x + box.width / 2, 880, { steps: 15 });
    await page.mouse.up();
    assert.equal(await selected(), "", "nothing selected by a card's drag");
});

await check(
    "a click on the visible strip of each hand card acts on that card",
    async (page) => {
        await page.clock.runFor(2000);
        // Each hand card's strip in the fan, at rest: where it is the card the pointer hits,
        // as a row of points across the near edge.
        const strips = await page.evaluate(() => {
            const cards = [
                ...document.querySelectorAll('.card-el-hand[data-zone="hand"]'),
            ];
            const y = innerHeight - 40;
            const out = [];
            for (const el of cards) {
                const box = el.getBoundingClientRect();
                const xs = [];
                for (let x = Math.ceil(box.left); x < box.right; x += 2) {
                    const hit = document.elementFromPoint(x, y);
                    if (hit?.closest("[data-slot]") === el) xs.push(x);
                }
                out.push({ slot: Number(el.dataset.slot), xs, y });
            }
            return out.sort((a, b) => a.xs[0] - b.xs[0]);
        });
        assert.ok(strips.length >= 3, "a fan of cards");
        // Every click is caught before the board hears of it, and the card it lands on noted.
        await page.evaluate(() => {
            window.hits = [];
            for (const type of ["pointerdown", "pointerup", "click"]) {
                window.addEventListener(
                    type,
                    (e) => {
                        if (type === "pointerdown") {
                            const el = e.target.closest?.("[data-slot]");
                            window.hits.push(Number(el?.dataset.slot));
                        }
                        e.stopPropagation();
                    },
                    true,
                );
            }
        });
        const mid = ({ xs, y }) => ({ x: xs[xs.length >> 1], y });
        for (const [i, strip] of strips.entries()) {
            assert.ok(strip.xs.length > 4, `slot ${strip.slot} shows a strip`);
            // With each neighbor raised, the edge of the strip next to it.
            for (const n of [i - 1, i + 1]) {
                const neighbor = strips[n];
                if (!neighbor) continue;
                const raised = mid(neighbor);
                await page.mouse.move(raised.x, raised.y, { steps: 4 });
                await page.clock.runFor(500);
                const x =
                    n < i ? strip.xs[0] + 4 : strip.xs[strip.xs.length - 1] - 4;
                await page.mouse.move(x, strip.y, { steps: 4 });
                await page.clock.runFor(500);
                await page.mouse.down();
                await page.mouse.up();
                const hits = await page.evaluate(() => window.hits);
                assert.equal(
                    hits.at(-1),
                    strip.slot,
                    `a click on slot ${strip.slot}'s strip beside raised slot ${neighbor.slot}`,
                );
            }
        }
    },
);

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
 * rises inside its element, which a copy over the hand shows), and whether a layout animation
 * is moving its frame. */
const drawn = (page) =>
    page.evaluate(() =>
        Object.fromEntries(
            [...document.querySelectorAll(".card-el[data-slot]")].flatMap(
                (e) => {
                    const look = e.firstElementChild;
                    if (!look) return [];
                    const r = look.getBoundingClientRect();
                    const frame =
                        e.querySelector(".card-slot")?.style.transform;
                    return [
                        [
                            e.dataset.slot,
                            {
                                box: [r.left, r.top, r.right, r.bottom],
                                animating: !!frame && frame !== "none",
                            },
                        ],
                    ];
                },
            ),
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
        `each battlefield row is centered as near as the stack allows at ${viewport.width} wide`,
        async (page) => {
            await page.clock.runFor(2000);
            const stage = await page.locator(".stage").boundingBox();
            const middle = stage.x + stage.width / 2;
            // A row too long to center left of the stack's column (layout.ts ROOM, which the
            // stack's coming and going never changes) reaches that column's edge instead,
            // short of it by no more than the lean of its last card.
            const scale = stage.width / 1600;
            const roomEnd = stage.x + 1160 * scale;
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
                const centered = Math.abs((left + right) / 2 - middle) <= 3;
                const flush =
                    (left + right) / 2 < middle &&
                    Math.abs(right - roomEnd) <= 12 * scale;
                assert.ok(
                    centered || flush,
                    `${whose} row spans ${left} to ${right}, centered on ${middle} ` +
                        `or reaching ${roomEnd}`,
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

// On a table whose rows reach into the log's column, the log folds to a button rather than lie
// half hidden under the permanents; the button opens it over them, whole, and closes it again.
await check(
    "the log folds away from a crowded table and opens over it",
    async (page) => {
        await page.clock.runFor(2000);
        const list = page.locator(".game-log-list");
        const toggle = page.getByRole("button", { name: "Show log" });
        assert.ok(await list.isHidden(), "the log is folded");
        assert.deepEqual(await cutText(page), [], "no text is cut or covered");
        await toggle.click();
        await page.clock.runFor(500);
        assert.ok(await list.isVisible(), "the button opens the log");
        const box = await list.boundingBox();
        const under = await page.$$eval(
            ".card-layer .card-el-field",
            (els, b) =>
                els.some((e) => {
                    const r = e.getBoundingClientRect();
                    return (
                        r.left < b.x + b.width &&
                        r.top < b.y + b.height &&
                        r.bottom > b.y
                    );
                }),
            box,
        );
        assert.ok(under, "the open log lies over permanents");
        assert.deepEqual(await cutText(page), [], "the open log shows whole");
        await page.getByRole("button", { name: "Hide log" }).click();
        assert.ok(await list.isHidden(), "the button folds it again");
    },
    {
        fixture: "combat-double-block",
        // The table as the block declaration begins.
        at:
            (await load("combat-double-block")).findIndex(
                (v) => v.status.decision?.kind === "blocker",
            ) + 1,
        ready: ".game-log-toggle",
    },
);

// A fixture's history reaches the log once, however often the screen starts listening (React's
// StrictMode mounts it twice), so no turn's heading comes twice.
await check(
    "the log tells a fixture's history once",
    async (page) => {
        const turns = await page.locator(".game-log-turn").allTextContents();
        assert.ok(turns.length > 0, "the log heads its turns");
        assert.deepEqual(
            turns,
            [...new Set(turns)],
            `each turn is headed once: ${await page.locator(".game-log").textContent()}`,
        );
    },
    { fixture: "opponent-turn", at: 22, ready: ".game-log-entry" },
);

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

await check(
    "Space during the opponent's turn shows the rest of it at once",
    async (page) => {
        await page.clock.runFor(2000);
        // From here on time passes only when the test says so: the replay cannot advance.
        await page.clock.pauseAt(Date.now() + 60_000);
        await page.getByRole("button", { name: "Take Nothing" }).click();
        await page.waitForSelector(".table:not(.table-dimmed)");
        const halt = page.getByRole("button", { name: "Continue" });
        assert.ok(!(await halt.isVisible()), "the turn is still playing out");
        await page.keyboard.press("Space");
        // The turn's last view at once, with nothing it did lost.
        await halt.waitFor();
        assert.ok(await halt.isEnabled(), "Continue is the turn button");
        await page.waitForSelector(
            `[data-slot="${opponentLand.slot}"][data-zone="battlefield"]`,
        );
        const log = await page.locator(".game-log").textContent();
        assert.ok(
            log.includes(`Opponent played ${landName}`),
            `the log says the land was played: ${log}`,
        );
        assert.equal((await chosen(page)).length, 1, "Space passed nothing");
        // The next press acts on it.
        await page.keyboard.press("Space");
        assert.deepEqual((await chosen(page)).slice(1), [0], "continued");
    },
    { fixture: "opponent-turn", at: lookAt + 1, ready: ".choice" },
);

// A pass that the game answers with several views before the human's next priority, at which
// they may play a land that was in their hand all along.
const replayed = views.flatMap((v, i) => {
    const next = views.findIndex((w, j) => j > i && awaits(w));
    const between = views.slice(i + 1, next).filter((w) => w.events.length);
    const pass = v.status.decision?.candidates.findIndex(
        (c) => c.option.kind === "pass",
    );
    const land = views[next]?.status.decision?.candidates.findIndex(
        (c) =>
            c.option.kind === "playLand" &&
            v.objects.some(
                (o) => o.slot === c.option.slot && o.zone === "hand",
            ),
    );
    return v.status.decision?.kind === "priority" &&
        pass >= 0 &&
        between.length >= 3 &&
        land >= 0
        ? [{ at: i, pass, next, land }]
        : [];
})[0];
assert.ok(replayed, "a pass is followed by several views, then a land drop");
const replayedLand = `[data-slot="${
    views[replayed.next].status.decision.candidates[replayed.land].option.slot
}"]`;

await check(
    "a land clicked while the game plays on to it is played",
    async (page) => {
        await page.clock.runFor(2000);
        await page.clock.pauseAt(Date.now() + 60_000);
        await page.keyboard.press("Space");
        assert.deepEqual(await chosen(page), [replayed.pass], "passed");
        const box = await page.locator(replayedLand).boundingBox();
        assert.ok(
            !(await page.$(`${replayedLand}.card-el-playable`)),
            "the land is not yet playable",
        );
        // Pressed during the replay, which ends; let go at the land drop, which plays it.
        await page.mouse.click(box.x + box.width / 2, box.y + 40);
        assert.deepEqual(
            await chosen(page),
            [replayed.pass, replayed.land],
            "played the land",
        );
    },
    {
        at: replayed.at + 1,
        ready: `${replayedLand}[data-zone="hand"]`,
    },
);

/** A recorded combat declaration: the last run of the human's decisions of `kinds` in
 * `views`, the `at` that opens a page on its first, the candidate each was answered with, as
 * the view after it shows (the creature newly attacking or blocking, the attacker the blocker
 * waiting for one blocks, or done), and the view after the last, which shows the whole
 * declaration before combat moves on. */
function recordedDeclaration(views, kinds) {
    const asked = views.flatMap((v, i) =>
        v.status.kind === "deciding" ? [[v, i]] : [],
    );
    const of = (k) => kinds.includes(asked[k][0].status.decision.kind);
    let last = asked.length - 1;
    while (!of(last)) last -= 1;
    let first = last;
    while (first > 0 && of(first - 1)) first -= 1;
    const combat = (view, slot) =>
        view.objects.find((o) => o.slot === slot)?.combat;
    const picks = [];
    for (let k = first; k <= last; k++) {
        const [view] = asked[k];
        const next = views[asked[k][1] + 1] ?? views.at(-1);
        const { candidates } = view.status.decision;
        const waiting = view.objects.find(
            (o) => o.combat?.kind === "blocking" && o.combat.attacker === null,
        );
        const index = candidates.findIndex(({ option: o }) =>
            o.kind === "attack" || o.kind === "block"
                ? !combat(view, o.slot) &&
                  combat(next, o.slot)?.kind ===
                      (o.kind === "attack" ? "attacking" : "blocking")
                : o.kind === "object" &&
                  combat(next, waiting.slot)?.attacker === o.slot,
        );
        picks.push(
            index >= 0
                ? index
                : candidates.findIndex((c) => c.option.kind === "done"),
        );
    }
    return {
        at: asked[first][1] + 1,
        view: asked[first][0],
        picks,
        declared: views[asked[last][1] + 1] ?? views.at(-1),
    };
}

const slotOf = (slot) => `[data-slot="${slot}"]`;
/** Whether the card in `slot` shows `role` ("attacker" or "blocker") on the table. */
const shows = async (page, slot, role) =>
    Boolean(await page.$(`${slotOf(slot)} .card-${role}`));
const named = (view, name) =>
    view.objects
        .filter((o) => o.zone === "battlefield" && o.name === name)
        .map((o) => o.slot);

// The attack declaration (`combat-declare`): Elite Spellbinder, Malcolm and two Insect tokens
// attack, of six creatures that may, the engine offering one of the twin tokens at a time.
const attackViews = await load("combat-declare");
const attackRecord = recordedDeclaration(attackViews, ["attacker"]);
const attackView = attackRecord.view;
const declaredAttackers = attackRecord.declared.objects
    .filter((o) => o.controller === "you" && o.combat?.kind === "attacking")
    .map((o) => o.slot);
const insects = named(attackView, "Insect Token");
assert.equal(insects.length, 2, "two Insect tokens");
assert.deepEqual(
    [...declaredAttackers].sort((a, b) => a - b),
    [
        ...named(attackView, "Elite Spellbinder"),
        ...named(attackView, "Malcolm, Alluring Scoundrel"),
        ...insects,
    ].sort((a, b) => a - b),
    "the fixture declares the four attackers",
);
// Every creature of yours that may attack: the candidates and their twins.
const mayAttack = attackView.objects.filter(
    (o) =>
        o.zone === "battlefield" &&
        o.controller === "you" &&
        o.types.includes("creature") &&
        !o.tapped &&
        !o.summoningSick,
);
assert.ok(
    mayAttack.length > declaredAttackers.length,
    "some creature that may attack stays home",
);
const offeredAttackers = attackView.status.decision.candidates.flatMap((c) =>
    c.option.kind === "attack" ? [c.option.slot] : [],
);
assert.ok(
    insects.some((slot) => !offeredAttackers.includes(slot)),
    "the engine offers one of the twin tokens",
);
const attackRun = {
    fixture: "combat-declare",
    at: attackRecord.at,
    ready: slotOf(insects[1]),
};

await check(
    "every creature that may attack glows, identical tokens included",
    async (page) => {
        await page.clock.runFor(2000);
        for (const o of mayAttack) {
            assert.ok(
                await page.$(`${slotOf(o.slot)} .card-target`),
                `${o.name} (${o.slot}) glows`,
            );
        }
    },
    attackRun,
);

await check(
    "attackers picked in any order are declared together",
    async (page) => {
        await page.clock.runFor(2000);
        // The twin the engine does not offer first, then the rest from right to left.
        const order = [
            insects[1],
            ...[...declaredAttackers]
                .filter((s) => s !== insects[1])
                .sort((a, b) => b - a),
        ];
        for (const slot of order) {
            await page.click(slotOf(slot));
            await page.clock.runFor(500);
        }
        // Changing one's mind: a second click takes an attacker back, a third returns it.
        const malcolm = named(attackView, "Malcolm, Alluring Scoundrel")[0];
        await page.click(slotOf(malcolm));
        await page.clock.runFor(500);
        assert.ok(!(await shows(page, malcolm, "attacker")), "taken back");
        await page.click(slotOf(malcolm));
        await page.clock.runFor(500);
        for (const slot of declaredAttackers) {
            assert.ok(await shows(page, slot, "attacker"), `${slot} attacks`);
        }
        assert.deepEqual(await chosen(page), [], "nothing is sent yet");
        await page.getByText(`${declaredAttackers.length} attacking`).waitFor();
        await page.getByRole("button", { name: "Attack", exact: true }).click();
        assert.deepEqual(
            await settled(page, attackRecord.picks.length),
            attackRecord.picks,
            "the engine's picks, one attacker at a time, then done",
        );
        // The views that follow show the declaration.
        await page.clock.runFor(2000);
        for (const o of mayAttack) {
            assert.equal(
                await shows(page, o.slot, "attacker"),
                declaredAttackers.includes(o.slot),
                `${o.name} (${o.slot})`,
            );
        }
    },
    attackRun,
);

await check(
    "Attack with All picks every creature that may attack; Clear none",
    async (page) => {
        await page.clock.runFor(2000);
        await page.getByRole("button", { name: "Attack with All" }).click();
        await page.clock.runFor(500);
        await page.getByText(`${mayAttack.length} attacking`).waitFor();
        await page.getByRole("button", { name: "Clear" }).click();
        await page.clock.runFor(500);
        await page.getByRole("button", { name: "No Attacks" }).waitFor();
        assert.deepEqual(await chosen(page), []);
    },
    attackRun,
);

await check(
    "dragging a creature forward joins the attack",
    async (page) => {
        await page.clock.runFor(2000);
        const spellbinder = named(attackView, "Elite Spellbinder")[0];
        // Not far enough: it goes back to its place.
        await drag(page, slotOf(spellbinder), { by: { x: 0, y: -20 } });
        await page.clock.runFor(1000);
        assert.ok(!(await shows(page, spellbinder, "attacker")), "a nudge");
        await drag(page, slotOf(spellbinder), { by: { x: 0, y: -150 } });
        await page.clock.runFor(1000);
        assert.ok(await shows(page, spellbinder, "attacker"), "attacks");
        assert.deepEqual(await chosen(page), []);
    },
    attackRun,
);

// The block declaration (`combat-double-block`): two blockers on each of two attackers.
const blockViews = await load("combat-double-block");
const blockRecord = recordedDeclaration(blockViews, ["blocker", "blockTarget"]);
const blockView = blockRecord.view;
const declaredBlocks = new Map(
    blockRecord.declared.objects
        .filter((o) => o.combat?.kind === "blocking")
        .map((o) => [o.slot, o.combat.attacker]),
);
const doubled = [...declaredBlocks.values()].find(
    (a) => [...declaredBlocks.values()].filter((b) => b === a).length === 2,
);
assert.ok(doubled !== undefined, "the fixture blocks one attacker twice");
assert.equal(
    new Set(declaredBlocks.values()).size,
    2,
    "and another attacker once",
);
const blockRun = {
    fixture: "combat-double-block",
    at: blockRecord.at,
    ready: slotOf(doubled),
};
const nameIn = (view, slot) => view.objects.find((o) => o.slot === slot).name;

await check(
    "blockers paired with attackers in any order are declared together",
    async (page) => {
        await page.clock.runFor(2000);
        const banner = page.locator(".step-banner-detail");
        assert.equal(
            await banner.textContent(),
            "Click a creature to block with",
        );
        // From the last blocker to the first: each, then its attacker.
        const pairs = [...declaredBlocks].sort(([a], [b]) => b - a);
        for (const [blocker, attacker] of pairs) {
            await page.click(slotOf(blocker));
            await page.clock.runFor(500);
            assert.equal(
                await banner.textContent(),
                `Click the attacker ${nameIn(blockView, blocker)} blocks`,
            );
            assert.ok(
                await page.$(`${slotOf(attacker)} .card-target`),
                "the attackers it may block glow",
            );
            await page.click(slotOf(attacker));
            await page.clock.runFor(500);
            assert.ok(
                await shows(page, blocker, "blocker"),
                `${blocker} blocks`,
            );
        }
        // Changing one's mind: a blocker clicked again waits for an attacker anew.
        const [again, its] = pairs[0];
        await page.click(slotOf(again));
        await page.clock.runFor(500);
        assert.ok(!(await shows(page, again, "blocker")), "taken back");
        await page.click(slotOf(its));
        await page.clock.runFor(500);
        assert.deepEqual(await chosen(page), [], "nothing is sent yet");
        await page.getByText(`${declaredBlocks.size} blocking`).waitFor();
        await page.getByRole("button", { name: "Block", exact: true }).click();
        assert.deepEqual(
            await settled(page, blockRecord.picks.length),
            blockRecord.picks,
            "the engine's picks, each blocker then its attacker, then done",
        );
    },
    blockRun,
);

await check(
    "a blocker blocks only an attacker it may, and can be dragged onto it",
    async (page) => {
        await page.clock.runFor(2000);
        // A blocker that may block the doubled attacker but not another, as a creature without
        // flying or reach may not block a flyer (CR 702.9b): by the engine's preview, which
        // names one of identical attackers, so one named like none it may block.
        const nameOf = (slot) => nameIn(blockView, slot);
        const attacking = blockView.objects
            .filter((o) => o.combat?.kind === "attacking")
            .map((o) => o.slot);
        const [blocker, unblockable] = blockView.status.decision.candidates
            .filter((c) => c.option.kind === "block")
            .map((c) => {
                const names = new Set(c.blocks.map(nameOf));
                return [
                    c.option.slot,
                    c.blocks.includes(doubled)
                        ? attacking.find((a) => !names.has(nameOf(a)))
                        : undefined,
                ];
            })
            .find(([, a]) => a !== undefined);
        assert.ok(blocker !== undefined, "a blocker that may not block all");
        await page.click(slotOf(blocker));
        await page.clock.runFor(500);
        assert.ok(!(await page.$(`${slotOf(unblockable)} .card-target`)));
        await page.click(slotOf(unblockable));
        await page.clock.runFor(500);
        assert.ok(!(await shows(page, blocker, "blocker")), "no block");
        await drag(page, slotOf(blocker), { to: slotOf(doubled) });
        await page.clock.runFor(1000);
        assert.ok(
            await shows(page, blocker, "blocker"),
            "dragged on, it blocks",
        );
        assert.deepEqual(await chosen(page), []);
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

// A face-down permanent a side, the human's tapped: each in a permanent's frame (the field's
// short one, not a card back's full height), showing its 2/2, over no land.
const faceDown = blockersView.objects.filter(
    (o) => o.zone === "battlefield" && o.facing === "down",
);
for (const side of ["you", "opponent"]) {
    assert.ok(
        faceDown.some((o) => o.controller === side),
        `the blockers fixture has a face-down permanent for ${side}`,
    );
}
assert.ok(
    faceDown.some((o) => o.tapped),
    "the blockers fixture has a tapped face-down permanent",
);
// The face-up permanents: a tapped one draws in a frame of its own size.
const faceUp = blockersView.objects.filter(
    (o) =>
        o.zone === "battlefield" &&
        o.facing === "up" &&
        !o.types.includes("land"),
);
await check(
    "a face-down permanent stands in a permanent's frame, a 2/2",
    async (page) => {
        await page.clock.runFor(2000);
        const el = (o) => page.locator(`.card-el[data-slot="${o.slot}"]`);
        // Untransformed sizes, which a tapped card's turn leaves alone.
        const size = (o) =>
            el(o).evaluate((e) => [e.offsetWidth, e.offsetHeight]);
        for (const o of faceDown) {
            const like = faceUp.find((u) => u.tapped === o.tapped);
            assert.ok(like, `a face-up permanent is tapped as ${o.slot} is`);
            const [width, height] = await size(like);
            assert.match(
                await el(o).getAttribute("class"),
                /card-el-field/,
                `${o.slot} is drawn as a permanent`,
            );
            const [w, h] = await size(o);
            assert.ok(
                Math.abs(w - width) <= 1 && Math.abs(h - height) <= 1,
                `${o.slot} is ${w} by ${h}, ${like.name} ${width} by ${height}`,
            );
            assert.equal(
                await el(o).locator(".card-pt").textContent(),
                "2/2",
                `${o.slot} shows its power and toughness`,
            );
        }
        await assertClearOfLands(page);
    },
    {
        fixture: "combat-blockers",
        at: blockersViews.length,
        ready: `[data-slot="${faceDown[0].slot}"]`,
    },
);

// A table with tapped and untapped lands and creatures, and a card Portable Hole exiled.
const beneathViews = await load("exiled-beneath");
const beneathView = beneathViews.at(-1);
const onField = beneathView.objects.filter((o) => o.zone === "battlefield");
const exiled = beneathView.objects.find(
    (o) => o.zone === "exile" && o.exiledBy !== null,
);
assert.ok(exiled, "the exiled-beneath fixture has a card exiled beneath");
for (const [kind, tapped] of [
    ["land", true],
    ["land", false],
    ["creature", true],
]) {
    assert.ok(
        onField.some((o) => o.types.includes(kind) && o.tapped === tapped),
        `the exiled-beneath fixture has ${tapped ? "a tapped" : "an untapped"} ${kind}`,
    );
}
const beneathRun = {
    fixture: "exiled-beneath",
    at: beneathViews.length,
    ready: `[data-slot="${exiled.slot}"]`,
};

await check(
    "a tapped card turns whole, keeping its frame's shape",
    async (page) => {
        await page.clock.runFor(2000);
        for (const o of onField) {
            const { frame, turn, slot } = await page
                .locator(`[data-slot="${o.slot}"] .card`)
                .evaluate((card) => {
                    // Unrounded, untransformed sizes.
                    const aspect = (e) => {
                        const style = getComputedStyle(e);
                        return (
                            parseFloat(style.width) / parseFloat(style.height)
                        );
                    };
                    const m = new DOMMatrix(getComputedStyle(card).transform);
                    return {
                        frame: aspect(card),
                        turn: (Math.atan2(m.b, m.a) * 180) / Math.PI,
                        slot: aspect(card.parentElement),
                    };
                });
            const what = `${o.name} (${o.tapped ? "tapped" : "untapped"})`;
            assert.ok(
                Math.abs(frame - 1.3) < 0.01,
                `${what}'s frame is ${frame} wide for its height`,
            );
            assert.ok(
                Math.abs(turn - (o.tapped ? 90 : 0)) < 0.5,
                `${what} is turned ${turn}°`,
            );
            assert.ok(
                Math.abs(slot - (o.tapped ? 1 / 1.3 : 1.3)) < 0.01,
                `${what} takes a footprint ${slot} wide for its height`,
            );
        }
        await assertClearOfLands(page);
    },
    beneathRun,
);

await check(
    "a card exiled by a permanent peeks out from beneath it, previewed on hover",
    async (page) => {
        await page.clock.runFor(2000);
        const under = page.locator(`[data-slot="${exiled.slot}"]`);
        const host = page.locator(`[data-slot="${exiled.exiledBy}"]`);
        const [mine, theirs] = [
            await under.boundingBox(),
            await host.boundingBox(),
        ];
        assert.ok(
            mine.x < theirs.x && mine.y < theirs.y,
            `${exiled.name} peeks out up and to the left of what exiled it`,
        );
        const z = (l) => l.evaluate((e) => Number(getComputedStyle(e).zIndex));
        assert.ok((await z(under)) < (await z(host)), "it lies beneath");
        // Its peeking strip, left of what exiled it.
        const at = { x: (mine.x + theirs.x) / 2, y: mine.y + mine.height / 2 };
        assert.equal(
            await page.evaluate(
                ({ x, y }) =>
                    document.elementFromPoint(x, y)?.closest("[data-slot]")
                        ?.dataset.slot,
                at,
            ),
            String(exiled.slot),
            "its strip shows",
        );
        await page.mouse.move(at.x, at.y);
        await page.waitForSelector(
            `.inspect[aria-label="${exiled.name}, inspected"]`,
        );
    },
    beneathRun,
);

// The human's Swift Reconfiguration on the opponent's Sphinx of False Conclusions (or whatever the
// fixture attaches across the table): an Aura one player controls on a permanent the other does.
const attachedView = (await load("attached-across")).at(-1);
const attachedBySlot = new Map(attachedView.objects.map((o) => [o.slot, o]));
const aura = attachedView.objects.find(
    (o) =>
        o.zone === "battlefield" &&
        o.attachedTo !== null &&
        attachedBySlot.get(o.attachedTo)?.controller !== o.controller,
);
assert.ok(aura, "the attached-across fixture attaches across the table");
const auraHost = attachedBySlot.get(aura.attachedTo);

await check(
    "an attachment is drawn behind its host, linked on hover",
    async (page) => {
        await page.clock.runFor(2000);
        const mine = page.locator(`[data-slot="${aura.slot}"]`);
        const host = page.locator(`[data-slot="${auraHost.slot}"]`);
        const [a, h] = [await mine.boundingBox(), await host.boundingBox()];
        // Their boxes overlap or touch, in the host's row rather than its controller's.
        assert.ok(
            a.x <= h.x + h.width &&
                h.x <= a.x + a.width &&
                a.y <= h.y + h.height &&
                h.y <= a.y + a.height,
            `${aura.name} touches ${auraHost.name}`,
        );
        assert.ok(
            Math.abs(center(a).y - center(h).y) < h.height / 2,
            `${aura.name} lies in ${auraHost.name}'s row`,
        );
        const z = (l) => l.evaluate((e) => Number(getComputedStyle(e).zIndex));
        assert.ok((await z(mine)) < (await z(host)), "it lies behind");
        // Its name bar, above the host's top edge.
        const strip = { x: a.x + a.width * 0.4, y: (a.y + h.y) / 2 };
        assert.equal(
            await page.evaluate(
                ({ x, y }) =>
                    document.elementFromPoint(x, y)?.closest("[data-slot]")
                        ?.dataset.slot,
                strip,
            ),
            String(aura.slot),
            "its name bar shows above its host",
        );
        // Hovering either rings the other.
        await page.mouse.move(strip.x, strip.y);
        await host.locator(".card-linked").waitFor();
        await page.mouse.move(center(h).x, h.y + h.height * 0.75);
        await mine.locator(".card-linked").waitFor();
        // The host's inspector shows its types now and what is attached to it.
        const inspect = page.locator(
            `.inspect[aria-label="${auraHost.name}, inspected"]`,
        );
        await inspect.locator(`[data-attached="${aura.slot}"]`).waitFor();
        const types = (await inspect.locator(".inspect-types").innerText())
            .toLowerCase()
            .split(/\s+/);
        for (const t of auraHost.types) {
            assert.ok(
                types.includes(t),
                `the inspector shows ${t} in ${types}`,
            );
        }
    },
    {
        fixture: "attached-across",
        at: (await load("attached-across")).length,
        ready: `[data-slot="${aura.slot}"]`,
    },
);

// A permanent with more than one use (a planeswalker's loyalty abilities) lists them where it
// is clicked, each a button.
const uses = new Map();
attachedView.status.decision.candidates.forEach((c, index) => {
    if (c.option.kind !== "activate") return;
    uses.set(c.option.slot, [...(uses.get(c.option.slot) ?? []), index]);
});
const [walker, abilities] = [...uses].find(([, a]) => a.length > 1) ?? [];
assert.ok(
    walker !== undefined,
    "the attached-across fixture offers two abilities",
);
await check(
    "a card's menu lists its uses as buttons",
    async (page) => {
        await page.clock.runFor(2000);
        await page.click(`[data-slot="${walker}"]`);
        const menu = page.locator(".action-menu");
        const buttons = menu.getByRole("button");
        assert.equal(await buttons.count(), abilities.length, "one per use");
        const label =
            attachedView.status.decision.candidates[abilities[1]].label;
        await menu
            .getByRole("button", {
                name: label.slice(label.lastIndexOf(": ") + 2),
            })
            .click();
        assert.deepEqual(
            await chosen(page),
            [abilities[1]],
            `chose "${label}"`,
        );
    },
    {
        fixture: "attached-across",
        at: (await load("attached-across")).length,
        ready: `[data-slot="${walker}"]`,
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

const landActs = pairView.status.decision.candidates.filter(
    (c) => c.option.slot === playable.get(twin.name).option.slot,
);
assert.equal(landActs.length, 1, "the land's one use is to be played");
await check(
    "one click plays a land with one use",
    async (page) => {
        await page.clock.runFor(2000);
        await page.click(`[data-slot="${twin.slot}"]`);
        assert.deepEqual(await settled(page, 1), [
            playable.get(twin.name).index,
        ]);
        assert.equal(await page.locator(".action-menu").count(), 0, "no menu");
    },
    {
        fixture: "hand-pair",
        at: pairViews.length,
        ready: `[data-slot="${twin.slot}"][data-zone="hand"]`,
    },
);

// The pair's land played by dragging the twin the engine did not name: the named one goes to
// the battlefield, then (the opponent's Oust, say) the dragged one goes into the library and
// is drawn again. Each view after the drop is the recorded one changed so.
const pairLand = playable.get(twin.name).option.slot;
const zoned = (v, moves) => ({
    ...v,
    events: [],
    objects: v.objects.map((o) =>
        o.slot in moves ? { ...o, zone: moves[o.slot] } : o,
    ),
});
const landPlayed = {
    ...zoned(pairView, { [pairLand]: "battlefield" }),
    battlefield: [pairLand],
    status: {
        kind: "deciding",
        decision: {
            ...pairView.status.decision,
            candidates: pairView.status.decision.candidates.filter(
                (c) => c.option.kind !== "playLand",
            ),
        },
    },
};
const you = pairView.players.you;
const twinMoved = (from, to, reason) => [
    { kind: "zoneChange", slot: twin.slot, from, to, reason },
];
const playedOn = [
    landPlayed,
    {
        ...zoned(landPlayed, { [twin.slot]: "library" }),
        players: {
            ...pairView.players,
            you: { ...you, library: [twin.slot, ...you.library] },
        },
        status: { kind: "playing" },
        events: twinMoved("hand", "library", "effect"),
    },
    { ...landPlayed, events: twinMoved("library", "hand", "draw") },
];
{
    const fixture = JSON.parse(
        await readFile(join(fixtures, "hand-pair.json"), "utf8"),
    );
    const event = fixture.messages.findLast((m) => m.kind === "event");
    // The land played, then the pass.
    for (let i = 0; i < 2; i++) {
        fixture.messages.push({
            kind: "response",
            command: "choose",
            value: null,
        });
    }
    for (const view of playedOn) {
        fixture.messages.push({
            ...event,
            payload: { ...event.payload, view },
        });
    }
    await writeFile(
        join(fixtures, "hand-pair-played.json"),
        JSON.stringify(fixture),
    );
}

/** Where each of your hand cards is drawn, by slot, once the page has settled. */
async function fan(page) {
    await page.clock.runFor(2000);
    return page.$$eval(
        '.card-el-hand[data-zone="hand"]:not(.card-ghost)',
        (cards) =>
            Object.fromEntries(
                cards.map((e) => {
                    const r = e
                        .querySelector(".hand-card")
                        .getBoundingClientRect();
                    return [e.dataset.slot, [r.x, r.y]];
                }),
            ),
    );
}

/** Asserts that every hand card in `seen` is at its place in `rest`, within 2 pixels. */
function inFan(seen, rest, when) {
    assert.deepEqual(Object.keys(seen).sort(), Object.keys(rest).sort(), when);
    for (const [slot, [x, y]] of Object.entries(seen)) {
        const off = Math.hypot(x - rest[slot][0], y - rest[slot][1]);
        assert.ok(off <= 2, `${when}: slot ${slot} is ${off} px off its place`);
    }
}

await check(
    "a dragged twin goes back to its place in the fan, also once drawn again",
    async (page) => {
        await page.clock.runFor(2000);
        await dragFromHand(page, `[data-slot="${twin.slot}"]`, {
            x: 0,
            y: -320,
        });
        await page.mouse.up();
        await until(page, `[data-slot="${pairLand}"][data-zone="battlefield"]`);
        // The pointer stays over the battlefield, off the hand.
        const played = await fan(page);
        await page.keyboard.press("Space");
        await until(page, `[data-slot="${twin.slot}"][data-zone="library"]`);
        await until(page, `[data-slot="${twin.slot}"][data-zone="hand"]`);
        const drawn = await fan(page);
        // Each table's hand at rest, never touched.
        const rest = async (at) => {
            await page.goto(
                new URL(`/?fixture=hand-pair-played&at=${at}#/game`, base).href,
                { waitUntil: "load" },
            );
            await page.waitForSelector('[data-zone="hand"]');
            return fan(page);
        };
        inFan(played, await rest(pairViews.length + 1), "the land played");
        inFan(drawn, await rest(pairViews.length + 3), "the twin drawn again");
    },
    {
        fixture: "hand-pair-played",
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

// Subtlety, cast for its mana or with evoke, whose exile from the hand is asked as a cost while
// the cast can still be taken back.
const evokeViews = await load("cast-evoke");
const evokeCasts = evokeViews.at(-2).status.decision.candidates;
const evoked = evokeCasts.findIndex(
    (c) => c.option.kind === "cast" && c.option.costs.length > 0,
);
assert.ok(evoked >= 0, "the evoke fixture offers the evoked cast");
const exiling = evokeViews.at(-1).status.decision;
assert.equal(exiling.kind, "exileCost");
assert.ok(exiling.cancellable, "the exile is asked while the cast is unpaid");
const subtlety = `[data-slot="${evokeCasts[evoked].option.slot}"]`;
const evokeFixture = {
    fixture: "cast-evoke",
    at: evokeViews.length - 1,
    ready: `${subtlety}[data-zone="hand"]`,
};

/** Picks up the hand card `selector` matches, raised, and lets it go at `to`. */
async function dragHandCardTo(page, selector, to) {
    const box = await page.locator(selector).boundingBox();
    await page.mouse.move(box.x + box.width / 2, box.y + 40);
    await page.clock.runFor(1000);
    const raised = await page.locator(selector).boundingBox();
    const grab = { x: raised.x + raised.width / 2, y: raised.y + 40 };
    await page.mouse.move(grab.x, grab.y);
    await page.mouse.down();
    for (let i = 1; i <= 10; i++) {
        await page.mouse.move(
            grab.x + ((to.x - grab.x) * i) / 10,
            grab.y + ((to.y - grab.y) * i) / 10,
        );
    }
    await page.mouse.up();
}

/** The enabled primary buttons on screen. */
const primaries = (page) =>
    page.$$eval(
        ".button-primary",
        (els) =>
            els.filter((b) => !b.disabled && b.getClientRects().length > 0)
                .length,
    );

await check(
    "the cast step is the one primary, and its card dragged back takes it back",
    async (page) => {
        await page.clock.runFor(2000);
        await page.click(subtlety);
        const step = page.getByRole("dialog", { name: "Cast Subtlety" });
        await step.waitFor();
        assert.equal(await primaries(page), 1, "the cast step's Cast alone");
        assert.ok(
            await page.locator(".turn-action button").isDisabled(),
            "the turn button waits for the cast",
        );
        // Let go in the hand, the card goes back and the step closes, casting nothing.
        await dragHandCardTo(page, subtlety, { x: 700, y: 870 });
        assert.equal(await step.count(), 0, "the cast step closed");
        assert.deepEqual(await chosen(page), [], "nothing cast");
        assert.ok(
            await page.locator(".turn-action button").isEnabled(),
            "the turn button passes again",
        );
        // Let go on the battlefield, it is cast as the step had it.
        await page.click(subtlety);
        await step.getByRole("radio", { name: /^Evoke/ }).click();
        await dragHandCardTo(page, subtlety, { x: 800, y: 380 });
        assert.deepEqual(await settled(page, 1), [evoked], "cast with evoke");
    },
    evokeFixture,
);

await check(
    "an evoke's exile is taken back by Escape, a right-click, Cancel and a drag",
    async (page) => {
        const overlay = page.locator(".overlay.choice");
        const evoke = async () => {
            await page.clock.runFor(2000);
            await page.click(subtlety);
            const step = page.getByRole("dialog", { name: "Cast Subtlety" });
            await step.getByRole("radio", { name: /^Evoke/ }).click();
            await step
                .getByRole("button", { name: "Cast", exact: true })
                .click();
            await overlay.waitFor();
        };
        const takenBack = async (n) => {
            await page.waitForSelector(`${subtlety}[data-zone="hand"]`);
            assert.equal(await cancels(page), n, `cancelled ${n}`);
            assert.equal(await overlay.count(), 0, "the exile is not asked");
        };
        await evoke();
        // The card whose cost it is stands beside the choice.
        assert.equal(
            await overlay.locator(".overlay-aside .card-name").textContent(),
            "Subtlety",
        );
        await page.keyboard.press("Escape");
        await takenBack(1);
        await evoke();
        await page.mouse.click(800, 450, { button: "right" });
        await takenBack(2);
        await evoke();
        await overlay
            .getByRole("button", { name: "Cancel", exact: true })
            .click();
        await takenBack(3);
        // Put aside to see the table, the cast is taken back from there: Escape, or the
        // spell dragged from the stack to the hand.
        const peek = async () => {
            await evoke();
            await overlay
                .getByRole("button", { name: "View Battlefield" })
                .click();
            await overlay.waitFor({ state: "detached" });
            await page.clock.runFor(1000);
        };
        await peek();
        await page.keyboard.press("Escape");
        await takenBack(4);
        await peek();
        const onStack = `${subtlety}[data-zone="stack"]`;
        const from = center(await page.locator(onStack).boundingBox());
        await drag(page, onStack, { by: { x: 700 - from.x, y: 870 - from.y } });
        await takenBack(5);
        assert.deepEqual(await chosen(page), Array(5).fill(evoked));
    },
    evokeFixture,
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

// The last priority in the scry fixture where both players have graveyards and exiled cards,
// and the human holds a hand wide enough to fan out as far as it goes.
const exiledFrom = (v, side) =>
    v.objects.filter(
        (o) =>
            o.owner === side &&
            o.zone === "exile" &&
            !v.battlefield.includes(o.exiledBy),
    );
const pilesAt = scry.views.findLastIndex(
    (v) =>
        v.status.decision?.kind === "priority" &&
        v.objects.filter((o) => o.zone === "hand" && o.owner === "you")
            .length >= 6 &&
        ["you", "opponent"].every(
            (side) =>
                v.players[side].graveyard.length > 0 &&
                exiledFrom(v, side).length > 0,
        ),
);
assert.ok(pilesAt >= 0, "the scry fixture has a view with every pile");
const pilesView = scry.views[pilesAt];
const pile = (side, zone) => `[data-pile="${zone}"][data-side="${side}"]`;
const pileRun = {
    fixture: "choice-scry",
    at: pilesAt + 1,
    ready: pile("you", "graveyard"),
};
const overlaps = (a, b) =>
    a.x < b.x + b.width &&
    b.x < a.x + a.width &&
    a.y < b.y + b.height &&
    b.y < a.y + a.height;

for (const viewport of [
    { width: 1600, height: 900 },
    { width: 1280, height: 800 },
]) {
    await check(
        `your graveyard and exile lie clear of the hand at ${viewport.width}`,
        async (page) => {
            await page.clock.runFor(2000);
            const hand = await page.locator(".card-el-hand").all();
            assert.ok(hand.length >= 6, "the hand fans out as far as it goes");
            for (const zone of ["graveyard", "exile"]) {
                const box = await page.locator(pile("you", zone)).boundingBox();
                for (const card of hand) {
                    const name = await card.getAttribute("data-card");
                    assert.ok(
                        !overlaps(box, await card.boundingBox()),
                        `${name} covers your ${zone}`,
                    );
                }
            }
        },
        { ...pileRun, viewport },
    );
}

/** The names of the cards a pile in the piles' view holds. */
function pileNames(side, zone) {
    const slots =
        zone === "graveyard"
            ? pilesView.players[side].graveyard
            : exiledFrom(pilesView, side).map((o) => o.slot);
    return slots.map(
        (slot) => pilesView.objects.find((o) => o.slot === slot).name,
    );
}

await check(
    "a pile opens a viewer listing each of its cards, which closes",
    async (page) => {
        await page.clock.runFor(2000);
        // A pile is not pointed at card by card: hovering it shows no card whole.
        await page.hover(pile("you", "graveyard"));
        await page.clock.runFor(500);
        assert.equal(await page.$(".inspect"), null, "a pile card is hovered");
        const close = [
            (p) => p.keyboard.press("Escape"),
            (p) => p.getByRole("button", { name: "Close" }).click(),
            (p) => p.mouse.click(20, 450),
        ];
        const piles = [
            ["you", "graveyard"],
            ["opponent", "graveyard"],
            ["opponent", "exile"],
        ];
        for (const [i, [side, zone]] of piles.entries()) {
            const names = pileNames(side, zone);
            assert.ok(names.length > 0, `the ${side}'s ${zone} has cards`);
            await page.click(pile(side, zone));
            const viewer = `.zone-viewer${pile(side, zone)}`;
            await page.waitForSelector(viewer);
            const listed = await page.$$eval(
                `${viewer} [data-viewer-slot]`,
                (cards) => cards.map((c) => c.dataset.card),
            );
            assert.deepEqual(
                [...listed].sort(),
                [...names].sort(),
                `the ${side}'s ${zone} lists each of its cards`,
            );
            // Each card shows whole on hover.
            await page.hover(`${viewer} [data-card="${listed[0]}"]`);
            await page
                .getByLabel(`${listed[0]}, inspected`)
                .waitFor({ state: "visible" });
            await close[i](page);
            await page.waitForSelector(viewer, { state: "detached" });
        }
        assert.deepEqual(await chosen(page), [], "browsing answers nothing");
    },
    pileRun,
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

// Menus and prompts say what each option does (by-isv.8). `menu-choices` stops at a priority
// decision with Jace, the Mind Sculptor's three loyalty abilities and Sink into Stupor, a
// spell whose back is a land: every card's menu draws its costs as symbols, never braces;
// pointing at a loyalty ability lights its paragraph of Jace's text; and the land is played
// by its land face's name.
const menus = await lastDecision("menu-choices");
const menuView = menus.views.at(-1);
const nameOfSlot = (view, slot) =>
    view.objects.find((o) => o.slot === slot)?.name;
const menuSlots = [
    ...Map.groupBy(
        menus.decision.candidates.filter(
            (c) => offeredSlot(c.option) !== undefined,
        ),
        (c) => offeredSlot(c.option),
    ),
]
    .filter(([, cs]) => cs.length > 1)
    .map(([slot]) => slot);
const sculptor = menuView.objects.find(
    (o) => o.name === "Jace, the Mind Sculptor" && o.zone === "battlefield",
).slot;
const stupor = menuView.objects.find(
    (o) => o.name === "Sink into Stupor" && o.zone === "hand",
).slot;
assert.ok(
    menuSlots.includes(sculptor) && menuSlots.includes(stupor),
    "Jace and Sink into Stupor each offer more than one thing",
);
await check(
    "menus draw costs as symbols and light the ability pointed at",
    async (page) => {
        await page.clock.runFor(3000);
        for (const slot of menuSlots) {
            await page.click(`[data-slot="${slot}"]`);
            const text = await page.locator(".action-menu").textContent();
            assert.ok(!text.includes("{"), `no braces in ${text}`);
            await page.keyboard.press("Escape");
        }
        await page.click(`[data-slot="${sculptor}"]`);
        const items = page.locator(".action-menu-item");
        assert.deepEqual(
            await items
                .locator(".loyalty-cost")
                .evaluateAll((es) =>
                    es.map((e) => e.getAttribute("aria-label")),
                ),
            ["+2", "0", "\u22121"],
            "each loyalty ability by its symbol",
        );
        await items.nth(2).hover();
        assert.match(
            await page.locator(".inspect .rules-lit").textContent(),
            /^\u22121: Return target creature/,
            "pointing at −1 lights its paragraph",
        );
        await page.keyboard.press("Escape");
        await page.click(`[data-slot="${stupor}"]`);
        assert.deepEqual(
            await items.allTextContents(),
            ["Play Soporific Springs", "Cast Sink into Stupor"],
            "the land is played by its land face",
        );
        await items.nth(0).hover();
        assert.equal(
            await page.locator(".inspect").getAttribute("aria-label"),
            "Soporific Springs, inspected",
            "pointing at it shows that face",
        );
    },
    { fixture: "menu-choices", at: menus.views.length, ready: ".turn-action" },
);

// A target prompt with two cards of one name (by-isv.8): `namesake-target` stops on a
// target for Teferi, Hero of Dominaria with a Snapcaster Mage on each side. The prompt lists
// neither; each glows on the table, on its side, and clicking one there picks it.
const namesake = await lastDecision("namesake-target");
const namesakeView = namesake.views.at(-1);
const mages = [...namesake.bySlot.keys()].filter(
    (slot) => nameOfSlot(namesakeView, slot) === "Snapcaster Mage",
);
const theirMage = mages.find(
    (slot) =>
        namesakeView.objects.find((o) => o.slot === slot).controller ===
        "opponent",
);
assert.equal(mages.length, 2, "the fixture offers two Snapcaster Mages");
await check(
    "a target prompt leaves same-named cards to the table",
    async (page) => {
        await page.clock.runFor(3000);
        const buttons = await page
            .locator(".prompt-choices .button")
            .allTextContents();
        assert.ok(
            !buttons.some((b) => b.includes("Snapcaster Mage")),
            `the prompt lists no Snapcaster Mage: ${buttons}`,
        );
        for (const slot of mages) {
            assert.ok(
                await page.$(
                    `[data-slot="${slot}"][data-zone="battlefield"] .card-target`,
                ),
                "each glows on the table",
            );
        }
        await page.click(`[data-slot="${theirMage}"]`);
        assert.deepEqual(await settled(page, 1), [
            namesake.bySlot.get(theirMage),
        ]);
    },
    {
        fixture: "namesake-target",
        at: namesake.views.length,
        ready: ".prompt",
    },
);

// Paying life for a land (by-isv.8): `pay-life` stops as Sink into Stupor is played as
// Soporific Springs with three untapped lands, Jace, the Mind Sculptor in hand the one card
// the fourth would make castable. The prompt shows the land face over the player's life orb
// and defaults to paying; with Jace renamed Counterspell, nothing wants the mana this turn
// and it defaults to entering tapped.
const payLife = await lastDecision("pay-life");
const answerOf = (kind) =>
    payLife.decision.candidates.findIndex((c) => c.option.kind === kind);
await writeFile(
    join(fixtures, "pay-life-unwanted.json"),
    (await readFile(join(fixtures, "pay-life.json"), "utf8")).replaceAll(
        "Jace, the Mind Sculptor",
        "Counterspell",
    ),
);
for (const [fixture, primary, kind] of [
    ["pay-life", "Pay 3 Life", "yes"],
    ["pay-life-unwanted", "Enter Tapped", "no"],
]) {
    await check(
        `paying life for a land defaults by whether its mana is wanted (${fixture})`,
        async (page) => {
            await page.clock.runFor(2000);
            assert.equal(
                await page.locator(".overlay-aside .card-name").textContent(),
                "Soporific Springs",
                "the land face",
            );
            assert.equal(
                await page
                    .locator(".choice-life .life-orb-total")
                    .textContent(),
                String(payLife.views.at(-1).players.you.life),
            );
            const confirm = page.locator(".overlay-confirm");
            assert.equal(await confirm.textContent(), primary);
            await confirm.click();
            assert.deepEqual(await settled(page, 1), [answerOf(kind)]);
        },
        { fixture, at: payLife.views.length, ready: ".choice" },
    );
}

// The trigger-order prompt (still asked under exact stops) names each trigger's source in full,
// even the cube's longest names: `choice-trigger-order` stops on one between two permanents,
// renamed here to the two longest names in the cube list, and no text on the page is cut.
const cube = await readFile(
    new URL("../../data/arena-powered-cube-2025-10-30.csv", import.meta.url),
    "utf8",
);
const longest = cube
    .split("\n")
    .slice(1)
    .map((line) => line.replace(/^[^,]*,/, "").replace(/^"(.*)"$/, "$1"))
    .sort((a, b) => b.length - a.length)
    .slice(0, 2);
const order = await lastDecision("choice-trigger-order");
const sources = order.decision.candidates.map(
    (c) =>
        order.views.at(-1).objects.find((o) => o.slot === c.option.source).name,
);
assert.equal(sources.length, 2, "the fixture orders two triggers");
let renamed = await readFile(
    join(fixtures, "choice-trigger-order.json"),
    "utf8",
);
for (const [i, name] of sources.entries()) {
    renamed = renamed.replaceAll(JSON.stringify(name).slice(1, -1), longest[i]);
}
await writeFile(join(fixtures, "choice-trigger-order-long.json"), renamed);
await check(
    "the trigger-order prompt shows the longest names in full",
    async (page) => {
        await page.clock.runFor(3000);
        assert.deepEqual(await cutText(page), [], "no text is cut");
        const choices = page.locator(".prompt-choices .button");
        assert.deepEqual(
            await choices.allTextContents(),
            longest,
            "each trigger is named by its source",
        );
        await choices.nth(1).click();
        assert.deepEqual(await settled(page, 1), [1]);
    },
    {
        fixture: "choice-trigger-order-long",
        at: order.views.length,
        ready: ".prompt",
    },
);

// The stack's arrows (by-l1w.8): a spell aimed at a card beside the stack, an ability aimed at
// its source's neighbor, and one aimed at a player. Each arrow ties the elements the top of
// the stack names, starts and ends on their edges apart from every other arrow's ends, is long
// enough to read, and runs mostly clear of the other cards on the table.
for (const fixture of ["arrow-spell", "arrow-ability", "arrow-player"]) {
    const views = await load(fixture);
    const view = views.at(-1);
    const top = view.stack.length - 1;
    const item = view.stack[top];
    const from = item.kind === "spell" ? `card-${item.slot}` : `stack-${top}`;
    const expected = [
        ...(item.kind === "ability" && item.source !== null
            ? [[`card-${item.source}`, from]]
            : []),
        ...item.targets.map((t) => [
            from,
            t.kind === "player" ? `${t.player}-life-orb` : `card-${t.slot}`,
        ]),
    ];
    await check(
        `${fixture}: each arrow's source and target read distinctly`,
        async (page) => {
            await page.clock.runFor(3000);
            const drawn = await page.evaluate(() => {
                const box = (e) => {
                    const r = e.getBoundingClientRect();
                    return { x: r.x, y: r.y, w: r.width, h: r.height };
                };
                const cards = [
                    ...document.querySelectorAll(".card-el .card-slot"),
                ];
                return [...document.querySelectorAll(".arrow")].map((g) => {
                    const path = g.querySelector(".arrow-line path");
                    const ctm = path.getScreenCTM();
                    const length = path.getTotalLength();
                    const points = Array.from({ length: 61 }, (_, i) => {
                        const p = path.getPointAtLength((length * i) / 60);
                        const q = new DOMPoint(p.x, p.y).matrixTransform(ctm);
                        return [q.x, q.y];
                    });
                    const ends = [g.dataset.from, g.dataset.to].map((id) =>
                        document.getElementById(id),
                    );
                    const others = cards
                        .filter(
                            (c) => !ends.some((e) => c === e || c.contains(e)),
                        )
                        .map(box);
                    return {
                        ends: [g.dataset.from, g.dataset.to],
                        boxes: ends.map(box),
                        points,
                        others,
                    };
                });
            });
            assert.deepEqual(
                drawn.map((a) => a.ends),
                expected,
                "an arrow from the source and one to each target",
            );
            const within = ([x, y], b, m) =>
                x > b.x - m &&
                x < b.x + b.w + m &&
                y > b.y - m &&
                y < b.y + b.h + m;
            const gap = (a, b) => Math.hypot(a[0] - b[0], a[1] - b[1]);
            for (const a of drawn) {
                const [start, end] = [a.points[0], a.points.at(-1)];
                const name = a.ends.join(" -> ");
                // On each end's edge: within its box grown a little but not deep inside it.
                for (const [p, b] of [
                    [start, a.boxes[0]],
                    [end, a.boxes[1]],
                ]) {
                    assert.ok(within(p, b, 3), `${name} meets its element`);
                    assert.ok(!within(p, b, -12), `${name} meets its edge`);
                }
                const length = a.points
                    .slice(1)
                    .reduce((sum, p, i) => sum + gap(p, a.points[i]), 0);
                assert.ok(length >= 100, `${name} is ${length} px long`);
                const over = a.points.filter((p) =>
                    a.others.some((b) => within(p, b, 0)),
                ).length;
                assert.ok(
                    over <= 6,
                    `${name} runs over other cards at ${over} of 61 points`,
                );
            }
            const ends = drawn.flatMap((a) => [a.points[0], a.points.at(-1)]);
            for (const [i, p] of ends.entries()) {
                for (const q of ends.slice(i + 1)) {
                    assert.ok(gap(p, q) >= 24, "arrows end apart");
                }
            }
        },
        { fixture, at: views.length, ready: ".arrow" },
    );
}

// `marks` and `marks-damage` end on boards where a permanent bearing one mark and no other
// stands beside a twin of the same name bearing none: +1/+1 counters, summoning sickness and
// story or time counters in one, damage in the other.
{
    const counted = (kinds) => (o) =>
        o.counters.some((c) => kinds.includes(c.kind));
    const creature = (o) => o.types.includes("creature");
    const MARKS = [
        (o) => creature(o) && counted(["plusOne"])(o),
        (o) => creature(o) && o.damage > 0,
        (o) => creature(o) && o.summoningSick,
        counted(["story", "time"]),
    ];
    const [plusOne, damaged, sick, storyOrTime] = MARKS;
    /** A permanent bearing `mark` alone in fixture `views`' last view, and a bare twin. */
    const pair = (views, mark) => {
        const field = views
            .at(-1)
            .objects.filter((o) => o.zone === "battlefield" && o.name !== null);
        const bare = (o) =>
            !MARKS.some((m) => m(o)) &&
            o.counters.every((c) => c.kind === "loyalty");
        for (const o of field) {
            if (!mark(o) || MARKS.some((m) => m !== mark && m(o))) continue;
            const twin = field.find(
                (t) =>
                    t.slot !== o.slot &&
                    t.name === o.name &&
                    t.token === o.token &&
                    bare(t),
            );
            if (twin) return [o, twin];
        }
        assert.fail("the fixture has a marked permanent beside a bare twin");
    };
    const el = (page, o) => page.locator(`.card-el[data-slot="${o.slot}"]`);
    const marks = await load("marks");
    const hurt = await load("marks-damage");
    await check(
        "marks: counters and sickness are drawn",
        async (page) => {
            await page.clock.runFor(2000);
            for (const [kinds, mark] of [
                [["plusOne"], plusOne],
                [["story", "time"], storyOrTime],
            ]) {
                const [o, twin] = pair(marks, mark);
                const shown = await el(page, o)
                    .locator(".card-counters .mark-badge")
                    .evaluateAll((bs) =>
                        bs.map((b) => [b.className, b.textContent]),
                    );
                for (const c of o.counters.filter((c) =>
                    kinds.includes(c.kind),
                )) {
                    assert.ok(
                        shown.some(
                            ([cls, n]) =>
                                cls.includes(`mark-${c.kind}`) &&
                                n === String(c.count),
                        ),
                        `${o.name} wears ${c.count} ${c.kind}: ${shown}`,
                    );
                }
                assert.equal(
                    await el(page, twin).locator(".card-counters").count(),
                    0,
                    `${twin.name}'s twin wears no badge`,
                );
            }
            const [ill, ready] = pair(marks, sick);
            assert.equal(await el(page, ill).locator(".card-sick").count(), 1);
            assert.equal(
                await el(page, ready).locator(".card-sick").count(),
                0,
            );
        },
        { fixture: "marks", at: marks.length, ready: ".card-counters" },
    );
    await check(
        "marks: damage lowers the toughness drawn, in red",
        async (page) => {
            await page.clock.runFor(2000);
            const [o, whole] = pair(hurt, damaged);
            const toughness = el(page, o).locator(".card-pt b").last();
            assert.equal(
                await toughness.textContent(),
                String(o.pt.toughness - o.damage),
            );
            assert.match(await toughness.getAttribute("class"), /stat-down/);
            assert.equal(
                await el(page, whole).locator(".card-pt b.stat-down").count(),
                0,
            );
        },
        { fixture: "marks-damage", at: hurt.length, ready: ".card-pt" },
    );
    await check(
        "marks: a countered creature's inspector shows its current P/T",
        async (page) => {
            await page.clock.runFor(2000);
            const [o] = pair(marks, plusOne);
            await page.hover(`.card-el[data-slot="${o.slot}"]`);
            const pt = page.locator(".inspect .inspect-pt");
            await pt.waitFor();
            const text = await pt.textContent();
            assert.ok(
                text.endsWith(`${o.pt.power}/${o.pt.toughness}`),
                `${o.name}'s inspector reads ${text}`,
            );
            await page
                .locator(".inspect .inspect-marks .mark-plusOne")
                .waitFor();
        },
        { fixture: "marks", at: marks.length, ready: ".card-counters" },
    );
}

// A card the opponent's Elite Spellbinder exiled from the human's hand, which they may cast from
// exile (`exile-cast`): its pile glows, its viewer offers it, and picking it there casts it.
// The human also knows a card in the opponent's hand, one their Sink into Stupor returned there.
const exileViews = await load("exile-cast");
const exileView = exileViews.at(-1);
const exileCast = exileView.status.decision.candidates.findIndex(
    (c) =>
        c.option.kind === "cast" &&
        exileView.objects.find((o) => o.slot === c.option.slot)?.zone ===
            "exile",
);
assert.ok(exileCast >= 0, "exile-cast offers a cast from exile");
const exiledSlot = exileView.status.decision.candidates[exileCast].option.slot;
const yourExile = '[data-pile="exile"][data-side="you"]';
const exileRun = {
    fixture: "exile-cast",
    at: exileViews.length,
    ready: yourExile,
};

await check(
    "a card castable from exile is cast from its pile",
    async (page) => {
        // The pile glows once the decision is shown.
        await until(page, `${yourExile}.pile-button-playable`);
        await page.click(yourExile);
        await page
            .getByRole("dialog", { name: "Your exile" })
            .locator(`[data-viewer-slot="${exiledSlot}"]`)
            .click();
        // With nothing to choose, the pick casts it at once.
        assert.deepEqual(
            await chosen(page),
            [exileCast],
            `chose "${exileView.status.decision.candidates[exileCast].label}"`,
        );
    },
    exileRun,
);

const theirHand = exileView.objects.filter(
    (o) => o.zone === "hand" && o.owner === "opponent",
);
const knownCard = theirHand.find((o) => o.name !== null);
assert.ok(knownCard, "exile-cast: the human knows a card in their hand");
assert.ok(
    theirHand.some((o) => o.name === null),
    "exile-cast: the opponent holds a card the human does not know",
);

await check(
    "a card you know in the opponent's hand lies face up",
    async (page) => {
        await page.clock.runFor(2000);
        for (const o of theirHand) {
            const back = await page.$(`${slotOf(o.slot)} .card-back`);
            assert.equal(
                !back,
                o.name !== null,
                `slot ${o.slot} (${o.name ?? "unknown"}) is ` +
                    (back ? "a back" : "face up"),
            );
        }
        // It peeks in from the top edge; hovering what shows of it shows it whole.
        const box = await page.locator(slotOf(knownCard.slot)).boundingBox();
        await page.mouse.move(box.x + box.width / 2, box.y + box.height - 12);
        await page.getByLabel(`${knownCard.name}, inspected`).waitFor();
    },
    exileRun,
);

// Tokens, piles of identical permanents and stolen permanents: the `tokens` fixture holds two
// identical Map tokens of yours, whose ability the engine offers through one of them only
// (architecture §7.2), a token changed by an Aura, and a permanent its owner doesn't control.
const tokenViews = await load("tokens");
const tokenView = tokenViews.at(-1);
const tokenField = tokenView.objects.filter((o) => o.zone === "battlefield");
const mapAct = tokenView.status.decision.candidates.findIndex(
    (c) =>
        c.option.kind === "activate" &&
        tokenField.find((o) => o.slot === c.option.slot)?.token,
);
assert.ok(mapAct >= 0, "tokens: a token's ability is offered");
const offeredToken = tokenField.find(
    (o) => o.slot === tokenView.status.decision.candidates[mapAct].option.slot,
);
const tokenPile = [
    offeredToken,
    ...tokenField.filter((o) => twins(offeredToken, o)),
];
assert.ok(tokenPile.length > 1, "tokens: the offered token has a twin");
const stolen = tokenField.filter((o) => o.owner !== o.controller);
assert.ok(stolen.length > 0, "tokens: a permanent is stolen");
assert.ok(
    tokenField.some(
        (o) =>
            o.token &&
            o.name !== offeredToken.name &&
            tokenField.some((a) => a.attachedTo === o.slot),
    ),
    "tokens: a token wears an Aura",
);
const tokenRun = {
    fixture: "tokens",
    at: tokenViews.length,
    ready: slotOf(offeredToken.slot),
};

await check(
    "tokens wear their frame and stolen permanents their edge",
    async (page) => {
        await page.clock.runFor(2000);
        for (const o of tokenField) {
            if (o.types.includes("land")) continue;
            const marks = await page
                .locator(`${slotOf(o.slot)} .card`)
                .evaluate((e) => [
                    e.classList.contains("card-token"),
                    !!e.querySelector(".card-stolen-edge"),
                ]);
            assert.deepEqual(
                marks,
                [o.token, o.owner !== o.controller],
                `${o.name} in slot ${o.slot}: [token, stolen]`,
            );
        }
        // The inspector names the owner.
        const thief = stolen[0];
        await page.hover(slotOf(thief.slot));
        await page
            .getByLabel(`${thief.name}, inspected`)
            .getByText(`Owner: ${thief.owner === "you" ? "You" : "Opponent"}`)
            .waitFor();
    },
    tokenRun,
);

await check(
    "clicking a pile's top card acts on the one the engine offers",
    async (page) => {
        await page.clock.runFor(2000);
        // One card of the pile wears its count, and it is the one on top.
        const counted = [];
        for (const o of tokenPile) {
            const badge = await page.$(`${slotOf(o.slot)} .card-count`);
            if (badge) {
                assert.equal(
                    await badge.textContent(),
                    `×${tokenPile.length}`,
                    "the count",
                );
                counted.push(o.slot);
            }
        }
        assert.equal(counted.length, 1, "one card of the pile is counted");
        const top = counted[0];
        assert.notEqual(top, offeredToken.slot, "the offered token is beneath");
        const box = await page.locator(slotOf(top)).boundingBox();
        const at = center(box);
        const hit = await page.evaluate(
            ({ x, y }) =>
                document.elementFromPoint(x, y)?.closest("[data-slot]")?.dataset
                    .slot,
            at,
        );
        assert.equal(Number(hit), top, "the counted card is on top");
        // The token's one use acts on the click.
        await page.mouse.click(at.x, at.y);
        assert.deepEqual(await settled(page, 1), [mapAct], "the ability");
    },
    tokenRun,
);

await check(
    "a pile splits on any difference, and on a target",
    async (page) => {
        // The board's layout of variants of the fixture's view, by the page's own module.
        const counts = await page.evaluate(
            async ({ view, map }) => {
                const { layout } = await import("/src/game/layout.ts");
                // Each card of the pile of `map`'s copies that wears a count, with the count.
                const counted = (v) =>
                    [...layout(v).values()]
                        .filter((p) => p.count !== undefined)
                        .filter((p) =>
                            v.objects.some(
                                (o) => o.slot === p.slot && o.name === map.name,
                            ),
                        )
                        .map((p) => p.count);
                const vary = (change) => {
                    const v = structuredClone(view);
                    const copy = { ...structuredClone(map), slot: 250 };
                    v.objects.push(copy);
                    v.battlefield.push(copy.slot);
                    change(
                        v,
                        v.objects.find((o) => o.slot === copy.slot),
                    );
                    return counted(v);
                };
                const aura = view.objects.find(
                    (o) => o.zone === "battlefield" && o.attachedTo !== null,
                ).slot;
                return {
                    fixture: counted(view),
                    third: vary(() => {}),
                    types: vary((_, o) => o.types.push("creature")),
                    owner: vary((_, o) => (o.owner = "opponent")),
                    tapped: vary((_, o) => (o.tapped = true)),
                    enchanted: vary((v, o) => {
                        v.objects.find((a) => a.slot === aura).attachedTo =
                            o.slot;
                    }),
                    targeted: vary((v, o) =>
                        v.stack.push({
                            kind: "ability",
                            source: null,
                            ability: 0,
                            granted: false,
                            name: "Test",
                            triggered: true,
                            nth: 0,
                            controller: "opponent",
                            targets: [{ kind: "object", slot: o.slot }],
                            x: 0,
                        }),
                    ),
                };
            },
            { view: tokenView, map: offeredToken },
        );
        assert.deepEqual(counts, {
            fixture: [2],
            third: [3],
            types: [2],
            owner: [2],
            tapped: [2],
            enchanted: [2],
            targeted: [2],
        });
    },
    tokenRun,
);

// Every object an open decision offers something for has an element a player can see and point
// at: the card on the table or in a choice's tray, a twin the engine offers it through, or the
// button of the pile it lies in. Every exiled card can be seen the same way, if not pointed at
// under a screen over the table. Checked at the last decision of every fixture, with motion
// reduced so that nothing is caught halfway through fading in.
for (const fixture of recorded) {
    const fixtureViews = await load(fixture);
    const last = fixtureViews.findLastIndex(
        (v) => v.status.kind === "deciding",
    );
    if (last < 0) continue;
    const view = fixtureViews[last];
    const byId = new Map(view.objects.map((o) => [o.slot, o]));
    const describe = (slot, why) =>
        `slot ${slot} (${byId.get(slot)?.name ?? "unknown"}, ${why})`;
    const wanted = [];
    for (const c of view.status.decision.candidates) {
        const slot = offeredSlot(c.option);
        if (slot === undefined) continue;
        const o = byId.get(slot);
        const twinSlots = o
            ? view.objects.filter((t) => twins(o, t)).map((t) => t.slot)
            : [];
        wanted.push({
            what: describe(slot, `${c.option.kind} "${c.label}"`),
            slots: [slot, ...twinSlots],
            point: true,
        });
    }
    for (const o of view.objects) {
        if (o.zone === "exile") {
            wanted.push({
                what: describe(o.slot, "exiled"),
                slots: [o.slot],
                point: false,
            });
        }
    }
    const owners = Object.fromEntries(
        view.objects.map((o) => [o.slot, o.owner]),
    );
    await check(
        `${fixture}: what the decision offers and what is exiled can be seen`,
        async (page) => {
            await page.clock.runFor(5000);
            const unseen = await page.evaluate(
                ({ wanted, owners }) => {
                    const seen = (el, point) => {
                        for (let e = el; e; e = e.parentElement) {
                            const style = getComputedStyle(e);
                            if (Number(style.opacity) < 0.5) return false;
                            if (style.visibility === "hidden") return false;
                        }
                        const r = el.getBoundingClientRect();
                        if (r.width < 1 || r.height < 1) return false;
                        // Some part of it lies on the screen, and, to be pointed at, is the
                        // topmost thing there.
                        for (let i = 1; i < 10; i++) {
                            for (let j = 1; j < 10; j++) {
                                const x = r.left + (r.width * i) / 10;
                                const y = r.top + (r.height * j) / 10;
                                if (x < 0 || y < 0) continue;
                                if (x >= innerWidth || y >= innerHeight)
                                    continue;
                                if (!point) return true;
                                const hit = document.elementFromPoint(x, y);
                                if (hit && el.contains(hit)) return true;
                            }
                        }
                        return false;
                    };
                    const shown = (slot, point) =>
                        [
                            ...document.querySelectorAll(
                                `[data-slot="${slot}"]`,
                            ),
                        ].some((el) => {
                            if (!el.classList.contains("card-el-pile")) {
                                return seen(el, point);
                            }
                            const pile = document.querySelector(
                                `[data-pile="${el.dataset.zone}"]` +
                                    `[data-side="${owners[slot]}"]`,
                            );
                            return !!pile && seen(pile, point);
                        });
                    return wanted
                        .filter((w) => !w.slots.some((s) => shown(s, w.point)))
                        .map((w) => w.what);
                },
                { wanted, owners },
            );
            assert.deepEqual(unseen, [], "every one can be seen");
        },
        // The board draws its first view, the whole fixture's, once the fixture has loaded.
        {
            fixture,
            at: last + 1,
            ready: ".card-el:is(.card-el-hand, .card-el-land, .card-el-field)",
            reducedMotion: "reduce",
        },
    );
}

await check(
    "the full-control toggle sends the session its new stops",
    async (page) => {
        const recorded = JSON.parse(
            await readFile(join(fixtures, `${FIXTURE}.json`), "utf8"),
        );
        const session = recorded.messages.find(
            (m) => m.kind === "response" && m.command === "start_game",
        ).value;
        await page.clock.runFor(2000);
        await page.click('[aria-label="Settings"]');
        const toggle = page.locator(".settings-toggle input");
        assert.equal(await toggle.isChecked(), false, "smart stops at first");
        await toggle.click();
        const sent = await page.evaluate(() =>
            window.berylFixture.calls
                .filter((c) => c.command === "set_stops")
                .map((c) => c.args.request),
        );
        assert.deepEqual(sent, [{ session, stops: "exact" }]);
    },
);

// `mana-floating` stops at the human's priority in their main phase with Mana Crypt's {C}
// unspent, then plays on past their pass, which ends the step and empties the pool.
const floatingViews = await load("mana-floating");
const floatingAt =
    floatingViews.findIndex(
        (v) =>
            v.status.kind === "deciding" &&
            Object.values(v.players.you.mana).some((n) => n > 0),
    ) + 1;

/** The words standing on the table, outside cards, stack items, open prompts and panels and
 * the game log (the record a player reads back, which grows with the game): what a player
 * must read past to see the board. Text hidden or transparent, such as a closed tip, does not
 * count. */
const tableWords = (page) =>
    page.evaluate(() => {
        const SKIP = [
            ".card-el",
            ".card-slot",
            ".card-back",
            ".stack-item",
            ".prompt",
            ".action-menu",
            ".choice",
            ".zone-viewer",
            ".inspect",
            ".settings",
            ".announcements",
            ".game-log",
        ].join(", ");
        const shown = (el) => {
            if (getComputedStyle(el).visibility !== "visible") return false;
            for (let a = el; a; a = a.parentElement) {
                if (parseFloat(getComputedStyle(a).opacity) === 0) return false;
            }
            return el.getClientRects().length > 0;
        };
        const words = [];
        const table = document.querySelector(".table");
        const walk = document.createTreeWalker(table, NodeFilter.SHOW_TEXT);
        for (let n = walk.nextNode(); n; n = walk.nextNode()) {
            const el = n.parentElement;
            if (!el || el.closest(SKIP) || !shown(el)) continue;
            words.push(...(n.textContent.match(/[\p{L}\p{N}]+/gu) ?? []));
        }
        return words;
    });

// The table's standing words in states that show its piles, mana and the stack: the badges,
// the life totals and the turn button. Moving the piles' names and counts into their tips took
// these from 15, 13, 11 and 8; no change adds standing words to the table.
const WORD_BUDGETS = [
    { fixture: "choice-scry", at: pilesAt + 1, budget: 7 },
    { fixture: FIXTURE, at: 119, budget: 7 },
    { fixture: "stack-x", budget: 7 },
    { fixture: "mana-floating", at: floatingAt, budget: 8 },
];
for (const { fixture, at, budget } of WORD_BUDGETS) {
    await check(
        `${fixture}: the table stands no more words than before`,
        async (page) => {
            await page.clock.runFor(3000);
            const words = await tableWords(page);
            assert.ok(
                words.length <= budget,
                `${words.length} words, over ${budget}: ${words.join(" ")}`,
            );
        },
        {
            fixture,
            at: at ?? (await load(fixture)).length,
            ready: ".card-el:is(.card-el-hand, .card-el-land, .card-el-field)",
            reducedMotion: "reduce",
        },
    );
}

await check(
    "hovering the tracker, a pile or a library says what the table shows",
    async (page) => {
        await page.clock.runFor(3000);
        const view = pilesView;
        assert.equal(
            view.turn.step,
            "precombatMain",
            "the fixture is at Main 1",
        );
        /** The one tip showing with the pointer on the first match of `selector`. */
        const tip = async (selector) => {
            await page.hover(selector);
            return page.locator(".tip").filter({ visible: true }).innerText();
        };
        assert.equal(
            await tip(".turn-controls .phase-current"),
            `Turn ${view.turn.number} · Main 1`,
        );
        assert.equal(
            await tip("[data-pile=graveyard][data-side=you]"),
            `Graveyard · ${view.players.you.graveyard.length}`,
        );
        assert.equal(
            await tip('[aria-label^="Library"]'),
            `Library · ${view.players.opponent.library.length}`,
        );
    },
    {
        fixture: "choice-scry",
        at: pilesAt + 1,
        ready: ".card-el:is(.card-el-hand, .card-el-land, .card-el-field)",
        reducedMotion: "reduce",
    },
);

await check(
    "unspent mana shows by the life orb and pulses on a pass, unasked",
    async (page) => {
        await page.clock.runFor(3000);
        await page.clock.pauseAt(Date.now() + 60_000);
        const decision = floatingViews[floatingAt - 1].status.decision;
        assert.equal(decision.passTo.kind, "step", "the pass ends the step");
        const pool = page.locator(".mana-pool");
        assert.equal(
            await pool.getAttribute("aria-label"),
            "Unspent mana: {C}",
        );
        await page.getByRole("button", { name: /^Next/ }).click();
        assert.equal(await page.locator(".mana-pool-pulse").count(), 1);
        assert.equal(
            await page.getByRole("dialog").count(),
            0,
            "passing asks nothing",
        );
        assert.deepEqual(await chosen(page), [
            decision.candidates.findIndex((c) => c.option.kind === "pass"),
        ]);
        // The step ends and the pool empties; it goes once the pulse is over.
        await page.clock.runFor(5000);
        assert.equal(await pool.count(), 0, "the emptied pool goes");
    },
    {
        fixture: "mana-floating",
        at: floatingAt,
        ready: ".card-el:is(.card-el-hand, .card-el-land, .card-el-field)",
    },
);

await check(
    "each stack item shows whose it is and any X announced",
    async (page) => {
        await page.clock.runFor(3000);
        const view = (await load("stack-x")).at(-1);
        const abilities = page.locator(".stack-ability");
        let ability = 0;
        for (const item of view.stack) {
            const [el, controller] =
                item.kind === "spell"
                    ? [
                          page.locator(
                              `.card-el-stack[data-slot="${item.slot}"]`,
                          ),
                          view.objects.find((o) => o.slot === item.slot)
                              .controller,
                      ]
                    : [abilities.nth(ability++), item.controller];
            assert.equal(
                await el
                    .locator("[data-controller]")
                    .getAttribute("data-controller"),
                controller,
            );
            const x = el.locator(".stack-x");
            if (item.x > 0) {
                assert.equal(await x.innerText(), `X=${item.x}`);
            } else {
                assert.equal(await x.count(), 0, "no X, no badge");
            }
        }
        const controllers = view.stack.map((i) =>
            i.kind === "spell"
                ? view.objects.find((o) => o.slot === i.slot).controller
                : i.controller,
        );
        assert.deepEqual(
            new Set(controllers),
            new Set(["you", "opponent"]),
            "the fixture's stack holds both players' items",
        );
        assert.ok(
            view.stack.some((i) => i.x > 0),
            "the fixture's stack holds an X",
        );
    },
    {
        fixture: "stack-x",
        at: (await load("stack-x")).length,
        ready: ".card-el-stack",
        reducedMotion: "reduce",
    },
);

// A late, crowded table: at least eight nonland permanents a side and a spell on the stack.
const crowdedViews = await load("crowded");
const crowdedAt = crowdedViews.findLastIndex(
    (v) =>
        v.status.decision?.kind === "priority" &&
        v.stack.some((item) => item.kind === "spell"),
);
assert.ok(crowdedAt >= 0, "the crowded fixture has a spell on the stack");
for (const side of ["you", "opponent"]) {
    assert.ok(
        nonlands(crowdedViews[crowdedAt], side).length >= 8,
        `the crowded fixture has eight nonland permanents for ${side}`,
    );
}
const crowdedRun = (at, viewport) => ({
    fixture: "crowded",
    at: at + 1,
    ready: '[data-zone="battlefield"]',
    reducedMotion: "reduce",
    ...(viewport && { viewport }),
});

/** The permanents of each row a player sees whole, by side: each but the lands, what lies
 * tucked under another, and the copies beneath a pile's top card, with the box of the card and
 * of each of its name, power and toughness (or loyalty), count and counter badges. */
const rows = (page) =>
    page.evaluate(() => {
        const cards = [
            ...document.querySelectorAll(
                '.card-el[data-zone="battlefield"]:not(.card-el-land)',
            ),
        ].filter((e) => !e.querySelector(".card-under"));
        const z = (e) => Number(e.style.zIndex) || 0;
        const box = (e) => {
            const r = e.getBoundingClientRect();
            return { x: r.x, y: r.y, width: r.width, height: r.height };
        };
        const beneath = (e) => {
            const r = e.getBoundingClientRect();
            return cards.some((f) => {
                const s = f.getBoundingClientRect();
                return (
                    f !== e &&
                    f.dataset.card === e.dataset.card &&
                    z(f) > z(e) &&
                    Math.abs(s.x - r.x) < r.width * 0.2 &&
                    Math.abs(s.y - r.y) < r.height * 0.2
                );
            });
        };
        const out = { you: [], opponent: [] };
        for (const e of cards.filter((e) => !beneath(e))) {
            const side = e.classList.contains("card-el-you")
                ? "you"
                : "opponent";
            out[side].push({
                slot: e.dataset.slot,
                name: e.dataset.card,
                box: box(e),
                marks: [
                    ...e.querySelectorAll(
                        ".card-name, .card-pt, .card-count, .mark-badge",
                    ),
                ].map((m) => ({ mark: m.className, box: box(m) })),
            });
        }
        for (const side of Object.values(out))
            side.sort((a, b) => a.box.x - b.box.x);
        return out;
    });

/** Of the points across the middle of `box`, inset 2 px, those where the topmost element is
 * not inside the card in `slot`. */
const coveredPoints = (page, slot, box) =>
    page.evaluate(
        ({ slot, box }) => {
            const y = box.y + box.height / 2;
            const xs = [
                box.x + 2,
                box.x + box.width / 2,
                box.x + box.width - 2,
            ];
            return xs.filter((x) => {
                const top = document.elementFromPoint(x, y);
                return top?.closest("[data-slot]")?.dataset.slot !== slot;
            });
        },
        { slot, box },
    );

/** Whether two boxes overlap by more than a pixel each way. */
const overlap = (a, b) =>
    Math.min(a.x + a.width, b.x + b.width) - Math.max(a.x, b.x) > 1 &&
    Math.min(a.y + a.height, b.y + b.height) - Math.max(a.y, b.y) > 1;

await check(
    "no permanent's name or marks are covered on a crowded table",
    async (page) => {
        await page.clock.runFor(2000);
        const covered = [];
        for (const cards of Object.values(await rows(page))) {
            for (const { slot, name, marks } of cards) {
                for (const { mark, box } of marks) {
                    const at = await coveredPoints(page, slot, box);
                    if (at.length) covered.push(`${name} ${mark} at x ${at}`);
                }
            }
        }
        assert.deepEqual(covered, [], "covered names and marks");
        await assertClearOfLands(page);
    },
    crowdedRun(crowdedAt),
);

await check(
    "sweeping a crowded table moves no card but inspects each, clear of the stack and lives",
    async (page) => {
        await page.clock.runFor(2000);
        const still = await drawn(page);
        const forbidden = await page.evaluate(() =>
            [
                ...document.querySelectorAll(
                    ".card-el-stack, .stack-ability, [data-life]",
                ),
            ].map((e) => {
                const r = e.getBoundingClientRect();
                return {
                    what: e.dataset.card ?? e.dataset.life ?? "an ability",
                    x: r.x,
                    y: r.y,
                    width: r.width,
                    height: r.height,
                };
            }),
        );
        assert.ok(
            forbidden.filter((f) => f.what === "you" || f.what === "opponent")
                .length === 2 && forbidden.length >= 3,
            "the stack and both lives are drawn",
        );
        const { you, opponent } = await rows(page);
        const hand = await page.$$eval(".card-el-hand", (cards) =>
            cards.map((e) => {
                const r = e.getBoundingClientRect();
                return {
                    slot: e.dataset.slot,
                    name: e.dataset.card,
                    box: { x: r.x, y: r.y, width: r.width, height: r.height },
                };
            }),
        );
        for (const { slot, name, box } of [...opponent, ...you, ...hand]) {
            // Where the card shows: its frame's middle, or a hand card's top.
            const at = {
                x: box.x + box.width / 2,
                y:
                    box.y +
                    (hand.some((h) => h.slot === slot) ? 30 : box.height / 2),
            };
            await page.mouse.move(at.x, at.y);
            await page.clock.runFor(400);
            const now = await drawn(page);
            for (const [other, { box: b }] of Object.entries(now)) {
                if (other === slot || !still[other]) continue;
                const moved = Math.max(
                    ...b.map((v, i) => Math.abs(v - still[other].box[i])),
                );
                assert.ok(
                    moved <= 1,
                    `hovering ${name} moved slot ${other} by ${moved}`,
                );
            }
            const inspect = page.locator(
                `.inspect[aria-label="${name}, inspected"]`,
            );
            assert.equal(
                await inspect.count(),
                1,
                `the pointer at ${name}'s middle inspects it`,
            );
            const shown = await inspect.boundingBox();
            for (const f of forbidden) {
                assert.ok(
                    !overlap(shown, f),
                    `${name}'s inspector covers ${f.what}`,
                );
            }
            // A raised hand card stays under the life orbs, which are drawn over it (though
            // yours lets the pointer through to it: hit-test it as if it didn't).
            const life = await page.evaluate(() =>
                [...document.querySelectorAll("[data-life]")].every((e) => {
                    const r = e.getBoundingClientRect();
                    e.style.pointerEvents = "auto";
                    const top = document.elementFromPoint(
                        r.x + r.width / 2,
                        r.y + r.height / 2,
                    );
                    e.style.pointerEvents = "";
                    return e.contains(top);
                }),
            );
            assert.ok(life, `hovering ${name} hides a life total`);
        }
    },
    crowdedRun(crowdedAt),
);

/** Writes the fixture `as`: fixture `name` up to its view at index `at`, that view changed by
 * `edit`, so that a check can see one state of the table with and without something. */
async function derive(name, as, at, edit) {
    const fixture = JSON.parse(
        await readFile(join(fixtures, `${name}.json`), "utf8"),
    );
    const messages = [];
    let seen = -1;
    for (const m of fixture.messages) {
        if (m.kind === "event" && ++seen > at) break;
        if (m.kind === "event" && seen === at) {
            m.payload.view = edit(m.payload.view);
        }
        messages.push(m);
    }
    await writeFile(
        join(fixtures, `${as}.json`),
        JSON.stringify({ ...fixture, messages }),
    );
}

// The crowded table with a spell on the stack, and the same table with the stack empty, as it
// stands before the spell is cast and after it resolves. The spell names nothing here: a pile
// the stack names splits (by-isv.2), which changes the table.
const topSpell = crowdedViews[crowdedAt].stack.findLast(
    (item) => item.kind === "spell",
);
await derive("crowded", "crowded-stacked", crowdedAt, (v) => ({
    ...v,
    stack: [{ ...topSpell, targets: [] }],
}));
await derive("crowded", "crowded-unstacked", crowdedAt, (v) => ({
    ...v,
    stack: [],
}));
await check(
    "the stack appearing or resolving moves no permanent on a crowded table",
    async (page) => {
        const places = async (fixture) => {
            await page.goto(
                new URL(`/?fixture=${fixture}&at=${crowdedAt + 1}#/game`, base)
                    .href,
                { waitUntil: "load" },
            );
            await page.waitForSelector('[data-zone="battlefield"]');
            await page.clock.runFor(2000);
            return page.$$eval('.card-el[data-zone="battlefield"]', (cards) =>
                Object.fromEntries(
                    cards.map((e) => {
                        const r = e.getBoundingClientRect();
                        return [e.dataset.slot, [r.x, r.y, r.width, r.height]];
                    }),
                ),
            );
        };
        const stacked = await places("crowded-stacked");
        assert.ok(
            await page.$(".card-el-stack"),
            "the spell is drawn on the stack",
        );
        const unstacked = await places("crowded-unstacked");
        assert.equal(
            await page.$(".card-el-stack"),
            null,
            "the stack is empty",
        );
        for (const [slot, box] of Object.entries(stacked)) {
            const moved = Math.max(
                ...box.map((v, i) => Math.abs(v - unstacked[slot][i])),
            );
            assert.ok(moved <= 4, `the stack moves slot ${slot} by ${moved}`);
        }
    },
    crowdedRun(crowdedAt),
);

// A coin flip is announced for a moment, below the opponent's life orb.
await derive("crowded", "crowded-flip", crowdedAt, (v) => ({
    ...v,
    events: [...v.events, { kind: "coinFlip", player: "opponent", won: false }],
}));
await check(
    "a coin flip's banner clears the life orbs and goes after a moment",
    async (page) => {
        const banner = page.locator(".step-banner");
        await banner.waitFor();
        assert.match(await banner.innerText(), /flip/i, "the flip's banner");
        const shown = await banner.boundingBox();
        for (const side of ["you", "opponent"]) {
            const life = await page
                .locator(`[data-life="${side}"]`)
                .boundingBox();
            assert.ok(!overlap(shown, life), `the banner covers ${side}r life`);
        }
        await page.clock.runFor(4000);
        await banner.waitFor({ state: "detached" });
    },
    { ...crowdedRun(crowdedAt), fixture: "crowded-flip" },
);

// Cards over the table are small to fit; each shows whole on hover, wherever it is.
for (const [title, run, selector] of [
    ["the opening hand", { at: 2, ready: ".opening-card" }, ".opening-card"],
    [
        "a scry",
        {
            fixture: "choice-scry",
            at: scry.views.length,
            ready: ".choice-card .shown-card",
        },
        ".choice-card .shown-card",
    ],
]) {
    await check(
        `hovering a card in ${title} inspects it`,
        async (page) => {
            await page.clock.runFor(2000);
            const cards = page.locator(selector);
            const target = cards.last();
            const box = await target.boundingBox();
            // The card's visible right edge: cards in a tray may overlap.
            await page.mouse.move(
                box.x + box.width - 12,
                box.y + box.height / 2,
            );
            await page.clock.runFor(300);
            const name = await target.evaluate(
                (e) => e.closest("[data-card]")?.dataset.card,
            );
            await page
                .getByLabel(`${name}, inspected`)
                .waitFor({ state: "visible" });
            await page.mouse.move(1, 1);
            await page.clock.runFor(300);
            assert.equal(
                await page.$(".inspect"),
                null,
                "the inspector goes with the pointer",
            );
        },
        run,
    );
}

/** The inspector's box, once it shows the card `name`. */
async function inspectorOf(page, name) {
    const inspect = page.locator(`.inspect[aria-label="${name}, inspected"]`);
    await inspect.waitFor({ state: "visible" });
    return inspect.boundingBox();
}

/** A point of the element `selector` picks (the `nth`) where it is on top, so the pointer
 * reaches it: its middle if nothing covers that, else the first such point on a 10 px grid. */
const visiblePoint = (page, selector, nth) =>
    page.evaluate(
        ([selector, nth]) => {
            const e = document.querySelectorAll(selector)[nth];
            const r = e.getBoundingClientRect();
            const on = (x, y) => e.contains(document.elementFromPoint(x, y));
            const mid = { x: r.x + r.width / 2, y: r.y + r.height / 2 };
            if (on(mid.x, mid.y)) return mid;
            for (let y = r.top + 5; y < r.bottom; y += 10) {
                for (let x = r.left + 5; x < r.right; x += 10) {
                    if (on(x, y)) return { x, y };
                }
            }
            return null;
        },
        [selector, nth],
    );

// A prompt's cards show whole on hover, beside its instruction, options and cards, never over
// them, on an opaque inspector (by-isv.17): a look at the top four, a pay-or-counter and X.
for (const [fixture, at] of [
    ["choice-look", (await load("choice-look")).length],
    ["choice-pay", pay.views.length],
    ["cast-kicker", kickViews.length],
]) {
    await check(
        `hovering a card in ${fixture}'s prompt keeps its inspector off the prompt`,
        async (page) => {
            await page.clock.runFor(2000);
            const prompt = await page.$$eval(
                ".overlay-main :is(.overlay-title, .overlay-instruction, button, .shown-card)",
                (els) =>
                    els.map((e) => {
                        const r = e.getBoundingClientRect();
                        return {
                            what:
                                e.closest("[data-card]")?.dataset.card ??
                                e.textContent.trim().slice(0, 30),
                            x: r.x,
                            y: r.y,
                            width: r.width,
                            height: r.height,
                        };
                    }),
            );
            assert.ok(
                prompt.some((p) => p.what) && prompt.length >= 3,
                "the prompt is drawn",
            );
            const cards = ".overlay .shown-card";
            const count = await page.locator(cards).count();
            assert.ok(count >= 1, "the prompt shows a card");
            for (let i = 0; i < count; i++) {
                const point = await visiblePoint(page, cards, i);
                if (!point) continue;
                await page.mouse.move(point.x, point.y);
                await page.clock.runFor(300);
                const inspect = page.locator(".inspect");
                await inspect.waitFor({ state: "visible" });
                const name = await inspect.getAttribute("aria-label");
                const shown = await inspect.boundingBox();
                for (const p of prompt) {
                    assert.ok(
                        !overlap(shown, p),
                        `${name}'s inspector covers ${p.what}`,
                    );
                }
                const fill = await inspect.evaluate(
                    (e) => getComputedStyle(e).backgroundColor,
                );
                assert.match(
                    fill,
                    /^rgb\(/,
                    `the inspector is opaque, not ${fill}`,
                );
            }
        },
        { fixture, at, ready: ".overlay .shown-card" },
    );
}

// In a target step, a card shows whole clear of every target it might cover (by-isv.17):
// Fractured Identity's in greedy-turn-9, whose targets fill the table's middle, and Swift
// Reconfiguration's in attached-across and in crew, whose targets lie on both sides.
for (const fixture of ["greedy-turn-9", "attached-across", "crew"]) {
    const views = await load(fixture);
    const at = views.findIndex((v) => v.status.decision?.kind === "target");
    await check(
        `hovering a card in ${fixture}'s target step covers no target`,
        async (page) => {
            await page.clock.runFor(2000);
            const targets = await page.$$eval(
                '[data-zone="battlefield"]:has(.card-target)',
                (els) =>
                    els.map((e) => {
                        const r = e.getBoundingClientRect();
                        return {
                            what: e.dataset.card,
                            x: r.x,
                            y: r.y,
                            width: r.width,
                            height: r.height,
                        };
                    }),
            );
            assert.ok(targets.length > 0, "the targets glow");
            const cards = '.card-el-hand, .card-el[data-zone="battlefield"]';
            const count = await page.locator(cards).count();
            for (let i = 0; i < count; i++) {
                const name = await page
                    .locator(cards)
                    .nth(i)
                    .getAttribute("data-card");
                const point = name && (await visiblePoint(page, cards, i));
                if (!point) continue;
                await page.mouse.move(point.x, point.y);
                await page.clock.runFor(400);
                const shown = await inspectorOf(page, name);
                for (const t of targets) {
                    assert.ok(
                        !overlap(shown, t),
                        `${name}'s inspector covers the target ${t.what}`,
                    );
                }
            }
        },
        {
            fixture,
            at: at + 1,
            ready: '[data-zone="battlefield"]',
            reducedMotion: "reduce",
        },
    );
}

// Up to one target is one targeting step (`target-up-to-one`: Teferi, Time Raveler's −3, "up to
// one target artifact, creature, or enchantment"): its targets glow at the count, and clicking
// one announces one target and then it; a single button chooses none.
const upTo = await load("target-up-to-one");
const countAt = upTo.findLastIndex(
    (v) => v.status.decision?.kind === "targetCount",
);
const counted = upTo[countAt].status.decision;
const targeted = upTo.slice(countAt + 1).find((v) => v.status.decision)
    .status.decision;
assert.equal(targeted.kind, "target", "the fixture announces one target");
const countOf = (n) =>
    counted.candidates.findIndex(
        (c) => c.option.kind === "number" && c.option.n === n,
    );
const aimed = targeted.candidates.find(
    (c) => c.label === "Staff of the Storyteller",
).option.slot;
const upToRun = {
    fixture: "target-up-to-one",
    at: countAt + 1,
    ready: ".prompt",
};
await check(
    "up to one target is picked on the board with its count",
    async (page) => {
        await page.clock.runFor(2000);
        assert.ok(
            await page.$(`${slotOf(aimed)} .card-target`),
            "a target glows",
        );
        const prompt = page.getByRole("dialog");
        assert.match(await prompt.innerText(), /Choose a target/);
        assert.deepEqual(
            await prompt.locator(".prompt-choices button").allInnerTexts(),
            ["No Target"],
            "no count is asked",
        );
        await page.click(slotOf(aimed));
        assert.deepEqual(await settled(page, 2), [
            countOf(1),
            targeted.candidates.findIndex(
                (c) => c.option.kind === "object" && c.option.slot === aimed,
            ),
        ]);
    },
    upToRun,
);
await check(
    "up to one target chooses none from its one button",
    async (page) => {
        await page.getByRole("button", { name: "No Target" }).click();
        assert.deepEqual(await settled(page, 1), [countOf(0)]);
    },
    upToRun,
);

// A click never aims a spell for the player (`target-own`: Oust, whose one legal target is
// the human's own Jace, Vryn's Prodigy): the click starts the cast, the target is asked on the
// board, and Escape takes the cast back.
const own = await load("target-own");
const [ownBefore, ownAfter] = own.slice(-2);
const oust = ownBefore.objects.find((o) => o.name === "Oust").slot;
const oustCast = ownBefore.status.decision.candidates.findIndex(
    (c) => c.option.kind === "cast" && c.option.slot === oust,
);
const ownTarget = ownAfter.status.decision;
assert.equal(ownTarget.kind, "target", "the fixture asks Oust's target");
assert.deepEqual(
    ownTarget.candidates.map((c) => c.label),
    ["Jace, Vryn's Prodigy"],
    "the human's own creature is the one target",
);
await check(
    "a click on a spell with one target, the player's own, asks it",
    async (page) => {
        await page.clock.runFor(2000);
        await page.click(slotOf(oust));
        const targeting = page.getByRole("dialog", {
            name: "Choose a target for Oust",
        });
        await targeting.waitFor();
        assert.ok(
            await page.$(
                `${slotOf(ownTarget.candidates[0].option.slot)} .card-target`,
            ),
            "the target glows",
        );
        await page.clock.runFor(2000);
        assert.deepEqual(await chosen(page), [oustCast], "only the cast sent");
        await page.keyboard.press("Escape");
        await page.waitForSelector(`${slotOf(oust)}[data-zone="hand"]`);
        assert.equal(await cancels(page), 1, "cancelled");
        assert.equal(await targeting.count(), 0, "the target is not asked");
        assert.deepEqual(await chosen(page), [oustCast]);
    },
    {
        fixture: "target-own",
        at: own.length - 1,
        ready: `${slotOf(oust)}[data-zone="hand"]`,
    },
);

// An attack on a player who controls a planeswalker (`attack-past-walker`: Snapcaster Mage
// against The Wandering Emperor) attacks the player unasked: the engine's question of what it
// attacks (CR 508.1b) is answered from the declaration.
const pastWalker = await load("attack-past-walker");
const firstAim = pastWalker.findIndex(
    (v) => v.status.decision?.kind === "attackTarget",
);
const attackAt = pastWalker
    .slice(0, firstAim)
    .findLastIndex((v) => v.status.decision?.kind === "attacker");
const attacking = pastWalker[attackAt].status.decision;
const attacker = attacking.candidates.findIndex(
    (c) => c.option.kind === "attack",
);
const aiming = pastWalker[firstAim].status.decision;
await check(
    "an attack past a planeswalker asks nothing of what it attacks",
    async (page) => {
        await page.clock.runFor(2000);
        await page.click(slotOf(attacking.candidates[attacker].option.slot));
        await page.getByRole("button", { name: "Attack", exact: true }).click();
        const [first, second] = await settled(page, 2);
        assert.equal(first, attacker);
        assert.deepEqual(
            aiming.candidates[second].option,
            { kind: "defender", planeswalker: null },
            "the player is attacked",
        );
    },
    {
        fixture: "attack-past-walker",
        at: attackAt + 1,
        ready: slotOf(attacking.candidates[attacker].option.slot),
    },
);

// Explore (`choice-explore`, a Map's) shows the card it revealed, each answer named by where
// the card goes.
const explore = await lastDecision("choice-explore");
const revealed = explore.views.at(-1).players.you.library.at(-1);
await check(
    "explore shows the revealed card and answers by where it goes",
    async (page) => {
        assert.ok(await page.$(tray(revealed)), "the revealed card is shown");
        await page
            .getByRole("button", { name: "Graveyard", exact: true })
            .click();
        assert.deepEqual(await settled(page, 1), [
            explore.decision.candidates.findIndex(
                (c) => c.option.kind === "yes",
            ),
        ]);
    },
    { fixture: "choice-explore", at: explore.views.length, ready: ".choice" },
);

// A loot (`choice-loot`, Jace, Vryn's Prodigy's) shows its draw before it asks for the
// discard: the card drawn is in hand, and the log says so, as the discard is asked.
const loot = await lastDecision("choice-loot");
const lootAt = loot.views.findLastIndex(
    (v) => v.status.decision?.kind === "priority",
);
const lootHand = (v) =>
    v.objects
        .filter((o) => o.zone === "hand" && o.owner === "you")
        .map((o) => o.slot);
const lootDrawn = lootHand(loot.views.at(-1)).filter(
    (slot) => !lootHand(loot.views[lootAt]).includes(slot),
);
assert.equal(lootDrawn.length, 1, "the loot draws one card");
const jaceVp = named(loot.views[lootAt], "Jace, Vryn's Prodigy")[0];
await check(
    "a loot's draw shows before the discard is asked",
    async (page) => {
        await page.clock.runFor(2000);
        await page.click(slotOf(jaceVp));
        await page.waitForSelector(".choice");
        assert.ok(
            await page.$(tray(lootDrawn[0])),
            "the drawn card is offered",
        );
        const entries = await page.locator(".game-log-entry").allTextContents();
        assert.match(entries.at(-1), /You drew a card/);
        assert.deepEqual(await chosen(page), [
            loot.views[lootAt].status.decision.candidates.findIndex(
                (c) => c.option.kind === "activate" && c.option.slot === jaceVp,
            ),
        ]);
    },
    { fixture: "choice-loot", at: lootAt + 1, ready: slotOf(jaceVp) },
);

// Time Spiral's untap (`choice-untap`) is picked on the board: the tapped lands it may untap
// glow, wherever they sit and whoever controls them, and no others; each pick turns gold, its
// count on the confirm button; picks of twins the engine offers one of go as that one.
const untapping = await lastDecision("choice-untap");
const untapView = untapping.views.at(-1);
const landsOf = (tapped) =>
    untapView.objects
        .filter(
            (o) =>
                o.zone === "battlefield" &&
                o.types.includes("land") &&
                o.tapped === tapped,
        )
        .map((o) => o.slot);
const tappedLands = landsOf(true);
const untappedLands = landsOf(false);
assert.ok(untappedLands.length > 0, "the fixture has untapped lands");
/** The slots of the permanents glowing `highlight`, in order. */
const glowing = (page, highlight) =>
    page.$$eval(`[data-zone="battlefield"] .card-${highlight}`, (els) =>
        [
            ...new Set(
                els.map((e) => Number(e.closest("[data-slot]").dataset.slot)),
            ),
        ].sort((a, b) => a - b),
    );
/** The first of `slots` whose card is on top where it sits, not covered by another in its
 * pile, so a click lands on it. */
const uncovered = (page, slots) =>
    page.evaluate(
        (slots) =>
            slots.find((slot) => {
                const el = document.querySelector(`[data-slot="${slot}"]`);
                const r = el.getBoundingClientRect();
                const top = document.elementFromPoint(
                    r.left + r.width / 2,
                    r.top + r.height / 2,
                );
                return top?.closest("[data-slot]") === el;
            }),
        slots,
    );
await check(
    "an untap is picked on the board with only eligible permanents lit",
    async (page) => {
        await page.clock.runFor(2000);
        assert.equal(await page.$(".choice"), null, "no overlay opens");
        assert.deepEqual(
            await glowing(page, "target"),
            [...tappedLands].sort((a, b) => a - b),
        );
        // An untapped land can't be picked.
        await page.click(slotOf(await uncovered(page, untappedLands)));
        await page.getByRole("button", { name: "Untap None" }).waitFor();
        const land = await uncovered(page, tappedLands);
        await page.click(slotOf(land));
        assert.deepEqual(await glowing(page, "source"), [land]);
        await page
            .getByRole("button", {
                name: `Untap 1/${untapping.decision.ask.max}`,
            })
            .click();
        const [first] = await settled(page, 1);
        const o = untapping.decision.candidates[first].option;
        const named = (slot) =>
            untapView.objects.find((x) => x.slot === slot).name;
        assert.equal(named(o.slot), named(land), "the land picked");
    },
    {
        fixture: "choice-untap",
        at: untapping.views.length,
        ready: ".prompt",
    },
);

// Trinket Mage's search (`choice-search`) finds up to one of two artifacts: a second click
// lights the second card in place of the first, never both, and Done sends the one lit.
const searching = await lastDecision("choice-search");
assert.equal(searching.decision.ask.max, 1, "the search finds up to one");
const [crypt, converter] = [...searching.bySlot.keys()];
await check(
    "an up-to-one search lights one card and sends it",
    async (page) => {
        await page.clock.runFor(2000);
        await page.click(tray(crypt));
        await page.click(tray(converter));
        const lit = await page.$$eval(
            '.choice-zone [aria-pressed="true"]',
            (cards) => cards.map((c) => Number(c.dataset.slot)),
        );
        assert.deepEqual(lit, [converter], "one card is lit, the last");
        await page.getByRole("button", { name: "Done" }).click();
        assert.deepEqual(
            await settled(page, 2),
            [searching.bySlot.get(converter)],
            "Done sends one pick",
        );
    },
    {
        fixture: "choice-search",
        at: searching.views.length,
        ready: tray(crypt),
    },
);

// The cleanup discard (`choice-cleanup`: eight cards, one over) counts its picks on its button.
const cleanup = await lastDecision("choice-cleanup");
const [extra] = [...cleanup.bySlot.keys()];
await check(
    "the cleanup discard counts its picks",
    async (page) => {
        await page.getByRole("button", { name: "Discard 0/1" }).waitFor();
        await page.click(tray(extra));
        await page.getByRole("button", { name: "Discard 1/1" }).click();
        assert.deepEqual(await settled(page, 1), [cleanup.bySlot.get(extra)]);
    },
    { fixture: "choice-cleanup", at: cleanup.views.length, ready: tray(extra) },
);

// Every choice overlay the fixtures hold keeps its words in their places: the instruction
// between the header's two rules, the title above the first, each label in its button. A
// picker of cards always says what to do; a question's buttons may say it alone.
for (const fixture of [
    "choice-scry",
    "choice-jace",
    "choice-pay",
    "choice-explore",
    "choice-loot",
    "choice-search",
    "choice-thirst",
    "choice-cleanup",
    "pay-life",
]) {
    const { views } = await lastDecision(fixture);
    await check(
        `${fixture}'s choice text fits its box`,
        async (page) => {
            await page.evaluate(() => document.fonts.ready.then(() => {}));
            await page.clock.runFor(2000);
            const misfits = await page.evaluate(() => {
                const header = document
                    .querySelector(".choice .overlay-header")
                    .getBoundingClientRect();
                const out = [];
                const picker = document.querySelector(".choice-pick") !== null;
                const within = (selector, top, bottom) => {
                    const e = document.querySelector(`.choice ${selector}`);
                    const text = e?.textContent.trim() ?? "";
                    if (!text) {
                        if (picker) out.push(`${selector} is empty`);
                        return;
                    }
                    const r = e.getBoundingClientRect();
                    if (
                        r.top < header.top + top - 1 ||
                        r.bottom > header.top + bottom + 1
                    )
                        out.push(
                            `${selector} "${text}" spans ${r.top - header.top}..${r.bottom - header.top}`,
                        );
                };
                // The header's rules lie 68 and 131 pixels down (components.css).
                within(".overlay-title", -8, 68);
                within(".overlay-instruction", 70, 131);
                for (const b of document.querySelectorAll(".choice button")) {
                    if (
                        b.scrollWidth > b.clientWidth + 1 ||
                        b.scrollHeight > b.clientHeight + 1
                    )
                        out.push(`button "${b.textContent}" overflows`);
                }
                return out;
            });
            assert.deepEqual(misfits, [], "choice text out of its box");
        },
        { fixture, at: views.length, ready: ".choice" },
    );
}

// Adeline's trigger (`attack-token`) asks what its Human attacks with the Human on the table,
// attacking the player; the turn button keeps that, and clicking The Wandering Emperor turns
// it on her.
const tokenAim = await lastDecision("attack-token");
const human = named(tokenAim.views.at(-1), "Human Token")[0];
const emperor = named(tokenAim.views.at(-1), "The Wandering Emperor")[0];
assert.ok(
    human !== undefined,
    "the Human is on the table as it is asked about",
);
const defending = (walker) =>
    tokenAim.decision.candidates.findIndex(
        (c) => c.option.kind === "defender" && c.option.planeswalker === walker,
    );
const adelineRun = {
    fixture: "attack-token",
    at: tokenAim.views.length,
    ready: slotOf(human),
};
await check(
    "Adeline's Human is in view as what it attacks is asked",
    async (page) => {
        await page.clock.runFor(2000);
        assert.ok(
            await page.$(`${slotOf(emperor)} .card-target`),
            "the planeswalker glows",
        );
        await page.click(slotOf(emperor));
        assert.deepEqual(await settled(page, 1), [defending(emperor)]);
    },
    adelineRun,
);
await check(
    "Adeline's Human attacks the player from the turn button",
    async (page) => {
        await page.clock.runFor(2000);
        await page.getByRole("button", { name: "Attack", exact: true }).click();
        assert.deepEqual(await settled(page, 1), [defending(null)]);
    },
    adelineRun,
);

// Crewing (`crew`: Swift Reconfiguration's crew 5) is picked on the board, a meter of pips
// showing the power tapped against the cost, and what the creature pointed at would add.
const crewViews = await load("crew");
const crewAt = crewViews.findIndex((v) => v.status.decision?.kind === "crew");
const crewing = crewViews[crewAt].status.decision;
const crewer = (name) =>
    crewing.candidates.find((c) => c.label === name).option.slot;
const crewMeter = (page) => page.getByRole("meter", { name: "Crew" });
await check(
    "crewing shows the power picked against the crew cost without text",
    async (page) => {
        await page.clock.runFor(2000);
        const meter = crewMeter(page);
        assert.deepEqual(
            [
                await meter.getAttribute("aria-valuenow"),
                await meter.getAttribute("aria-valuemax"),
                (await meter.innerText()).trim(),
            ],
            ["0", "5", ""],
        );
        await page.hover(slotOf(crewer("Elite Spellbinder")));
        assert.equal(await page.locator(".crew-pip-next").count(), 3);
        const snapcaster = crewer("Snapcaster Mage");
        await page.click(slotOf(snapcaster));
        assert.deepEqual(await settled(page, 1), [
            crewing.candidates.findIndex(
                (c) =>
                    c.option.kind === "object" && c.option.slot === snapcaster,
            ),
        ]);
    },
    { fixture: "crew", at: crewAt + 1, ready: ".crew-meter" },
);
await check(
    "the crew meter fills with the power tapped",
    async (page) => {
        await page.clock.runFor(2000);
        assert.equal(await crewMeter(page).getAttribute("aria-valuenow"), "2");
        assert.equal(await page.locator(".crew-pip-on").count(), 2);
    },
    { fixture: "crew", at: crewAt + 2, ready: ".crew-meter" },
);

// A waterbend lands alone can pay (`choice-scry`: Aang's Iceberg's {3}, three Plains and a
// Snapcaster Mage) asks nothing: the activation goes straight on to the scry.
const scryViews = await load("choice-scry");
assert.ok(
    scryViews.every((v) => v.status.decision?.kind !== "waterbend"),
    "no waterbend is asked",
);
const bendAt = scryViews.findLastIndex(
    (v) => v.status.decision?.kind === "priority",
);
const bending = scryViews[bendAt].status.decision;
const bend = bending.candidates.findIndex(
    (c) => c.option.kind === "activate" && c.label.includes("Waterbend"),
);
const iceberg = bending.candidates[bend].option.slot;
await check(
    "a waterbend payable by lands alone asks nothing",
    async (page) => {
        await page.clock.runFor(2000);
        await page.click(slotOf(iceberg));
        assert.deepEqual(await settled(page, 1), [bend]);
        await page.waitForSelector(".choice");
        assert.equal(await page.$(".prompt"), null, "nothing is asked first");
    },
    { fixture: "choice-scry", at: bendAt + 1, ready: slotOf(iceberg) },
);

// A waterbend mana can't pay (`waterbend-tap`: one Plains for {3}) shows its cost in symbols
// and lights the artifact and creature that may tap for it.
const tapBend = await lastDecision("waterbend-tap");
await check(
    "a waterbend shows its cost in symbols with its tappers lit",
    async (page) => {
        await page.clock.runFor(2000);
        assert.deepEqual(
            await page
                .locator(".prompt-cost .mana-symbol")
                .evaluateAll((els) => els.map((e) => e.ariaLabel)),
            ["{3}"],
        );
        assert.deepEqual(
            await glowing(page, "target"),
            [...tapBend.bySlot.keys()].sort((a, b) => a - b),
        );
    },
    {
        fixture: "waterbend-tap",
        at: tapBend.views.length,
        ready: ".prompt-cost",
    },
);

// The opening: `opening-bottom` ends on the human's first card to put on the bottom after two
// mulligans and a keep. The picks fill two slots and go back on a click; nothing is sent until
// both are chosen and confirmed, and then the first goes at once.
const openingViews = await load("opening-bottom");
const bottoming = openingViews.at(-1);
assert.equal(bottoming.status.decision.kind, "putOnBottom");
assert.equal(bottoming.players.you.mulligans, 2, "two cards go on the bottom");
await check(
    "cards for the bottom toggle in their slots until confirmed",
    async (page) => {
        const hand = page.locator(".opening-hand .opening-card");
        const slotted = page.locator(".opening-slots .opening-card");
        const done = page.getByRole("button", { name: "Done" });
        assert.equal(await page.locator(".opening-slot").count(), 2);
        const first = await hand.first().getAttribute("data-slot");
        await hand.first().click();
        assert.equal(await slotted.count(), 1, "a pick fills a slot");
        assert.equal(await done.isDisabled(), true, "one slot is still empty");
        await slotted.first().click();
        assert.equal(await slotted.count(), 0, "a second click takes it back");
        assert.equal(await hand.count(), 7);
        await hand.nth(2).click();
        await hand.nth(4).click();
        assert.equal(await slotted.count(), 2);
        assert.equal(await done.isEnabled(), true);
        assert.deepEqual(await chosen(page), [], "nothing sent before Done");
        await done.click();
        const [sent] = await settled(page, 1);
        const option = bottoming.status.decision.candidates[sent].option;
        const picked = await slotted.first().getAttribute("data-slot");
        const name = (slot) =>
            bottoming.objects.find((o) => o.slot === Number(slot)).name;
        assert.notEqual(picked, first, "the card taken back stays in hand");
        assert.equal(name(option.slot), name(picked), "the first pick is sent");
    },
    {
        fixture: "opening-bottom",
        at: openingViews.length,
        ready: ".opening-slot",
    },
);

// The start screen: Enter starts the game, and the opponent picked is offered again after a
// reload.
const startCalls = (page) =>
    page.evaluate(() =>
        window.berylFixture.calls.filter((c) => c.command === "start_game"),
    );
await check(
    "Enter on the start screen starts a game",
    async (page) => {
        await page.keyboard.press("Enter");
        await page.waitForURL(/#\/game$/);
        assert.equal((await startCalls(page)).length, 1);
    },
    { at: 0, hash: "#/", ready: ".start-agent input:checked" },
);
await check(
    "Enter with a game in progress returns to it",
    async (page) => {
        await page.getByRole("button", { name: "Return to Game" }).waitFor();
        await page.keyboard.press("Enter");
        await page.waitForURL(/#\/game$/);
        assert.equal((await startCalls(page)).length, 0, "no new game");
    },
    { at: 50, hash: "#/", ready: ".start-agent input:checked" },
);
await check(
    "the start screen remembers the last opponent across a reload",
    async (page) => {
        const checked = () =>
            page.locator(".start-agent input:checked").getAttribute("value");
        const other = await page
            .locator(".start-agent input:not(:checked)")
            .first()
            .getAttribute("value");
        await page.locator(`.start-agent input[value="${other}"]`).check();
        await page.getByRole("button", { name: "Start", exact: true }).click();
        await page.waitForURL(/#\/game$/);
        const [call] = await startCalls(page);
        assert.equal(call.args.request.agent, other);
        await page.evaluate(() => (window.location.hash = "#/"));
        await page.reload();
        await page.waitForSelector(".start-agent input:checked");
        assert.equal(await checked(), other, "the last opponent is picked");
    },
    { at: 0, hash: "#/", ready: ".start-agent input:checked" },
);

// The end of a game (`game-over`, under full control): Beza, the Bounding Spring attacks a
// human at 3 life with nothing to block, and its damage ends the game once they pass. The page
// starts at that last pass.
const overFixture = JSON.parse(
    await readFile(join(fixtures, "game-over.json"), "utf8"),
);
const overViews = await load("game-over");
const overEvents = overFixture.messages.filter((m) => m.kind === "event");
const lastPass = overViews.findLastIndex((v) => v.status.kind === "deciding");
const overAt = { fixture: "game-over", at: lastPass + 1, ready: "[data-life]" };
const lifeShown = async (page) =>
    Number(
        (
            await page
                .locator('[data-life="you"] .life-orb-total')
                .textContent()
        ).replace("\u2212", "-"),
    );
await check(
    "the result waits for the killing blow, says why, and can be put aside",
    async (page) => {
        await page.clock.runFor(2000);
        await page.keyboard.press(" ");
        const result = page.getByRole("dialog", { name: "Defeat" });
        // When the damage shows, and when the view of the end does (its log line).
        let [lost, ended] = [undefined, undefined];
        let t = 0;
        for (; t < 60 && !(await result.count()); t++) {
            lost ??= (await lifeShown(page)) < 0 ? t : undefined;
            const log = await page.locator(".game-log-entry").allTextContents();
            ended ??= log.some((e) => e.includes("lost the game"))
                ? t
                : undefined;
            await page.clock.runFor(100);
        }
        assert.ok(await result.count(), "the result shows");
        assert.ok(lost !== undefined && lost < t, "the damage shows first");
        // The end's events (the loss) hold it as long as an action (playback.ts' HOLD).
        assert.ok(
            ended !== undefined && (t - ended) * 100 >= 800,
            `the end plays first: ${ended}, then ${t}`,
        );
        assert.equal(
            await result.locator(".game-over-reason").textContent(),
            "Life",
        );
        const entries = await page.locator(".game-log-entry").allTextContents();
        assert.ok(
            entries.some((e) => /You lost the game at -1 life/.test(e)),
            "the log says how",
        );
        await result.getByRole("button", { name: "View battlefield" }).click();
        assert.equal(await result.count(), 0, "the battlefield shows");
        await page.getByRole("button", { name: "Back to Defeat" }).click();
        await result.getByRole("button", { name: "New game" }).waitFor();
    },
    overAt,
);

await check(
    "the settings concede the game",
    async (page) => {
        await page.clock.runFor(2000);
        await page.getByRole("button", { name: "Settings" }).click();
        await page
            .getByRole("button", { name: "Concede", exact: true })
            .click();
        const calls = await page.evaluate(() =>
            window.berylFixture.calls.filter((c) => c.command === "concede"),
        );
        assert.deepEqual(
            calls.map((c) => c.args.request.session),
            [overEvents[0].payload.session],
        );
        assert.equal(await page.locator(".settings").count(), 0, "closed");
    },
    overAt,
);

await check(
    "a new game with one in progress asks first, then concedes it",
    async (page) => {
        await page.evaluate(() => (window.location.hash = "#/"));
        const button = page.locator(".start-buttons button", {
            hasText: "New Game",
        });
        await button.click();
        const commands = () =>
            page.evaluate(() =>
                window.berylFixture.calls
                    .map((c) => c.command)
                    .filter((c) => c === "concede" || c === "start_game"),
            );
        assert.deepEqual(await commands(), [], "nothing yet");
        await page
            .getByRole("button", { name: "Concede & Start", exact: true })
            .click();
        for (let i = 0; i < 20 && (await commands()).length < 2; i++) {
            await page.clock.runFor(100);
        }
        assert.deepEqual(await commands(), ["concede", "start_game"]);
    },
    overAt,
);

// A reloaded page asks for the game in progress (`current_game`) and takes it up: here the
// fixture records no views, only that answer, the human's last pass in `game-over`.
await writeFile(
    join(fixtures, "game-over-resumed.json"),
    JSON.stringify({
        messages: [
            ...overFixture.messages.filter((m) => m.kind === "response"),
            {
                kind: "response",
                command: "current_game",
                value: overEvents[lastPass].payload,
            },
        ],
    }),
);
await check(
    "a reloaded page takes up the game in progress",
    async (page) => {
        assert.equal(await lifeShown(page), 3);
        assert.equal(await page.locator(".start-panel").count(), 0);
        await page.keyboard.press(" ");
        const chosen = await settled(page, 1);
        assert.deepEqual(chosen, [0], "the open decision is answered");
    },
    { fixture: "game-over-resumed", at: 0, ready: "[data-life]" },
);

await queue.drain();
await browser.close();
if (errors.length) {
    failed = true;
    console.error(`page errors:\n  ${errors.join("\n  ")}`);
}
process.exit(failed ? 1 : 0);
