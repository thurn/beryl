// The kept checks of a crowded table (kept.mjs): the words it stands, what sweeping it inspects,
// its plaques, the hand and the controls around it, a wide window and a long pile's viewer.
import assert from "node:assert/strict";
import { cutText } from "./capture.mjs";
import {
    ANSWERED,
    FIXTURE,
    PRESS,
    check,
    clicked,
    derive,
    drawn,
    load,
    overlap,
    pilesAt,
    replied,
    views,
} from "./kept-harness.mjs";

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
    // Why no audit: a count of the words standing on the table, held to a budget.
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
            ready: ".table",
            reducedMotion: "reduce",
        },
    );
}

// A late, crowded table: at least eight nonland permanents a side and a spell on the stack.
const crowdedViews = await load("crowded");
const crowdedAt = crowdedViews.findLastIndex(
    (v) =>
        v.status.decision?.kind === "priority" &&
        v.stack.some((item) => item.kind === "spell"),
);
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

// Why no audit: what each hover moves and where its inspector stands: a sweep of the pointer.
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
            // A hand card is read where it rises, with no inspector; any other is inspected.
            const raised = hand.some((h) => h.slot === slot);
            const inspect = page.locator(
                `.inspect[aria-label="${name}, inspected"]`,
            );
            assert.equal(
                await inspect.count(),
                raised ? 0 : 1,
                raised
                    ? `${name} raised has no inspector beside it`
                    : `the pointer at ${name}'s middle inspects it`,
            );
            const shown = raised
                ? await page
                      .locator(".stage-raised-copy > *")
                      .last()
                      .boundingBox()
                : await inspect.boundingBox();
            for (const f of forbidden) {
                // A raised hand card stands over your own life orb, as Arena's covers your
                // avatar (SswBO8aOOgc 21:02.65).
                if (raised && f.what === "you") continue;
                assert.ok(
                    !overlap(shown, f),
                    `${name}, ${raised ? "raised" : "inspected"}, covers ${f.what}`,
                );
            }
        }
    },
    {
        ...crowdedRun(crowdedAt),
    },
);

/** Each permanent's power and toughness (or loyalty) plaque on the table, by its card, and its
 * numerals' size on screen in window pixels: their font's size by the scales its card and the
 * stage are drawn at. A scan's printed text is too small to read there; the plaques are not. */
const plaqueSizes = (page) =>
    page.$$eval(
        '.card-el[data-zone="battlefield"]:not(.card-el-land) .card-pt span',
        (names) => {
            const scale = (e) => {
                const m = new DOMMatrix(getComputedStyle(e).transform);
                return Math.hypot(m.a, m.b);
            };
            const stage = scale(document.querySelector(".stage"));
            return names.map((e) => {
                const card = e.closest(".card-el");
                return {
                    name: card.dataset.card,
                    px:
                        parseFloat(getComputedStyle(e).fontSize) *
                        scale(card) *
                        stage,
                };
            });
        },
    );

// A crowded table's rows still fill the table between them, and their plaques stay readable.
// Why no audit: the least size a plaque is read at on the desktop, and the room between two rows.
await check(
    "a crowded table's plaques read at 14 px, its rows filling the room between them",
    async (page) => {
        await page.clock.runFor(2000);
        const sizes = await plaqueSizes(page);
        assert.ok(sizes.length >= 8, "both rows' plaques are drawn");
        const small = sizes
            .filter((n) => n.px < 14)
            .map((n) => `${n.name} at ${n.px.toFixed(1)} px`);
        assert.deepEqual(small, [], "plaques under 14 px");
        const { you, opponent } = await rows(page);
        const far = Math.max(...opponent.map((c) => c.box.y + c.box.height));
        const near = Math.min(...you.map((c) => c.box.y));
        // Only the lane where combat and the stack's arrows run lies between them.
        assert.ok(near - far <= 48, `${near - far} px between the rows`);
        assert.deepEqual(await cutText(page), [], "no name is cut");
    },
    {
        ...crowdedRun(crowdedAt),
    },
);

/** What lies over the opponent's hand, a permanent or a land, of what `by` matches: at each one's
 * middle and at its corners inset 8 px, as the ui-live harness's `probe.covered` looks. A copy
 * beneath another in a pile is covered by its own pile, which `by` does not match. */
const coveredBy = (page, by) =>
    page.evaluate((by) => {
        const out = [];
        for (const e of document.querySelectorAll(
            ".card-el-held, .card-el-field, .card-el-land",
        )) {
            const r = e.getBoundingClientRect();
            const points = [
                [r.left + r.width / 2, r.top + r.height / 2],
                [r.left + 8, r.top + 8],
                [r.right - 8, r.top + 8],
                [r.left + 8, r.bottom - 8],
                [r.right - 8, r.bottom - 8],
            ].filter(
                ([x, y]) =>
                    x >= 0 && y >= 0 && x < innerWidth && y < innerHeight,
            );
            for (const [x, y] of points) {
                const over = document.elementFromPoint(x, y)?.closest(by);
                if (over && !e.contains(over)) {
                    const what = e.dataset.card ?? "a back";
                    out.push(
                        `${what} under ${over.className} at ${Math.round(x)}, ${Math.round(y)}`,
                    );
                }
            }
        }
        return out;
    }, by);

/** What may never lie over a card on the table: a life orb, a tip, the game log or the reach of
 * a raised hand card. */
const COVERS = "[data-life], .tip, .game-log, .hand-hit";

// Eight cards in your hand while you may pass on to the next stop: the fan, each card at rest
// and raised in turn, stays clear of the round button left of the turn button (by-biv).
await derive("crowded", "crowded-eight", crowdedAt, (v) => {
    const hand = v.objects.filter(
        (o) => o.owner === "you" && o.zone === "hand",
    );
    const more = Array.from({ length: 8 - hand.length }, (_, i) => ({
        ...hand[i % hand.length],
        slot: 9200 + i,
    }));
    const decision = v.status.decision;
    return {
        ...v,
        objects: [...v.objects, ...more],
        status: {
            ...v.status,
            decision: {
                ...decision,
                passOn: decision.passOn ?? { kind: "nextTurn" },
            },
        },
    };
});
// Why no audit: the hand's cards, at rest and raised, against a button: cards the audit of controls
// does not weigh.
await check(
    "a full hand keeps clear of the pass-on button",
    async (page) => {
        await page.clock.runFor(2000);
        const button = await page.locator(".pass-on-button").boundingBox();
        assert.ok(button, "the pass-on button shows");
        // The right edge of everything drawn for a hand card, a raised one's copy included.
        const right = () =>
            page.$$eval(
                ".card-el-hand, .card-el-hand *, .card-ghost *",
                (all) =>
                    Math.max(
                        ...all.map((e) => e.getBoundingClientRect().right),
                    ),
            );
        const cards = await page.locator(".card-el-hand").all();
        assert.equal(cards.length, 8, "eight cards in hand");
        assert.ok(
            (await right()) < button.x,
            `the hand reaches ${await right()}, the button begins at ${button.x}`,
        );
        for (const card of cards) {
            const box = await card.boundingBox();
            await page.mouse.move(box.x + box.width / 2, box.y + 30);
            await page.clock.runFor(600);
            const name = await card.getAttribute("data-card");
            const reach = await right();
            assert.ok(
                reach < button.x,
                `${name} raised reaches ${reach}, the button begins at ${button.x}`,
            );
        }
    },
    {
        ...crowdedRun(crowdedAt),
        fixture: "crowded-eight",
    },
);

// The opponent holding nine cards: their fan's outer backs, which its turn lifts, still show.
await derive(FIXTURE, "held-nine", views.length - 2, (v) => {
    const held = v.objects.filter(
        (o) => o.owner === "opponent" && o.zone === "hand",
    );
    const more = Array.from({ length: 9 - held.length }, (_, i) => ({
        ...held[0],
        slot: 9100 + i,
    }));
    return { ...v, objects: [...v.objects, ...more] };
});
for (const [title, run] of [
    ["a crowded table", crowdedRun(crowdedAt)],
    [
        "a full opposing hand",
        {
            at: views.length - 1,
            ready: ".card-el-held",
            reducedMotion: "reduce",
        },
    ],
    [
        "nine cards in the opposing hand",
        {
            fixture: "held-nine",
            at: views.length - 1,
            ready: ".card-el-held",
            reducedMotion: "reduce",
        },
    ],
]) {
    // Why no audit: what a tip, the log and a raised card cover: states only the pointer enters.
    await check(
        `on ${title}, no orb, tip, log or raised hand card covers a card on the table`,
        async (page) => {
            await page.clock.runFor(2000);
            assert.deepEqual(await coveredBy(page, COVERS), [], "at rest");
            // Each of the opponent's backs reaches well into the window, to be counted.
            const feet = await page.$$eval(".card-el-held", (backs) =>
                backs.map((e) => Math.round(e.getBoundingClientRect().bottom)),
            );
            assert.ok(
                feet.every((y) => y >= 24),
                `the backs end at ${feet.join(", ")}`,
            );
            // Each hand card raised in turn, the pointer at its top.
            for (const card of await page.locator(".card-el-hand").all()) {
                const box = await card.boundingBox();
                await page.mouse.move(box.x + box.width / 2, box.y + 30);
                await page.clock.runFor(600);
                const name = await card.getAttribute("data-card");
                assert.deepEqual(
                    await coveredBy(page, COVERS),
                    [],
                    `${name} raised`,
                );
            }
            // Each of your piles' tips, over its pile, clear of your badge if it shows.
            const badges = page.locator(".player-badge");
            const badge = (await badges.count())
                ? await badges.last().boundingBox()
                : undefined;
            for (const pile of await page
                .locator(".has-tip:has([aria-label^=Library]), [data-side=you]")
                .all()) {
                const box = await pile.boundingBox();
                if (!box || box.y < 450) continue;
                await page.mouse.move(box.x + box.width / 2, box.y + 20);
                await page.clock.runFor(300);
                const tip = page.locator(".tip").filter({ visible: true });
                assert.equal(await tip.count(), 1, "the pile's tip shows");
                assert.ok(
                    !badge || !overlap(await tip.boundingBox(), badge),
                    `${await tip.innerText()} covers your badge`,
                );
                assert.deepEqual(await coveredBy(page, COVERS), [], "a tip");
            }
            const toggle = page.getByRole("button", { name: "Show log" });
            if (await toggle.isVisible()) {
                await toggle.click();
                await page.clock.runFor(2000);
                assert.deepEqual(
                    await coveredBy(page, COVERS),
                    [],
                    "the log open",
                );
            }
        },
        {
            ...run,
        },
    );
}

// At 16:10, the table reaches the window's edges rather than stand in bands, and each hand
// runs off its edge of the window as at 16:9, showing no less of each card.
// Why no audit: bare points at a window's edges, and how much of each hand shows there against
// 16:9.
await check(
    "a 16:10 window shows the table to its edges, both hands reaching them",
    async (page) => {
        await page.clock.runFor(2000);
        const { width, height } = page.viewportSize();
        const bare = await page.evaluate(
            ([w, h]) => {
                const frame = document.querySelector(".stage-frame");
                const points = [0.1, 0.3, 0.5, 0.7, 0.9].flatMap((f) => [
                    [w * f, 16],
                    [w * f, h - 16],
                    [16, h * f],
                    [w - 16, h * f],
                ]);
                return points
                    .filter(
                        ([x, y]) => document.elementFromPoint(x, y) === frame,
                    )
                    .map((p) => p.map(Math.round).join(", "));
            },
            [width, height],
        );
        assert.deepEqual(
            bare,
            [],
            "points 16 px from an edge showing no table",
        );
        // Each card of both hands runs off the window's edge, as at 16:9, showing at least as
        // much of itself as a 16:9 window would: as much as lies on the stage.
        const hands = await page.$$eval(
            ".card-el-hand, .card-el-held",
            (cards) => {
                const stage = document
                    .querySelector(".stage")
                    .getBoundingClientRect();
                const part = (r, top, bottom) =>
                    (Math.min(r.bottom, bottom) - Math.max(r.top, top)) /
                    r.height;
                return cards.map((e) => {
                    const r = e.getBoundingClientRect();
                    const held = e.classList.contains("card-el-held");
                    return {
                        slot: e.dataset.slot,
                        held,
                        shown: part(r, 0, innerHeight),
                        wide: part(r, stage.top, stage.bottom),
                        reaches: held ? r.top < 0 : r.bottom > innerHeight,
                        inside: r.left >= 0 && r.right <= innerWidth,
                    };
                });
            },
        );
        assert.ok(
            hands.some((c) => c.held) && hands.some((c) => !c.held),
            "both hands hold cards",
        );
        for (const c of hands) {
            assert.ok(
                c.reaches,
                `slot ${c.slot} ends short of the window's edge`,
            );
            assert.ok(c.inside, `slot ${c.slot} is cut at the window's side`);
            assert.ok(
                c.shown >= c.wide - 0.01,
                `slot ${c.slot} shows ${c.shown.toFixed(2)} of itself, ` +
                    `${c.wide.toFixed(2)} at 16:9`,
            );
        }
    },
    {
        at: views.length - 1,
        ready: ".card-el-held",
        reducedMotion: "reduce",
        viewport: { width: 1280, height: 800 },
    },
);

// A pile too long for its viewer fades out where more of it lies, until scrolled there.
const crowdedYou = crowdedViews[crowdedAt].objects.find(
    (o) =>
        o.zone === "battlefield" &&
        o.controller === "you" &&
        !o.types.includes("land") &&
        o.name,
);
await derive("crowded", "crowded-graveyard", crowdedAt, (v) => {
    const dead = Array.from({ length: 14 }, (_, i) => ({
        ...crowdedYou,
        slot: 9000 + i,
        zone: "graveyard",
        tapped: false,
        combat: null,
        counters: [],
        damage: 0,
    }));
    return {
        ...v,
        objects: [...v.objects, ...dead],
        players: {
            ...v.players,
            you: {
                ...v.players.you,
                graveyard: [
                    ...v.players.you.graveyard,
                    ...dead.map((o) => o.slot),
                ],
            },
        },
    };
});
// Why no audit: a fade that follows a scroll.
await check(
    "a long pile's viewer fades where more of it lies beyond the scroll",
    async (page) => {
        await page.clock.runFor(2000);
        await page.click("[data-pile=graveyard][data-side=you]");
        await page.clock.runFor(1000);
        const list = page.locator(".zone-viewer-cards");
        const state = () =>
            list.evaluate((e) => ({
                over: e.scrollHeight > e.clientHeight,
                more: e.dataset.more,
                mask:
                    getComputedStyle(e).maskImage ||
                    getComputedStyle(e).webkitMaskImage,
            }));
        const top = await state();
        assert.ok(top.over, "fourteen cards overflow the viewer");
        assert.equal(top.more, "below");
        assert.notEqual(top.mask, "none", "the list's foot fades");
        await list.evaluate((e) => {
            e.scrollTop = e.scrollHeight;
        });
        // The browser sends the scroll event with its next frame, in its own time: the
        // tries only guard against a hang, with room for a loaded machine.
        let more;
        for (let tries = 0; tries < 200 && more !== "above"; tries++) {
            await new Promise((resolve) => setTimeout(resolve, 100));
            await page.clock.runFor(100);
            ({ more } = await state());
        }
        assert.equal(more, "above", "scrolled to its end");
    },
    {
        ...crowdedRun(crowdedAt),
        fixture: "crowded-graveyard",
    },
);

await replied("mana-floating-pass", "mana-floating", [ANSWERED]);

// Why no audit: a pulse a press begins and a moment ends, and that the press asks nothing.
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
        assert.deepEqual(await clicked(page, 1), [PRESS("decline")]);
        // The step ends and the pool empties; it goes once the pulse is over.
        await page.clock.runFor(5000);
        assert.equal(await pool.count(), 0, "the emptied pool goes");
    },
    {
        fixture: "mana-floating-pass",
        at: floatingAt,
        ready: ".card-el:is(.card-el-hand, .card-el-land, .card-el-field)",
    },
);
