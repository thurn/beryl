// The kept checks of what the pointer brings up (kept.mjs): a hovered card's rise and what
// holds still, the ring linking an attachment, refusals and their reasons, the lands a button
// would tap, and where the inspector stands against a prompt and its targets.
import assert from "node:assert/strict";
import {
    ANSWERED,
    ON,
    SAME,
    card,
    center,
    check,
    chosen,
    clicked,
    committed,
    drawn,
    lastDecision,
    load,
    overlap,
    replied,
    views,
} from "./kept-harness.mjs";

/** The sounds the page has asked for so far (src/audio), oldest first. */
const cues = (page) =>
    page.evaluate(() => (window.berylSounds ?? []).map((s) => s.sound));

const fieldCard = `[data-slot="${
    views
        .at(-2)
        .objects.find(
            (o) => o.zone === "battlefield" && !o.types.includes("land"),
        ).slot
}"]`;
/** The card of your hand's fan that your permanents stand over least: a hand card rises only as
 * far as it keeps clear of your back row (game/handLayout.ts `handLifts`). */
const handCard = (page) =>
    page.evaluate(() => {
        const boxes = (selector) =>
            [...document.querySelectorAll(selector)].map((e) => [
                e,
                e.getBoundingClientRect(),
            ]);
        const yours = boxes('.card-el[data-zone="battlefield"]')
            .map(([, r]) => r)
            .filter((r) => r.top > innerHeight / 2);
        const under = (r) =>
            yours.reduce(
                (sum, p) =>
                    sum +
                    Math.max(
                        0,
                        Math.min(r.right, p.right) - Math.max(r.left, p.left),
                    ),
                0,
            );
        const [[least]] = boxes(
            '.card-el-hand[data-slot][data-zone="hand"]',
        ).sort((a, b) => under(a[1]) - under(b[1]));
        return `[data-slot="${least.dataset.slot}"]`;
    });
for (const [title, hand] of [
    ["a hand card", true],
    ["a battlefield card", false],
]) {
    // Why no audit: what a hover moves, and that the card it raises keeps the pointer: a sweep of
    // every card's box as the pointer rests.
    await check(
        `hovering ${title} moves no other card and starts no layout animation`,
        async (page) => {
            await page.clock.runFor(2000);
            const target = hand ? await handCard(page) : fieldCard;
            const still = await drawn(page);
            const slot = await page.$eval(target, (e) => e.dataset.slot);
            const box = await page.locator(target).boundingBox();
            const at = { x: box.x + box.width / 2, y: box.y + 40 };
            await page.mouse.move(at.x, at.y);
            for (let t = 0; t < 20; t++) {
                await page.clock.runFor(50);
                // What the hover set off, committed by React (`committed`), as it stands now.
                await committed(page);
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
            if (hand) {
                assert.ok(
                    still[slot].box[1] - top > 40,
                    `the hovered hand card, slot ${slot}, rises ${still[slot].box[1] - top} px`,
                );
                // Raised whole, larger than in the fan, its foot on the window's bottom edge.
                const raised = await page
                    .locator(".stage-raised-copy > *")
                    .first()
                    .boundingBox();
                const { height } = page.viewportSize();
                assert.ok(
                    raised.y + raised.height <= height &&
                        raised.y + raised.height > height - 20,
                    `the raised card's foot is at ${raised.y + raised.height}`,
                );
                assert.ok(
                    raised.width > 1.3 * box.width,
                    `the raised card is ${raised.width} wide, ${box.width} in the fan`,
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
            // A hand card is read where it rises; any other is shown whole beside the table.
            assert.equal(
                !!(await page.$(".inspect")),
                !hand,
                hand
                    ? "no inspector beside a raised hand card"
                    : "the card is inspected",
            );
        },
    );
}

// Why no audit: the audits see the page at rest, before and after a hover, and a card that
// snaps up or down looks the same there as one that eases. This steps the browser's frames
// the copy eases on (src/stage/raised.ts) and reads how high it is drawn in between.
await check(
    "a hovered hand card eases up out of the fan and back down as the pointer leaves",
    async (page) => {
        await page.clock.runFor(2000);
        const target = await handCard(page);
        const box = await page.locator(target).boundingBox();
        /** The top of the raised copy's card, or none while no copy is drawn. */
        const copyTop = () =>
            page.evaluate(
                () =>
                    document
                        .querySelector(".stage-raised-copy > *")
                        ?.getBoundingClientRect().top,
            );
        const rest = (await drawn(page))[
            await page.$eval(target, (e) => e.dataset.slot)
        ].box[1];
        /** How high the copy is drawn at each 8 ms of the next 400 ms. */
        const sampled = async () => {
            const tops = [];
            for (let t = 0; t < 50; t++) {
                await page.clock.runFor(8);
                await committed(page);
                tops.push(await copyTop());
            }
            return tops;
        };
        /** Whether any of `tops` is clear of both `a` and `b`, between them. */
        const between = (tops, a, b) =>
            tops.some(
                (top) =>
                    top !== undefined &&
                    top > Math.min(a, b) + 2 &&
                    top < Math.max(a, b) - 2,
            );
        await page.mouse.move(box.x + box.width / 2, box.y + 40);
        const rising = await sampled();
        const up = rising.at(-1);
        assert.ok(rest - up > 40, `the card rises ${rest - up} px`);
        assert.ok(
            between(rising, rest, up),
            `the card is drawn on its way up: ${rising.join(" ")}`,
        );
        // Off the hand, onto the felt above it.
        await page.mouse.move(box.x + box.width / 2, box.y - 300);
        const dropping = await sampled();
        assert.ok(
            between(dropping, up, rest),
            `the card is drawn on its way back down: ${dropping.join(" ")}`,
        );
        assert.equal(
            dropping.at(-1),
            undefined,
            "the card is back in the fan, with no copy over it",
        );
    },
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

const auraHost = attachedBySlot.get(aura.attachedTo);

// Why no audit: a ring only the pointer brings up, and which card lies over which where two
// overlap.
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

const timingViews = await load("teferi-timing");

const timingView = timingViews[0];

const timingCard = (name) =>
    `[data-slot="${timingView.objects.find((o) => o.name === name).slot}"]`;

await replied("teferi-timing-refused", "teferi-timing", [
    SAME(timingView.status.decision),
]);

// Why no audit: a word a refused click brings up for a moment, and where it stands by the permanent
// that forbids.
await check(
    "Teferi's timing refusal appears briefly beside its source",
    async (page) => {
        await page.clock.runFor(2000);
        const source = page.locator(timingCard("Teferi, Time Raveler"));
        const notice = page.locator(".timing-refusal");
        await page.click(timingCard("Thundertrap Trainer"));
        assert.equal(await notice.count(), 0, "ordinary timing has no source");
        await page.click(timingCard("Thirst for Discovery"));
        await notice.waitFor();
        await page.clock.runFor(500);
        assert.equal(await notice.textContent(), "Sorcery speed only");
        const at = await notice.boundingBox();
        const teferi = await source.boundingBox();
        assert.ok(
            Math.abs(center(at).x - center(teferi).x) < 3,
            "the notice is centered on the opposing source",
        );
        const gap = at.y - (teferi.y + teferi.height);
        assert.ok(
            gap >= -1 && gap < 12,
            `the notice is just below the source: ${gap}`,
        );
        assert.ok(
            Number(await notice.evaluate((e) => getComputedStyle(e).opacity)) >
                0.9,
        );
        await page.mouse.move(800, 380);
        assert.equal(await notice.count(), 1, "feedback outlasts hover");
        await page.clock.runFor(2000);
        await notice.waitFor({ state: "detached" });
        assert.deepEqual(
            await chosen(page),
            [],
            "the rejected click sends no action",
        );
    },
    {
        fixture: "teferi-timing-refused",
        at: 1,
        ready: timingCard("Thirst for Discovery"),
        reducedMotion: "reduce",
    },
);

// Mana Tithe asks the human to pay {1} on the board, the table in view: the land the payment
// would tap glows on hover; the prompt answers from its buttons.
const pay = await lastDecision("choice-pay");

const payTaps = pay.decision.candidates.find((c) => c.option.kind === "yes")
    .payments[0].taps;

/** On the stage: the permanents whose mark of what a button would tap shows. */
const suggested = (page) =>
    page.$$eval(".stage-mark-suggested", (marks) =>
        marks
            .filter((mark) => Number(getComputedStyle(mark).opacity) > 0)
            .map((mark) => Number(mark.parentElement.dataset.slot))
            .sort(),
    );

await replied("choice-pay-click", "choice-pay", [ANSWERED]);

// Why no audit: what the pointer over a button lights on the table, and puts out as it leaves.
await check(
    "a pay-or-counter prompt lights the land that would pay",
    async (page) => {
        await page.clock.runFor(2000);
        assert.equal(await page.$(".choice"), null, "no overlay opens");
        assert.equal(await page.$(".table-dimmed"), null, "the table shows");
        assert.deepEqual(await suggested(page), []);
        await page.getByRole("button", { name: /^Pay/ }).hover();
        await committed(page);
        assert.deepEqual(
            await suggested(page),
            payTaps,
            "the paying land glows",
        );
        await page.mouse.move(2, 2);
        await committed(page);
        assert.deepEqual(
            await suggested(page),
            [],
            "and no longer once it leaves",
        );
        await page.getByRole("button", { name: "Decline" }).click();
        const { id } = pay.decision.prompt.buttons.find(
            (button) => button.label === "Decline",
        );
        assert.deepEqual(await clicked(page, 1), [
            { kind: "button", button: id },
        ]);
    },
    {
        fixture: "choice-pay-click",
        at: pay.views.length,
        ready: ".stage-prompt",
    },
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

// A click on a hand card with nothing to offer (by-isv.23): the inspector says why, as the
// engine does. In `menu-choices` Counterspell has no spell to counter.
const refusedCard = menus.decision.refused.find(
    (r) => nameOfSlot(menuView, r.slot) === "Counterspell",
);

await replied("menu-refused", "menu-choices", [SAME(menus.decision)]);

// Why no audit: what the inspector says after a refused click, and its sound: a state a click
// enters and the pointer leaves.
await check(
    "clicking a refused card shows why in the inspector",
    async (page) => {
        await page.clock.runFor(3000);
        const counterspell = `[data-slot="${refusedCard.slot}"]`;
        const inspect = page.locator(".inspect");
        await page.hover(counterspell);
        await page.clock.runFor(300);
        // A hand card rises to be read in place, with no inspector beside it.
        assert.equal(await inspect.count(), 0, "hovering alone says nothing");
        await page.click(counterspell);
        assert.equal(
            await inspect.locator(".inspect-refusal").innerText(),
            "No target",
        );
        assert.equal((await cues(page)).at(-1), "no", "a refusal sounds no");
        await page.hover(".turn-action");
        await page.clock.runFor(500);
        await page.hover(counterspell);
        await page.clock.runFor(300);
        assert.equal(
            await inspect.count(),
            0,
            "the reason goes once the pointer leaves",
        );
    },
    {
        fixture: "menu-refused",
        at: menus.views.length,
        ready: ".turn-action",
    },
);

/** The inspector's box, once it shows the card `name`. */
async function inspectorOf(page, name) {
    const inspect = page.locator(`.inspect[aria-label="${name}, inspected"]`);
    await inspect.waitFor({ state: "visible" });
    return inspect.boundingBox();
}

/** A point of the element `selector` picks (the `nth`) where it is on top, so the pointer
 * reaches it: its middle if nothing covers that, else the first such point on a 10 px grid.
 * Whole pixels, where WebKit delivers the pointer: a fraction of a pixel inside a leaning
 * card's edge may lie outside it there. */
const visiblePoint = (page, selector, nth) =>
    page.evaluate(
        ([selector, nth]) => {
            const e = document.querySelectorAll(selector)[nth];
            const r = e.getBoundingClientRect();
            const on = (x, y) => e.contains(document.elementFromPoint(x, y));
            const mid = {
                x: Math.round(r.x + r.width / 2),
                y: Math.round(r.y + r.height / 2),
            };
            if (on(mid.x, mid.y)) return mid;
            for (let y = Math.ceil(r.top) + 5; y < r.bottom; y += 10) {
                for (let x = Math.ceil(r.left) + 5; x < r.right; x += 10) {
                    if (on(x, y)) return { x, y };
                }
            }
            return null;
        },
        [selector, nth],
    );

// A prompt's cards show whole on hover, beside its title, options and cards, never over
// them, on an opaque inspector (by-isv.17): a look at the top four.
for (const [fixture, at] of [
    ["choice-look", (await load("choice-look")).length],
]) {
    // Why no audit: where the inspector stands against a prompt's cards and buttons: a place only
    // the pointer on each card gives it.
    await check(
        `hovering a card in ${fixture}'s prompt keeps its inspector off the prompt`,
        async (page) => {
            await page.clock.runFor(2000);
            // The cards looked at are the library's own, held up over the table (its depths
            // 850 to 899, desktop.ts), and the prompt is the dock's.
            await page.evaluate(() => {
                for (const node of document.querySelectorAll("[data-stage]")) {
                    const depth = Number(node.style.zIndex);
                    if (depth >= 850 && depth < 900)
                        node.classList.add("held-up");
                }
            });
            const prompt = await page.$$eval(
                ".stage-prompt :is(.prompt-title, button), .held-up",
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
            const cards = ".held-up";
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
        {
            fixture,
            at,
            ready: ".stage-prompt",
        },
    );
}

// The prompt's title and buttons are drawn over everything at rest: the cards, the tray's
// shade, the cards a prompt holds up (850 to 899, desktop.ts), the order and choose layers
// and the arrows; only a card in flight (900 and over) passes over the dock. A layer over it
// dimmed the question (by-pehx) and a card over it hid the title (by-eidz).
// Why no audit: the generic overlap audit compares boxes, not paint order, and no fixture
// has a layer standing under the dock's box.
for (const fixture of ["combat-declare", "choice-look"]) {
    await check(
        `the ${fixture} prompt is drawn over every layer at rest`,
        async (page) => {
            await page.clock.runFor(2000);
            const { dock, layers, top } = await page.evaluate(() => {
                const depth = (e) => Number(getComputedStyle(e).zIndex) || 0;
                const layers = [
                    ...document.querySelectorAll(
                        "[data-stage], .stage-tray, .stage-choose, .stage-order, .stage-lines, .stage-lights",
                    ),
                ]
                    .map(depth)
                    .filter((d) => d < 900);
                const dock = document.querySelector(".dock");
                const top = [
                    ...dock.querySelectorAll(".prompt-title, button"),
                ].map((e) => {
                    const r = e.getBoundingClientRect();
                    const hit = document.elementFromPoint(
                        r.x + r.width / 2,
                        r.y + r.height / 2,
                    );
                    return dock.contains(hit);
                });
                return { dock: depth(dock), layers, top };
            });
            assert.ok(layers.length > 0, "the stage draws layers");
            assert.ok(
                dock > Math.max(...layers),
                `a layer (${Math.max(...layers)}) is over the dock (${dock})`,
            );
            assert.ok(dock < 900, `the dock (${dock}) is over a flying card`);
            assert.ok(
                top.length > 0 && top.every(Boolean),
                "something is drawn over the dock's title or buttons",
            );
        },
        {
            fixture,
            at: (await load(fixture)).length,
            ready: ".dock .prompt-title",
        },
    );
}

// In a target step, a card shows whole clear of every target it might cover (by-isv.17):
// Fractured Identity's in greedy-turn-9, whose targets fill the table's middle, and Swift
// Reconfiguration's in attached-across and in crew, whose targets lie on both sides.
for (const fixture of ["greedy-turn-9", "attached-across", "crew"]) {
    const views = await load(fixture);
    const at = views.findIndex((v) => v.status.decision?.kind === "target");
    // Why no audit: where the inspector stands against every target, for the pointer on each card
    // in turn.
    await check(
        `hovering a card in ${fixture}'s target step covers no target`,
        async (page) => {
            await page.clock.runFor(2000);
            const targets = await page.$$eval(
                '[data-zone="battlefield"]:has(> .stage-mark)',
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
                // A hand card is read where it rises; any other in the inspector.
                const hand = await page
                    .locator(cards)
                    .nth(i)
                    .evaluate((e) => e.classList.contains("card-el-hand"));
                const shown = hand
                    ? await page
                          .locator(".stage-raised-copy > *")
                          .first()
                          .boundingBox()
                    : await inspectorOf(page, name);
                for (const t of targets) {
                    assert.ok(
                        !overlap(shown, t),
                        `${name}, ${hand ? "raised" : "inspected"}, covers the target ${t.what}`,
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

// Why no audit: two Plains look the same, so no screen audit can tell which of them flew. The
// engine offers one candidate for both (architecture §7.2), and the session takes a drag of
// either for it: this follows the card the pointer let go of by where each Plains is drawn.
const twinViews = await load("hand-pair-twin");
const plainsIn = (v) =>
    v.objects
        .filter((o) => o.owner === "you" && o.zone === "hand")
        .filter((o) => o.name === "Plains")
        .map((o) => o.slot);
const [handled] = plainsIn(twinViews.at(-1));
const [offered] = plainsIn(twinViews[0]).filter((slot) => slot !== handled);
assert.ok(offered !== undefined, "the fixture plays the other Plains");
await check(
    "of two Plains in hand, the one dragged out is the one that plays",
    async (page) => {
        await page.clock.runFor(2000);
        const before = await drawn(page);
        const box = await page
            .locator(`[data-slot="${handled}"]`)
            .boundingBox();
        const from = { x: box.x + box.width / 2, y: box.y + 40 };
        const to = { x: from.x + 40, y: from.y - 320 };
        await page.mouse.move(from.x, from.y);
        await page.mouse.down();
        for (let i = 1; i <= 10; i++) {
            const k = i / 10;
            await page.mouse.move(
                from.x + (to.x - from.x) * k,
                from.y + (to.y - from.y) * k,
            );
        }
        await page.mouse.up();
        assert.deepEqual(await clicked(page, 1), [ON(handled)]);
        await page.clock.runFor(16);
        await committed(page);
        const now = await drawn(page);
        const middle = ([left, top, right, bottom]) => ({
            x: (left + right) / 2,
            y: (top + bottom) / 2,
        });
        const off = (a, b) => Math.hypot(a.x - b.x, a.y - b.y);
        // The Plains that plays starts from where the dragged one was let go, and the one
        // left in hand is where the other lay.
        const flying = middle(now[offered].box);
        const dropped = middle(before[handled].box);
        assert.ok(
            off(flying, { x: dropped.x + 40, y: dropped.y - 320 }) < 80,
            `the Plains played starts at ${JSON.stringify(flying)}, not where it was let go`,
        );
        assert.ok(
            off(middle(now[handled].box), middle(before[offered].box)) < 20,
            "the Plains left in hand is where the other one lay",
        );
        await page.clock.runFor(3000);
        assert.equal(
            await page.$eval(`[data-slot="${offered}"]`, (e) => e.dataset.zone),
            "battlefield",
        );
    },
    {
        fixture: "hand-pair-twin",
        at: 1,
        ready: `[data-slot="${handled}"][data-zone="hand"]`,
    },
);

// Nothing follows the pointer over a prompt's cards: a Snapcaster Mage trigger chooses among the
// graveyard's cards, held up over the dimmed table, and the arrow from the trigger to the
// cursor (UI Conventions, "An arrow to the pointer") is not drawn across them. The page's
// layer says whether the stage pulls anything (`data-aimed`), which a browser without WebGL
// shows no arrow for either. No generic check sees it: it is the absence of a trace that follows
// the mouse, not a state of the table.
await check(
    "no arrow follows the cursor over cards a target prompt holds up",
    async (page) => {
        await page.waitForSelector(".stage-tray");
        for (const [x, y] of [
            [500, 300],
            [900, 500],
        ]) {
            await page.mouse.move(x, y);
            assert.equal(
                await page.locator("[data-aimed]").count(),
                0,
                `no arrow to the cursor at ${x}, ${y}`,
            );
        }
    },
    {
        fixture: "target-graveyard",
        ready: ".stage-tray",
        at: (await load("target-graveyard")).length,
    },
);
