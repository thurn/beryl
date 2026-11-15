import assert from "node:assert/strict";
import {
    clippedText,
    crowdedControls,
    screenFaults,
    stageFaults,
} from "./capture.mjs";
import { center, check } from "./kept-harness.mjs";
import {
    clickTable,
    clickTableResult,
    startTable,
    tableActionRef,
    tableRoute,
    tableView,
} from "./table.mjs";

const tabletop = { route: tableRoute, ready: ".start-panel", serial: true };
const deck = "24 Llanowar Elves";
const moveHand = (to) => (view) => ({
    move: {
        to,
        ...tableActionRef(view, {
            kind: "object",
            slot: view.table.orders.you.hand.at(-1),
        }),
    },
});
const field = (view) =>
    view.objects.filter(({ zone }) => zone === "battlefield");
const permanent = (page, slot) =>
    page.locator(`.card-layer > [data-slot="${slot}"]`);
const row = (page, name) => page.getByRole("menuitem", { name, exact: true });
const library = (page) =>
    page.locator('button[data-pile="library"][data-side="you"]');

// The generic audits see the menu and the prompt. They cannot infer which
// action, arguments or captured object references a row sent to the host.
await check(
    "manual table menu adds a counter to a permanent",
    async (page) => {
        await startTable(page, deck, [moveHand("battlefield")]);
        const [object] = field(await tableView());
        await permanent(page, object.slot).click({ button: "right" });
        const view = await clickTable(page, row(page, "Add +1/+1 counter"));
        assert.deepEqual(field(view)[0].counters, [
            { kind: "plusOne", count: 1 },
        ]);
    },
    tabletop,
);

await check(
    "manual table menu draws with a typed count and restores focus",
    async (page) => {
        await startTable(page, deck);
        const before = await tableView();
        await library(page).focus();
        await library(page).click({ button: "right" });
        await row(page, "Draw cards…").click();
        const field = page.getByRole("textbox", { name: "Cards to draw" });
        assert.equal(await field.inputValue(), "1");
        assert.ok(
            await field.evaluate(
                (node) =>
                    node === document.activeElement &&
                    node.selectionStart === 0 &&
                    node.selectionEnd === 1,
            ),
        );
        await page.keyboard.type("3");
        await page
            .getByRole("button", { name: "Draw 3", exact: true })
            .waitFor();
        const [view] = await Promise.all([
            clickTableResult(page),
            page.keyboard.press("Enter"),
        ]);
        assert.equal(
            view.table.orders.you.hand.length,
            before.table.orders.you.hand.length + 3,
        );
        assert.equal(
            await library(page).evaluate(
                (node) => node === document.activeElement,
            ),
            true,
        );

        await library(page).click({ button: "right" });
        await row(page, "Every action…").click();
        const list = page.getByRole("dialog", { name: "Command list" });
        await list.getByRole("option", { name: /Draw cards…/ }).waitFor();
        assert.equal(
            await list.getByRole("option", { name: "Tap" }).count(),
            0,
        );
        await page.keyboard.press("Escape");
        await list.waitFor({ state: "hidden" });
        assert.equal(
            await library(page).evaluate(
                (node) => node === document.activeElement,
            ),
            true,
        );
    },
    tabletop,
);

await check(
    "manual table menu picks a permanent to attach to",
    async (page) => {
        await startTable(page, deck, [
            moveHand("battlefield"),
            moveHand("battlefield"),
            (view) => ({
                pin: {
                    row: "others",
                    ...tableActionRef(view, {
                        kind: "object",
                        slot: field(view)[1].slot,
                    }),
                },
            }),
        ]);
        const [source, host] = field(await tableView());
        await permanent(page, source.slot).click({ button: "right" });
        await row(page, "Attach to…").click();
        const life = page.locator('[data-life="you"] .life-hit');
        await life.click();
        assert.equal(field(await tableView())[0].attachedTo, null);
        const at = center(await permanent(page, host.slot).boundingBox());
        await page.mouse.move(at.x, at.y);
        assert.equal(await page.locator("[data-aimed]").count(), 1);
        const [view] = await Promise.all([
            clickTableResult(page),
            page.mouse.click(at.x, at.y),
        ]);
        assert.equal(
            field(view).find(({ slot }) => slot === source.slot).attachedTo,
            host.slot,
        );
    },
    tabletop,
);

await check(
    "manual table menu states pass screen and stage audits",
    async (page) => {
        // The solo server provides the board and references. Add the second seat
        // to its initial projection so the same stage can draw both permanent menus.
        await page.route("**/invoke/table_current", async (route) => {
            const response = await route.fetch();
            const result = await response.json();
            const view = result.ok?.view;
            if (view) {
                const [, opposing] = field(view);
                opposing.owner = opposing.controller = "opponent";
                view.players.opponent.life = 20;
                view.table.seats = 2;
                delete view.script;
            }
            await route.fulfill({ response, json: result });
        });
        await startTable(page, deck, [
            moveHand("battlefield"),
            moveHand("battlefield"),
            moveHand("stack"),
        ]);
        const view = await tableView();
        const [yours, theirs] = field(view);
        const life = page.locator('[data-life="you"] .life-hit');
        const stack = view.objects.find(({ zone }) => zone === "stack");
        const hand = view.table.orders.you.hand.at(-1);

        for (const [target, first, options] of [
            [permanent(page, yours.slot), "Tap"],
            [permanent(page, theirs.slot), "Graveyard"],
            [permanent(page, hand), "Play", { position: { x: 30, y: 25 } }],
            [permanent(page, stack.slot), "Resolve"],
            [library(page), "Draw"],
            [life, "−1"],
        ]) {
            await target.click({ button: "right", ...options });
            const menu = page.locator(".table-menu[role=menu]");
            assert.match(
                await menu.getByRole("menuitem").first().innerText(),
                new RegExp(`^${first}`),
            );
            assert.deepEqual(await clippedText(page), [], first);
            assert.deepEqual(await screenFaults(page), [], first);
            assert.deepEqual(await crowdedControls(page), [], first);
            assert.deepEqual(await stageFaults(page), [], first);
            await page.keyboard.press("Escape");
            await menu.waitFor({ state: "hidden" });
        }
    },
    tabletop,
);
