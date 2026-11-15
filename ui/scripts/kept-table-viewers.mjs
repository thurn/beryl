// The screen audits cover an open library. The other checks compare what the
// viewer shows and the host's card order after a gesture.
import assert from "node:assert/strict";
import {
    clippedText,
    crowdedControls,
    screenFaults,
    stageFaults,
} from "./capture.mjs";
import { check, committed } from "./kept-harness.mjs";
import {
    clickTable,
    clickTableResult,
    showTableView,
    startTable,
    tableActionRef,
    tableCommand,
    tableRoute,
    tableView,
} from "./table.mjs";

const live = { route: tableRoute, ready: ".start-panel", serial: true };
const deck = "16 Island\n8 Llanowar Elves\n8 Colossal Dreadmaw";
const named = (view, slot) => view.objects.find((card) => card.slot === slot);
const pile = (page, zone) =>
    page.locator(`button[data-pile="${zone}"][data-side=you]`);
const viewer = (page, name) => page.getByRole("dialog", { name, exact: true });
const cards = (dialog) => dialog.locator("[data-viewer-slot]");
const shownNames = (dialog) =>
    cards(dialog).evaluateAll((nodes) =>
        nodes.map((node) => node.dataset.card),
    );
const withReply = async (page, interaction) => {
    const [view] = await Promise.all([clickTableResult(page), interaction()]);
    return view;
};
const modifier = (page) =>
    page.evaluate(() => (/Mac/.test(navigator.platform) ? "Meta" : "Control"));

async function drag(page, source, destination, y = 0.5, x = 0.5) {
    const from = await source.boundingBox();
    const to = await destination.boundingBox();
    assert.ok(from && to);
    await page.mouse.move(from.x + from.width / 2, from.y + from.height / 4);
    await page.mouse.down();
    await page.mouse.move(to.x + to.width * x, to.y + to.height * y, {
        steps: 4,
    });
    await committed(page);
    return withReply(page, () => page.mouse.up());
}

await check(
    "manual pile viewer filters, groups, sorts and plays from the graveyard",
    async (page) => {
        const names = ["Island", "Llanowar Elves", "Colossal Dreadmaw"];
        await startTable(page, deck, [
            { look: { seat: 0, portion: "all" } },
            { close_look: {} },
            ...names.map((name) => (view) => ({
                move: {
                    to: { graveyard: 0 },
                    ...tableActionRef(view, {
                        kind: "object",
                        slot: view.objects.find((card) => card.name === name)
                            .slot,
                    }),
                },
            })),
        ]);
        await page.keyboard.press("F4");
        const dialog = viewer(page, "Your graveyard");
        await dialog.waitFor();
        const sort = dialog.getByRole("combobox", { name: "Sort" });
        await sort.selectOption("name");
        assert.deepEqual(await shownNames(dialog), [...names].sort());
        await sort.selectOption("mana");
        assert.deepEqual(await shownNames(dialog), names);
        await dialog.getByText("Group by type").click();
        assert.deepEqual(
            await dialog.getByRole("heading", { level: 3 }).allTextContents(),
            ["Creature", "Land"],
        );
        const search = dialog.getByRole("searchbox");
        await search.fill("dReAd");
        assert.deepEqual(await shownNames(dialog), ["Colossal Dreadmaw"]);
        await page.clock.runFor(250);
        await cards(dialog).click({ button: "right" });
        await page.locator(".table-menu[role=menu]").waitFor();
        await page.keyboard.press("Escape");
        assert.equal(await dialog.isVisible(), true);
        const view = await clickTable(page, cards(dialog));
        assert.equal(
            view.objects.find((object) => object.zone === "stack").name,
            "Colossal Dreadmaw",
        );
        await search.focus();
        await page.keyboard.press("Escape");
        await dialog.waitFor({ state: "hidden" });
    },
    live,
);

await check(
    "manual pile viewer looks at top and bottom and drags a card to the bottom",
    async (page) => {
        await startTable(page, deck);
        const mod = await modifier(page);
        const before = await tableView();
        const dialog = viewer(page, "Library");
        await page.keyboard.press(`${mod}+w`);
        await page
            .getByRole("textbox", { name: "Look at top cards" })
            .fill("3");
        const looked = await withReply(page, () =>
            page.keyboard.press("Enter"),
        );
        await dialog.waitFor();
        const [top] = before.players.you.library.slice(-3).reverse();
        assert.deepEqual(looked.handling, {
            zone: "pile",
            side: "you",
            pile: "library",
            count: 3,
            bottom: false,
        });
        assert.equal(await cards(dialog).count(), 3);
        assert.equal(
            await cards(dialog).first().getAttribute("data-viewer-slot"),
            String(top),
        );
        assert.ok((await shownNames(dialog)).every(Boolean));
        const moved = await drag(
            page,
            cards(dialog).first(),
            pile(page, "library"),
            0.8,
        );
        assert.equal(moved.players.you.library[0], top);
        assert.equal(await cards(dialog).count(), 2);
        await withReply(page, () => page.keyboard.press("Escape"));
        await dialog.waitFor({ state: "hidden" });

        await page.keyboard.press(`${mod}+Shift+w`);
        await page
            .getByRole("textbox", { name: "Look at bottom cards" })
            .fill("2");
        const bottom = await withReply(page, () =>
            page.keyboard.press("Enter"),
        );
        await dialog.waitFor();
        assert.equal(bottom.handling.count, 2);
        assert.equal(bottom.handling.bottom, true);
        assert.deepEqual(
            await shownNames(dialog),
            bottom.players.you.library
                .slice(0, 2)
                .reverse()
                .map((slot) => named(bottom, slot).name),
        );
        assert.equal(
            await dialog
                .getByRole("button", { name: "Shuffle", exact: true })
                .count(),
            0,
        );
        const closed = await withReply(page, () =>
            page.keyboard.press("Escape"),
        );
        assert.equal(closed.handling, undefined);
    },
    live,
);

await check(
    "manual pile viewer audits a full library and offers shuffle on close",
    async (page) => {
        await startTable(page, deck);
        const shown = await withReply(page, () => page.keyboard.press("F3"));
        const dialog = viewer(page, "Library");
        await dialog.waitFor();
        await page.clock.runFor(250);
        assert.equal(
            await cards(dialog).count(),
            shown.players.you.library.length,
        );
        assert.deepEqual(await clippedText(page), []);
        assert.deepEqual(await screenFaults(page), []);
        assert.deepEqual(await crowdedControls(page), []);
        assert.deepEqual(await stageFaults(page), []);
        await page.keyboard.press("Escape");
        const shuffle = dialog.getByRole("button", {
            name: "Shuffle",
            exact: true,
        });
        assert.match(await shuffle.getAttribute("class"), /button-primary/);
        assert.equal(
            await dialog
                .getByRole("button", { name: "Don't shuffle" })
                .isVisible(),
            true,
        );
        const shuffled = await clickTable(page, shuffle);
        await dialog.waitFor({ state: "hidden" });
        assert.equal(shuffled.handling, undefined);
        assert.notDeepEqual(
            shuffled.players.you.library,
            shown.players.you.library,
        );
        const reopened = await withReply(page, () => page.keyboard.press("F3"));
        await dialog.getByRole("button", { name: "Close" }).click();
        const kept = await clickTable(
            page,
            dialog.getByRole("button", { name: "Don't shuffle" }),
        );
        assert.deepEqual(
            kept.players.you.library,
            reopened.players.you.library,
        );
        assert.equal(kept.handling, undefined);
    },
    live,
);

await check(
    "manual pile viewer drags to and from the sideboard",
    async (page) => {
        await startTable(page, "24 Island\n\nSideboard\n1 Llanowar Elves");
        const mod = await modifier(page);
        await page.keyboard.press(`${mod}+F3`);
        const sideboard = viewer(page, "Sideboard");
        await sideboard.waitFor();
        const slot = (await tableView()).table.orders.you.sideboard[0];
        const moved = await drag(
            page,
            cards(sideboard),
            pile(page, "library"),
            0.25,
        );
        assert.deepEqual(moved.table.orders.you.sideboard, []);
        assert.equal(moved.players.you.library.at(-1), slot);

        const last = moved.table.orders.you.hand.at(-1);
        const hand = page.locator(`.card-layer > [data-slot="${last}"]`);
        const returned = await drag(page, hand, sideboard);
        assert.deepEqual(returned.table.orders.you.sideboard, [last]);
        await page.keyboard.press("F4");
        const graveyard = viewer(page, "Your graveyard");
        await graveyard.waitFor();
        await page.keyboard.press("Escape");
        await sideboard.waitFor();
    },
    live,
);

await check(
    "manual pile viewer hides the revealed cards with Option+H",
    async (page) => {
        await startTable(page, "24 Llanowar Elves");
        const reply = await tableCommand("table_act", {
            request: { action: { reveal: { in: { hand: 0 }, to: [0] } } },
        });
        assert.deepEqual(reply, { ok: true });
        await showTableView(page, await tableView());
        const open = page.getByRole("button", { name: "Revealed Cards" });
        await open.click();
        const dialog = viewer(page, "Revealed Cards");
        await dialog.waitFor();
        await dialog.getByRole("searchbox").focus();
        await page.keyboard.press("Alt+h");
        await dialog.waitFor({ state: "hidden" });
        await open.click();
        await dialog.getByRole("button", { name: "Hide" }).click();
        await dialog.waitFor({ state: "hidden" });
    },
    live,
);

await check(
    "manual pile viewer opens and plays a card on the phone",
    async (page) => {
        await startTable(page, "24 Llanowar Elves", [
            (view) => ({
                move: {
                    to: { graveyard: 0 },
                    ...tableActionRef(view, {
                        kind: "object",
                        slot: view.table.orders.you.hand.at(-1),
                    }),
                },
            }),
        ]);
        await pile(page, "graveyard").tap();
        const graveyard = viewer(page, "Your graveyard");
        await graveyard.getByRole("searchbox").fill("Elves");
        const view = await clickTable(page, cards(graveyard), { tap: true });
        assert.equal(
            view.objects.find(({ zone }) => zone === "stack").name,
            "Llanowar Elves",
        );
    },
    {
        ...live,
        route: (page) => tableRoute(page, true),
        phone: true,
        viewport: { width: 393, height: 759 },
    },
);
