import assert from "node:assert/strict";
import {
    clippedText,
    crowdedControls,
    phoneFaults,
    screenFaults,
    stageFaults,
} from "./capture.mjs";
import { center, check } from "./kept-harness.mjs";
import {
    clickTable,
    startTable,
    tableActionRef,
    tableRoute,
    tableView,
} from "./table.mjs";

export const phone = {
    route: (page) => tableRoute(page, true),
    ready: ".start-panel",
    serial: true,
    phone: true,
    viewport: { width: 393, height: 759 },
    init: () => {
        // A scripted pointer has no native capture. Leave real touches alone.
        const capture = Element.prototype.setPointerCapture;
        Element.prototype.setPointerCapture = function (id) {
            if (id !== 100) capture.call(this, id);
        };
    },
};

export const deck = "24 Forest";
export const moveLand = (view) => ({
    move: {
        to: "battlefield",
        ...tableActionRef(view, {
            kind: "object",
            slot: view.table.orders.you.hand.at(-1),
        }),
    },
});
export const card = (page, slot) =>
    page.locator(`.card-layer > [data-slot="${slot}"]`);
const field = (view) =>
    view.objects.filter(({ zone }) => zone === "battlefield");
export const sheet = (page) =>
    page.locator('[data-presentation="sheet"]').getByRole("menu");
export const row = (page, name) =>
    sheet(page).getByRole("menuitem", { name, exact: true });

export async function pressTouch(target) {
    const point = center(await target.boundingBox());
    const touch = {
        pointerId: 100,
        pointerType: "touch",
        isPrimary: true,
        button: 0,
        clientX: point.x,
        clientY: point.y,
    };
    await target.dispatchEvent("pointerdown", { ...touch, buttons: 1 });
    return touch;
}

export async function hold(page, target) {
    const touch = await pressTouch(target);
    await page.clock.runFor(500);
    await target.dispatchEvent("pointerup", touch);
    await sheet(page).waitFor();
}

// The generic audits check the open sheet. Only the gesture and host action
// assertions can tell whether holding and choosing Tap reach the same card.
await check(
    "manual table touch opens the card sheet and taps a permanent",
    async (page) => {
        await startTable(page, deck, [moveLand]);
        const [land] = field(await tableView());
        await hold(page, card(page, land.slot));
        await sheet(page).locator(".table-menu-preview .card-full").waitFor();
        assert.deepEqual(await clippedText(page), []);
        assert.deepEqual(await screenFaults(page), []);
        assert.deepEqual(await crowdedControls(page), []);
        assert.deepEqual(await stageFaults(page), []);
        assert.deepEqual(await phoneFaults(page), []);
        const view = await clickTable(page, row(page, "Tap"), { tap: true });
        assert.equal(field(view)[0].tapped, true);

        const library = page.locator(
            'button[data-pile="library"][data-side="you"]',
        );
        await hold(page, library);
        await row(page, "Bottom card").waitFor();
        await row(page, "Bottom of library").tap();
        await row(page, "Back").tap();
        await row(page, "Bottom card").waitFor();
        await page.keyboard.press("Escape");

        await hold(page, card(page, view.table.orders.you.hand.at(-1)));
        await row(page, "Play face down").waitFor();
    },
    phone,
);

// Only the live host result can show whether the touch selection sends both
// selected references together and doesn't apply a single-card action to either tap.
await check(
    "manual table touch selects and taps two lands",
    async (page) => {
        await startTable(page, deck, [
            moveLand,
            moveLand,
            // A counter keeps the identical lands in separate stacks.
            (view) => ({
                counter: {
                    name: "+1/+1",
                    add: 1,
                    ...tableActionRef(view, {
                        kind: "object",
                        slot: field(view)[1].slot,
                    }),
                },
            }),
        ]);
        const before = await tableView();
        const [first, second] = field(before);
        await hold(page, card(page, first.slot));
        await row(page, "Select").tap();

        const bar = page.locator(".table-selection-bar");
        const count = bar.getByLabel("Selected cards");
        await count.waitFor();
        assert.equal(await count.innerText(), "0 cards");
        for (const original of [first, second]) {
            await card(page, original.slot).tap();
            await card(page, original.slot)
                .locator(".manual-selection-mark")
                .waitFor();
        }
        assert.equal(await count.innerText(), "2 cards");
        await card(page, first.slot).tap();
        assert.equal(await count.innerText(), "1 card");
        await card(page, first.slot).tap();
        assert.equal(await count.innerText(), "2 cards");
        assert.deepEqual(
            field(await tableView()).map(({ tapped }) => tapped),
            [false, false],
        );

        await bar.getByRole("button", { name: "Actions" }).tap();
        const after = await clickTable(page, row(page, "Tap"), { tap: true });
        assert.deepEqual(
            field(after).map(({ tapped }) => tapped),
            [true, true],
        );
        assert.equal(after.table.log.length, before.table.log.length + 1);
        await bar.getByRole("button", { name: "Done" }).tap();
        await bar.waitFor({ state: "hidden" });
        assert.equal(await page.locator(".manual-selection-mark").count(), 0);
    },
    phone,
);

// The audits cover both the strip and sheet. The host result verifies each
// touch dispatches the right adjustment.
await check(
    "manual table touch adjusts mana and counters",
    async (page) => {
        await startTable(page, deck, [
            moveLand,
            (view) => ({
                move: { ...moveLand(view).move, to: { graveyard: 0 } },
            }),
            { player_counter: { seat: 0, name: "W", set: 2 } },
            { player_counter: { seat: 0, name: "energy", set: 1 } },
        ]);
        const mana = page.getByRole("button", { name: /^Your mana:/ });
        const panel = page.getByRole("dialog", {
            name: "Your mana and counters",
        });
        const counter = (name) => panel.locator(`[data-counter="${name}"]`);
        const remove = (name) =>
            panel.getByRole("button", { name: `Remove ${name}`, exact: true });

        for (const viewport of [phone.viewport, { width: 375, height: 647 }]) {
            await page.setViewportSize(viewport);
            await page.clock.runFor(500);
            assert.deepEqual(await phoneFaults(page), []);
            assert.deepEqual(await crowdedControls(page), []);
            await mana.tap();
            assert.deepEqual(await phoneFaults(page), []);
            assert.deepEqual(await crowdedControls(page), []);
            assert.deepEqual(await clippedText(page), []);
            assert.deepEqual(await screenFaults(page), []);

            let view = await clickTable(page, counter("W"), { tap: true });
            assert.equal(view.players.you.mana.white, 3);
            assert.equal(await mana.getAttribute("aria-label"), "Your mana: 3");
            view = await clickTable(page, remove("W"), { tap: true });
            assert.equal(view.players.you.mana.white, 2);

            view = await clickTable(page, counter("energy"), { tap: true });
            assert.equal(view.players.you.namedCounters[0].count, 2);
            view = await clickTable(page, remove("energy"), { tap: true });
            assert.equal(view.players.you.namedCounters[0].count, 1);
            await panel.getByRole("button", { name: "Done" }).tap();
            await panel.waitFor({ state: "hidden" });
            assert.equal(await mana.getAttribute("aria-label"), "Your mana: 2");
        }
    },
    phone,
);
