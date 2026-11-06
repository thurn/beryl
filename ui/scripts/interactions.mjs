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
// measured on Playwright's clock, so the checks do not depend on the machine's speed. The
// `choice-*` fixtures end on decisions the choice overlays ask. Exits non-zero on the first
// failed check or any page error.
import assert from "node:assert/strict";
import { readFile, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { cutText, launch } from "./capture.mjs";

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

// A table with tapped and untapped lands and creatures, and a card Aang's Iceberg exiled.
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

// The trigger-order prompt (still asked under exact stops) names each trigger's source in full,
// even the cube's longest names: `choice-trigger-order` stops on one between two creatures,
// renamed here to the two longest names in the cube list, and no text on the page is cut.
const cube = await readFile(
    new URL("../../data/vintage-cube-2026-10-07.csv", import.meta.url),
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

await browser.close();
if (errors.length) {
    failed = true;
    console.error(`page errors:\n  ${errors.join("\n  ")}`);
}
process.exit(failed ? 1 : 0);
