// The kept checks of what the pointer brings up (kept.mjs): a hovered card's rise and what
// holds still, the ring linking an attachment, refusals and their reasons, the lands a button
// would tap, and where the inspector stands against a prompt and its targets.
import assert from "node:assert/strict";
import {
    ANSWERED,
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
        await page.getByLabel("Counterspell, inspected").waitFor();
        assert.equal(
            await inspect.locator(".inspect-refusal").count(),
            0,
            "hovering alone says nothing",
        );
        await page.click(counterspell);
        assert.equal(
            await inspect.locator(".inspect-refusal").innerText(),
            "No target",
        );
        assert.equal((await cues(page)).at(-1), "no", "a refusal sounds no");
        await page.hover(".turn-action");
        await page.clock.runFor(500);
        await page.hover(counterspell);
        await page.getByLabel("Counterspell, inspected").waitFor();
        assert.equal(
            await inspect.locator(".inspect-refusal").count(),
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
