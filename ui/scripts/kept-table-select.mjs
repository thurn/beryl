// The generic audits cover the selected board's layout. The interaction checks
// also read the exact selection and live host replies.
import assert from "node:assert/strict";
import { duplicates, record } from "./audit.mjs";
import { crowdedControls, screenFaults, stageFaults } from "./capture.mjs";
import { check, committed } from "./kept-harness.mjs";
import {
    clickTable,
    showTableView,
    startTable,
    tableActionRef,
    tableCommand,
    tableRoute,
    tableView,
} from "./table.mjs";

const tabletop = { route: tableRoute, ready: ".start-panel", serial: true };
const deck = "24 Island";
const ref = (view, slot) => tableActionRef(view, { kind: "object", slot });
const slots = (view, zone) =>
    view.objects.filter((card) => card.zone === zone).map(({ slot }) => slot);
const card = (page, slot) =>
    page.locator(`.card-layer > [data-slot="${slot}"]`);
const move =
    (to, getSlot = (view) => slots(view, "hand")[0]) =>
    (view) => ({
        move: { to, ...ref(view, getSlot(view)) },
    });
const lands = (count) =>
    Array.from({ length: count }, () => move("battlefield"));
const onField = async () => slots(await tableView(), "battlefield");
const modifiers = async (page) =>
    (await page.evaluate(() => /Mac/.test(navigator.platform)))
        ? "Meta"
        : "Control";
const mark = ".manual-selection-mark";

async function marked(page, parent = ".card-layer > [data-slot]") {
    await committed(page);
    return page
        .locator(parent)
        .filter({ has: page.locator(mark) })
        .evaluateAll((nodes) =>
            nodes
                .map(({ dataset }) =>
                    Number(dataset.slot ?? dataset.viewerSlot),
                )
                .sort((a, b) => a - b),
        );
}

async function toggle(page, slot) {
    await card(page, slot).click({ modifiers: [await modifiers(page)] });
}

await check(
    "manual table selection toggles cards and taps mixed permanents together",
    async (page) => {
        await startTable(page, deck, [
            ...lands(2),
            (view) => ({
                tap: {
                    tapped: true,
                    ...ref(view, slots(view, "battlefield")[0]),
                },
            }),
        ]);
        const [a, b] = await onField();
        await toggle(page, a);
        await toggle(page, b);
        assert.deepEqual(await marked(page), [a, b]);
        await toggle(page, a);
        assert.deepEqual(await marked(page), [b]);
        await toggle(page, a);

        const before = await tableView();
        const tapped = await clickTable(page, card(page, a));
        assert.equal(tapped.table.log.length, before.table.log.length + 1);
        assert.ok(
            tapped.objects
                .filter((card) => card.zone === "battlefield")
                .every((card) => card.tapped),
        );
        assert.deepEqual(await marked(page), [a, b]);

        const untapped = await clickTable(page, card(page, a));
        assert.equal(untapped.table.log.length, tapped.table.log.length + 1);
        assert.ok(
            untapped.objects
                .filter((card) => card.zone === "battlefield")
                .every((card) => !card.tapped),
        );
    },
    tabletop,
);

await check(
    "manual table selection drags a rectangle over four lands",
    async (page) => {
        await startTable(page, deck, lands(4));
        const field = await onField();
        const bounds = await Promise.all(
            field.map((slot) => card(page, slot).boundingBox()),
        );
        const left = Math.min(...bounds.map((box) => box.x));
        const right = Math.max(...bounds.map((box) => box.x + box.width));
        const y =
            Math.min(...bounds.map((box) => box.y)) + bounds[0].height / 2;
        await page.mouse.move(right + 10, y - 10);
        await page.mouse.down();
        await page.mouse.move(left + 8, y + 10);
        await committed(page);
        assert.equal(await page.locator(".selection-rectangle").count(), 1);
        await page.mouse.up();
        assert.deepEqual(await marked(page), field);
        assert.equal(await page.locator(".selection-rectangle").count(), 0);
        const tapped = await clickTable(page, card(page, field[0]));
        assert.ok(
            tapped.objects
                .filter((card) => field.includes(card.slot))
                .every((card) => card.tapped),
        );
    },
    tabletop,
);

await check(
    "manual table selection uses select all in the hovered zone",
    async (page) => {
        await startTable(page, deck, lands(2));
        const view = await tableView();
        await card(page, view.table.orders.you.hand.at(-1)).hover();
        await page.keyboard.press(`${await modifiers(page)}+A`);
        assert.deepEqual(await marked(page), slots(view, "hand"));
        await card(page, slots(view, "battlefield")[0]).hover();
        await page.keyboard.press(`${await modifiers(page)}+A`);
        assert.deepEqual(await marked(page), slots(view, "battlefield"));
    },
    tabletop,
);

await check(
    "manual table selection uses select row on the battlefield",
    async (page) => {
        await startTable(page, deck, [
            ...lands(3),
            (view) => ({
                pin: {
                    row: "creatures",
                    ...ref(view, slots(view, "battlefield")[0]),
                },
            }),
        ]);
        const [first, second, third] = await onField();
        await toggle(page, (await tableView()).table.orders.you.hand.at(-1));
        await card(page, second).hover();
        await page.keyboard.press(`${await modifiers(page)}+Shift+X`);
        assert.deepEqual(await marked(page), [second, third]);
        await card(page, first).hover();
        await page.keyboard.press(`${await modifiers(page)}+Shift+X`);
        assert.deepEqual(await marked(page), [first]);
    },
    tabletop,
);

await check(
    "manual table selection uses select column in the viewer",
    async (page) => {
        await startTable(
            page,
            deck,
            Array.from({ length: 7 }, () => move({ graveyard: 0 })),
        );
        const view = await tableView();
        const viewerCards = ".zone-viewer [data-viewer-slot]";
        await page
            .locator('button[data-pile="graveyard"][data-side="you"]')
            .click();
        await page.clock.runFor(250);
        const first = page.locator(viewerCards).first();
        const expected = await first.evaluate((anchor) =>
            [...anchor.parentElement.children]
                .filter((item) => item.offsetLeft === anchor.offsetLeft)
                .map((item) => Number(item.dataset.viewerSlot))
                .sort((a, b) => a - b),
        );
        assert.ok(expected.length > 1);
        await first.hover();
        await page.keyboard.press(`${await modifiers(page)}+Shift+C`);
        assert.deepEqual(await marked(page, viewerCards), expected);
        await page.keyboard.press(`${await modifiers(page)}+A`);
        assert.deepEqual(
            await marked(page, viewerCards),
            slots(view, "graveyard"),
        );
    },
    tabletop,
);

await check(
    "manual table selection escape closes the viewer before clearing cards",
    async (page) => {
        await startTable(page, deck, [...lands(1), move({ graveyard: 0 })]);
        const field = await onField();
        await toggle(page, field[0]);
        await page
            .locator('button[data-pile="graveyard"][data-side="you"]')
            .click();
        const viewer = page.getByRole("dialog", { name: "Your graveyard" });
        await viewer.waitFor();
        await page.keyboard.press("Escape");
        await page.clock.runFor(250);
        assert.equal(await viewer.count(), 0);
        assert.deepEqual(await marked(page), field);
        await page.keyboard.press("Escape");
        assert.deepEqual(await marked(page), []);
    },
    tabletop,
);

await check(
    "manual table selection shows count and total power and toughness",
    async (page) => {
        await startTable(page, "24 Snapcaster Mage", [
            ...lands(2),
            (view) => ({
                stats: {
                    stats: [3, 5],
                    ...ref(view, slots(view, "battlefield")[0]),
                },
            }),
        ]);
        const field = await onField();
        for (const slot of field) await toggle(page, slot);
        await page.mouse.move(0, 0);
        await committed(page);
        assert.equal(
            await page.getByLabel("Selection").innerText(),
            "2 cards\n5/6",
        );
        assert.deepEqual(await screenFaults(page), []);
        assert.deepEqual(await stageFaults(page), []);
        assert.deepEqual(await crowdedControls(page), []);
        await page.evaluate(record);
        const frames = await page.evaluate(() => {
            window.__motion.frame(0);
            window.__motion.unhit();
            return window.__motion.frames;
        });
        assert.deepEqual(duplicates(frames), []);
    },
    tabletop,
);

await check(
    "manual table selection survives changes and drops cards that move zones",
    async (page) => {
        await startTable(page, deck, lands(2));
        const [a, b] = await onField();
        await toggle(page, a);
        await toggle(page, b);
        const before = await tableView();
        const action = move({ hand: 0 }, () => a)(before);
        assert.deepEqual(
            await tableCommand("table_act", { request: { action } }),
            { ok: true },
        );
        await showTableView(page, await tableView());
        assert.deepEqual(await marked(page), [b]);
    },
    tabletop,
);

await check(
    "manual table selection plays selected cards from the hand together",
    async (page) => {
        await startTable(page, deck);
        const view = await tableView();
        const target = view.table.orders.you.hand.at(-1);
        await card(page, target).hover();
        await page.keyboard.press(`${await modifiers(page)}+A`);
        const played = await clickTable(page, card(page, target));
        assert.equal(played.table.log.length, view.table.log.length + 1);
        assert.equal(
            slots(played, "battlefield").length,
            slots(view, "hand").length,
        );
        assert.deepEqual(await marked(page), []);
    },
    tabletop,
);
