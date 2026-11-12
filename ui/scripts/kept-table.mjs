// The kept checks of the table's cards (kept.mjs): the hand's fan, the inspector, where rows,
// blockers and face-down permanents stand, badges, marks, tokens and ability frames.
import assert from "node:assert/strict";
import {
    ANSWERED,
    FIXTURE,
    ON,
    card,
    cardRecords,
    center,
    check,
    clicked,
    drawn,
    load,
    name,
    overlap,
    replied,
    slotOf,
    tokenField,
    tokenRun,
} from "./kept-harness.mjs";

// Why no audit: which card the pointer's inspector shows, and its artist's line.
await check("hovering a card shows it whole, with its artist", async (page) => {
    await page.clock.runFor(2000);
    await page.hover(card);
    const { artist } = (await cardRecords(FIXTURE)).cards[name];
    await page
        .locator(".inspect")
        .getByText(`Illustrated by ${artist}`)
        .waitFor();
});

// A quiet table with a row of creatures a side: nothing on the stack, no combat.
const quietViews = await load("combat-blockers");
const nonlands = (view, side) =>
    view.objects.filter(
        (o) =>
            o.zone === "battlefield" &&
            o.controller === side &&
            !o.types.includes("land"),
    );
// The creatures' row (layout.ts ROWS): the other permanents stand behind it, beside the lands.
const creatures = (view, side) =>
    nonlands(view, side).filter((o) => o.types.includes("creature"));
const quiet = quietViews.findIndex(
    (v) =>
        v.status.decision?.kind === "priority" &&
        v.stack.length === 0 &&
        !v.objects.some((o) => o.combat) &&
        creatures(v, "you").length >= 3 &&
        creatures(v, "opponent").length >= 3,
);
for (const viewport of [
    { width: 1280, height: 800 },
    { width: 1600, height: 900 },
]) {
    // Why no audit: where a row stands between the table's middle and the stack: a measure of the
    // layout, which the screen audits hold only to its view and to overlap.
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
                const row = await boxes(creatures(quietViews[quiet], side));
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
            ready: `[data-slot="${creatures(quietViews[quiet], "you")[0].slot}"]`,
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

// Blockers declared: each blocker stands across from its attacker, and the rest of its row
// makes room rather than lying under it.
const blockersViews = await load("combat-blockers");
const blockersView = blockersViews.at(-1);
const defender = blockersView.objects.find(
    (o) => o.combat?.kind === "blocking",
)?.controller;
// The defending side's nonland permanents.
const standing = blockersView.objects.filter(
    (o) =>
        o.zone === "battlefield" &&
        o.controller === defender &&
        !o.types.includes("land"),
);

// Why no audit: a blocker's place by its attacker's: a measure between two cards.
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
// Why no audit: what lies on top at each plaque: a hit test between cards, which may overlap one
// another.
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
// The face-up permanents: each row's cards share a size, and a tapped one lies on its side.
const faceUp = blockersView.objects.filter(
    (o) =>
        o.zone === "battlefield" &&
        o.facing === "up" &&
        !o.types.includes("land"),
);
// Why no audit: a face-down card's frame beside a face-up one's of its row: a comparison of sizes.
await check(
    "a face-down permanent stands in a permanent's frame, a 2/2",
    async (page) => {
        await page.clock.runFor(2000);
        const el = (o) => page.locator(`.card-el[data-slot="${o.slot}"]`);
        // Untransformed sizes, which a tapped card's turn leaves alone.
        const size = (o) =>
            el(o).evaluate((e) => [e.offsetWidth, e.offsetHeight]);
        for (const o of faceDown) {
            const row = faceUp.filter((u) => u.controller === o.controller);
            const like = row.find((u) => u.tapped === o.tapped) ?? row[0];
            assert.ok(like, `a face-up permanent shares ${o.slot}'s row`);
            const [width, height] = await size(like);
            assert.match(
                await el(o).getAttribute("class"),
                /card-el-field/,
                `${o.slot} is drawn as a permanent`,
            );
            const [w, h] = await size(o);
            // Turned a quarter from `like`, its footprint is `like`'s turned.
            const [ww, hh] =
                like.tapped === o.tapped ? [width, height] : [height, width];
            assert.ok(
                Math.abs(w - ww) <= 1 && Math.abs(h - hh) <= 1,
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

// Teferi, Time Raveler at 1 loyalty can activate only its +1: a loyalty ability cannot be
// taken back, so the click still opens the chooser, the −3 shown disabled.
const lowViews = await load("walker-low");
const lowView = lowViews.at(-1);
const raveler = lowView.objects.find(
    (o) => o.name === "Teferi, Time Raveler" && o.zone === "battlefield",
).slot;
for (const phone of [false, true]) {
    // Why no audit: where a number sits in its glyph: a proportion inside one mark.
    await check(
        `the loyalty badge centers the number in the shield on ${phone ? "phone" : "desktop"}`,
        async (page) => {
            const badge = page.locator(
                `[data-slot="${raveler}"] .card-loyalty`,
            );
            const numberCenter = await badge.evaluate((e) => {
                const shield = e
                    .querySelector(".card-loyalty-glyph")
                    .getBoundingClientRect();
                const number = e.querySelector("b").getBoundingClientRect();
                return (
                    (number.top + number.height / 2 - shield.top) /
                    shield.height
                );
            });
            assert.ok(
                numberCenter >= 0.54 && numberCenter <= 0.64,
                `the number sits in the body below the shield's top notch: ${numberCenter}`,
            );
        },
        {
            fixture: "walker-low",
            at: lowViews.length,
            ready: ".card-loyalty",
            phone,
            viewport: phone ? { width: 390, height: 844 } : undefined,
        },
    );
}
// `marks` ends on a board where a permanent bearing one mark and no other stands beside a twin
// of the same name bearing none: +1/+1 counters, summoning sickness and story or time counters.
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
    const [plusOne] = MARKS;
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
    const marks = await load("marks");
    // Why no audit: what the inspector says of the card under the pointer.
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
        {
            fixture: "marks",
            at: marks.length,
            ready: ".card-counters",
        },
    );
}

// A card the opponent's Elite Spellbinder exiled from the human's hand, which they may cast from
// exile (`exile-cast`): it waits in their hand fan, outlined apart, and a click there casts it.
// The human also knows a card in the opponent's hand, one their Sink into Stupor returned there.
const exileViews = await load("exile-cast");
const exileView = exileViews.at(-1);
const exileCast = exileView.status.decision.candidates.findIndex(
    (c) =>
        c.option.kind === "cast" &&
        exileView.objects.find((o) => o.slot === c.option.slot)?.zone ===
            "exile",
);
const exiledSlot = exileView.status.decision.candidates[exileCast].option.slot;
const exileRun = {
    fixture: "exile-cast",
    at: exileViews.length,
    ready: `${slotOf(exiledSlot)}.card-el-hand`,
};

const theirHand = exileView.objects.filter(
    (o) => o.zone === "hand" && o.owner === "opponent",
);
const knownCard = theirHand.find((o) => o.name !== null);
// Why no audit: which of the opponent's cards the viewer knows: the screen audits read the viewer's
// own hand alone.
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
    {
        ...exileRun,
    },
);

const stolen = tokenField.filter((o) => o.owner !== o.controller);
// Why no audit: a frame and an edge against who made and who owns each permanent, which the screen
// audits do not read.
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
    {
        ...tokenRun,
    },
);

// Why no audit: the shape of a frame's corners: skin no audit measures.
await check(
    "ability frames have square tops and shield bottoms",
    async (page) => {
        await page.clock.runFor(1000);
        const corners = await page
            .locator(".stack-item")
            .first()
            .evaluate((e) => {
                const s = getComputedStyle(e);
                return [s.borderTopLeftRadius, s.borderBottomLeftRadius];
            });
        assert.equal(parseFloat(corners[0]), 0);
        assert.ok(
            parseFloat(corners[1]) >= 20,
            `rounded shield bottom ${corners[1]}`,
        );
    },
    {
        fixture: "arrow-ability",
        at: (await load("arrow-ability")).length,
        ready: ".stack-item",
    },
);

const yourExile = '[data-pile="exile"][data-side="you"]';

await replied("exile-cast-click", "exile-cast", [ANSWERED]);

// Why no audit: the outline of a card the hand holds from another zone, whose pile is gone: a look
// and an absence no audit reads.
await check(
    "a card castable from exile waits in the hand fan and is cast from there",
    async (page) => {
        await page.clock.runFor(2000);
        assert.ok(
            await page.$(`${slotOf(exiledSlot)} .card-elsewhere`),
            "it is outlined apart from the hand's own cards",
        );
        assert.equal(await page.$(yourExile), null, "its exile pile is gone");
        await page.click(slotOf(exiledSlot));
        // With nothing to choose, the pick casts it at once: the session's to say, which the
        // page sends the click.
        assert.deepEqual(await clicked(page, 1), [ON(exiledSlot)]);
    },
    {
        ...exileRun,
        fixture: "exile-cast-click",
    },
);
