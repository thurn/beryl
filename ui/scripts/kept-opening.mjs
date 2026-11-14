// The kept checks of the opening's cards (kept.mjs): those put on the bottom of the library,
// over the `opening-bottom` fixture, whose last view asks the player to put a card on the
// bottom; and the hand kept, over `opening-keep`, which plays on from the keep.
import assert from "node:assert/strict";
import { check, load } from "./kept-harness.mjs";

const views = await load("opening-bottom");
const asked = views.findLastIndex(
    (v) => v.status.decision?.prompt?.purpose === "put-on-bottom",
);

/** Clicks the card in `from` with the given slot and reads the glide that follows: where the
 * card's element starts and where it ends, and which other cards glide with it. The click and
 * the reading are one evaluation, so the glide cannot finish between them. */
const clickAndRead = (page, from, to, slot) =>
    page.evaluate(
        async ([from, to, slot]) => {
            const middle = (el) => {
                const r = el.getBoundingClientRect();
                return { x: r.left + r.width / 2, y: r.top + r.height / 2 };
            };
            const picked = document.querySelector(
                `${from} .opening-card[data-slot="${slot}"]`,
            );
            const before = { ...middle(picked), width: picked.offsetWidth };
            picked.click();
            // React renders a click's change in a microtask; the page's timers are paused.
            for (let i = 0; i < 5; i++) await Promise.resolve();
            const moved = document.querySelector(
                `${to} .opening-card[data-slot="${slot}"]`,
            );
            const glides = moved.getAnimations();
            const others = [...document.querySelectorAll(".opening-card")]
                .filter((el) => el !== moved && el.getAnimations().length)
                .map((el) => el.dataset.slot);
            const [glide] = glides;
            glide?.pause();
            if (glide) glide.currentTime = 0;
            const start = middle(moved);
            const startWidth = moved.getBoundingClientRect().width;
            glide?.cancel();
            const end = middle(moved);
            return {
                before,
                glides: glides.length,
                start,
                startWidth,
                end,
                endWidth: moved.offsetWidth,
                others,
            };
        },
        [from, to, slot],
    );

// Why no audit: the generic motion audit times the stage's verbs, and the opening is not on
// the stage. The player asked that a card picked for the bottom go down into its slot rather
// than vanish from the hand and appear there (by-uo1r); this pins where the glide starts.
for (const phone of [false, true]) {
    await check(
        `a card picked for the bottom glides down to its slot and back on ${phone ? "phone" : "desktop"}`,
        async (page) => {
            await page
                .getByRole("dialog", { name: "Put on the bottom" })
                .waitFor();
            const hand = ".opening-hand";
            const slots = ".opening-slots";
            const cards = page.locator(`${hand} .opening-card`);
            const slot = await cards.nth(2).getAttribute("data-slot");
            const down = await clickAndRead(page, hand, slots, slot);
            assert.equal(down.glides, 1, "the picked card glides");
            // It starts where it lay in the hand (within the fan's lean), at the hand's size.
            assert.ok(
                Math.hypot(
                    down.start.x - down.before.x,
                    down.start.y - down.before.y,
                ) < 4,
                `starts in the hand: ${JSON.stringify(down)}`,
            );
            assert.ok(
                Math.abs(down.startWidth / down.before.width - 1) < 0.1,
                `starts at the hand's size: ${JSON.stringify(down)}`,
            );
            assert.ok(down.end.y > down.start.y + 20, "and goes down");
            // The cards after it in the hand close the gap it left.
            assert.ok(down.others.length > 0, "the hand closes up");

            const up = await clickAndRead(page, slots, hand, slot);
            assert.equal(up.glides, 1, "the card clicked again glides back");
            assert.ok(
                Math.hypot(up.start.x - up.before.x, up.start.y - up.before.y) <
                    4,
                `starts in its slot: ${JSON.stringify(up)}`,
            );
            assert.ok(up.end.y < up.start.y - 20, "and goes up");
        },
        {
            fixture: "opening-bottom",
            at: asked + 1,
            ready: ".opening-card",
            phone,
            viewport: phone ? { width: 393, height: 759 } : undefined,
        },
    );
}

// The `opening-keep` fixture's views run from the player's keep-or-mulligan decision into the
// first turns; the page opens on that decision.
const keepViews = await load("opening-keep");
const keepAsked = keepViews.findIndex(
    (v) => v.status.decision?.kind === "keepOrMulligan",
);

/** The middle of each card of the player's hand, by slot: on the opening screen, or on the
 * stage. */
const middles = (page, selector) =>
    page.evaluate(
        (selector) =>
            Object.fromEntries(
                [...document.querySelectorAll(selector)].map((el) => {
                    const r = el.getBoundingClientRect();
                    return [
                        el.dataset.slot,
                        { x: r.left + r.width / 2, y: r.top + r.height / 2 },
                    ];
                }),
            ),
        selector,
    );

// Why no audit: the generic motion audit times the stage's verbs, and the hand coming back from
// under the opening is no verb of a script. The player asked that the hand they keep move down
// into place rather than appear there (by-qmbe), as Arena's drops from the opening screen; this
// pins that each card starts where the opening showed it and comes down to its place.
for (const phone of [false, true]) {
    await check(
        `the kept hand drops from the opening into place on ${phone ? "phone" : "desktop"}`,
        async (page) => {
            await page.getByRole("dialog", { name: "Opening hand" }).waitFor();
            const shown = await middles(page, ".opening-hand .opening-card");
            assert.equal(Object.keys(shown).length, 7, "seven cards shown");
            await page.getByRole("button", { name: /^Keep/ }).click();
            // The views after the keep arrive on the page's timers: the opening closes with
            // the first view of the first turn, and the hand starts its drop in that commit.
            for (let ms = 0; await page.locator(".opening").count(); ms += 4) {
                assert.ok(ms < 5000, "the opening closes");
                await page.clock.runFor(4);
            }
            const start = await middles(page, '.stage-node[data-zone="hand"]');
            await page.clock.runFor(1000);
            const end = await middles(page, '.stage-node[data-zone="hand"]');
            for (const [slot, from] of Object.entries(shown)) {
                const [a, b] = [start[slot], end[slot]];
                assert.ok(a && b, `card ${slot} is in the hand on the stage`);
                const fall = b.y - from.y;
                assert.ok(
                    fall > 40,
                    `the hand lies below the opening: ${fall}`,
                );
                // A frame of the drop may have passed: it starts near the opening's card,
                // well short of its place.
                assert.ok(
                    Math.abs(a.x - from.x) <
                        0.25 * Math.abs(b.x - from.x) + 8 &&
                        a.y - from.y < 0.4 * fall,
                    `card ${slot} starts at the opening's: ${JSON.stringify({ from, a, b })}`,
                );
            }
        },
        {
            fixture: "opening-keep",
            at: keepAsked + 1,
            ready: ".opening-card",
            phone,
            viewport: phone ? { width: 393, height: 759 } : undefined,
        },
    );
}
