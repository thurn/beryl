import assert from "node:assert/strict";
import {
    clippedText,
    crowdedControls,
    screenFaults,
    stageFaults,
} from "./capture.mjs";
import { check } from "./kept-harness.mjs";
import {
    clickTableResult,
    startTable,
    tableActionRef,
    tableRoute,
    tableView,
} from "./table.mjs";

const tabletop = { route: tableRoute, ready: ".start-panel", serial: true };
const deck = "24 Llanowar Elves";
const send = (page, key) =>
    Promise.all([clickTableResult(page), page.keyboard.press(key)]).then(
        ([view]) => view,
    );
const textbox = (page, name) =>
    page.getByRole("textbox", { name, exact: true });

async function setup(page, actions = []) {
    await startTable(page, deck, actions);
    const mac = await page.evaluate(() => /Mac/.test(navigator.platform));
    return mac ? "Meta" : "Control";
}

const library = (page) =>
    page.locator('button[data-pile="library"][data-side="you"]');

const handMove = (view) => ({
    move: {
        to: "battlefield",
        tapped: true,
        ...tableActionRef(view, {
            kind: "object",
            slot: view.table.orders.you.hand.at(-1),
        }),
    },
});

await check(
    "manual table keys draw, untap, shuffle and keep the page open",
    async (page) => {
        const mod = await setup(page, [handMove]);
        const before = await tableView();
        const first = await send(page, mod + "+d");
        assert.equal(
            first.table.orders.you.hand.length,
            before.table.orders.you.hand.length + 1,
        );
        await page.keyboard.press(mod + "+e");
        await textbox(page, "Cards to draw").fill("3");
        const three = await send(page, "Enter");
        assert.equal(
            three.table.orders.you.hand.length,
            first.table.orders.you.hand.length + 3,
        );
        const untapped = await send(page, mod + "+u");
        assert.equal(
            untapped.objects.find(({ zone }) => zone === "battlefield").tapped,
            false,
        );
        const shuffled = await send(page, mod + "+s");
        assert.notDeepEqual(
            shuffled.players.you.library,
            untapped.players.you.library,
        );
        await page.keyboard.press(mod + "+w");
        await textbox(page, "Look at top cards").waitFor();
        assert.match(page.url(), /#\/table$/);
    },
    tabletop,
);

await check(
    "manual table command list searches, binds and resets shortcuts",
    async (page) => {
        const mod = await setup(page);
        const search = page.getByRole("combobox", { name: "Search commands" });
        await library(page).focus();
        // With the pointer on the library, the list acts on that pile.
        await library(page).hover();
        await page.keyboard.press(mod + "+k");
        await search.waitFor();
        assert.equal(
            await page.locator(".command-list-acting").innerText(),
            "Library",
        );
        assert.deepEqual(await clippedText(page), []);
        assert.deepEqual(await screenFaults(page), []);
        assert.deepEqual(await crowdedControls(page), []);
        assert.deepEqual(await stageFaults(page), []);

        await page.keyboard.press("Escape");
        assert.ok(
            await library(page).evaluate(
                (element) => element === document.activeElement,
            ),
        );
        await page.keyboard.press(mod + "+k");
        await page.keyboard.type("top");
        await page.getByRole("option", { name: /^Look at top cards…/ }).click();
        await textbox(page, "Look at top cards").fill("3");
        const looked = await send(page, "Enter");
        assert.equal(looked.handling.count, 3);
        await page.mouse.move(10, 10);

        await page.keyboard.press(mod + "+k");
        await search.fill("Shuffle");
        await page.keyboard.press("Alt+Enter");
        await page
            .getByRole("button", { name: "Press a key · Esc cancels" })
            .waitFor();
        await page.keyboard.press(mod + "+d");
        await page.getByText("Draw lost its shortcut.").waitFor();
        assert.match(
            await page
                .getByRole("option", { name: /^Shuffle/ })
                .first()
                .locator("kbd")
                .innerText(),
            /D$/,
        );
        await page.keyboard.press("Escape");
        await page.reload();
        await library(page).waitFor();
        const before = await tableView();
        const after = await send(page, mod + "+d");
        assert.equal(
            after.table.orders.you.hand.length,
            before.table.orders.you.hand.length,
        );
        assert.notDeepEqual(
            after.players.you.library,
            before.players.you.library,
        );

        await page
            .getByRole("button", { name: "Settings", exact: true })
            .click();
        await page
            .getByRole("button", { name: "Shortcuts", exact: true })
            .click();
        await search.fill("Reset shortcuts");
        await page.keyboard.press("Enter");
        await page.getByText("Shortcuts reset.").waitFor();
        await page.keyboard.press("Escape");
        const drawn = await send(page, mod + "+d");
        assert.equal(
            drawn.table.orders.you.hand.length,
            after.table.orders.you.hand.length + 1,
        );
    },
    tabletop,
);

await check(
    "manual table command list can draw while a card is selected",
    async (page) => {
        const mod = await setup(page, [handMove]);
        const before = await tableView();
        const [slot] = before.battlefield;
        const card = page.locator(`.card-layer > [data-slot="${slot}"]`);
        await card.click({ modifiers: [mod] });
        await card.locator(".manual-selection-mark").waitFor();
        await page.keyboard.press(mod + "+k");
        const list = page.getByRole("dialog", { name: "Command list" });
        const search = list.getByRole("combobox");
        await search.fill("draw");
        const draw = list.getByRole("option", { name: /^Draw Your library/ });
        await draw.waitFor();
        const [after] = await Promise.all([
            clickTableResult(page),
            draw.click(),
        ]);
        assert.equal(
            after.table.orders.you.hand.length,
            before.table.orders.you.hand.length + 1,
        );
        await card.locator(".manual-selection-mark").waitFor();

        await page.evaluate(() => {
            window.location.hash = "#/";
        });
        await page.locator(".start-panel").waitFor();
        assert.equal(
            await page.evaluate(
                (mac) =>
                    document.body.dispatchEvent(
                        new KeyboardEvent("keydown", {
                            code: "KeyW",
                            metaKey: mac,
                            ctrlKey: !mac,
                            bubbles: true,
                            cancelable: true,
                        }),
                    ),
                mod === "Meta",
            ),
            true,
        );
    },
    tabletop,
);

await check(
    "manual table stack until sends only the query and hit count",
    async (page) => {
        const mod = await setup(page);
        const before = await tableView();
        assert.ok(
            before.objects
                .filter(({ zone }) => zone === "library")
                .every(({ name }) => !name),
        );
        await page.keyboard.press(mod + "+Shift+y");
        await textbox(page, "Card name or type").fill("creature");
        await page.keyboard.press("Enter");
        await textbox(page, "Hits").fill("2");
        const [request, after] = await Promise.all([
            page.waitForRequest((request) =>
                request.url().endsWith("/invoke/table_act"),
            ),
            send(page, "Enter"),
        ]);
        assert.deepEqual(request.postDataJSON(), {
            request: {
                action: {
                    stack_until: { seat: 0, query: "creature", hits: 2 },
                },
            },
        });
        assert.equal(after.stack.length, 2);
        assert.equal(
            after.players.you.library.length,
            before.players.you.library.length - 2,
        );
        assert.ok(
            after.objects
                .filter(({ zone }) => zone === "library")
                .every(({ name }) => !name),
        );
    },
    tabletop,
);

// Kept checks for selection, arrows, card state, viewers, players and turns
// cover their own keys. This check verifies the other action payloads without
// mutating the table, so each key sees the same references and pile order.
await check(
    "manual table appendix keys send the expected actions",
    async (page) => {
        const mod = await setup(page, [
            handMove,
            handMove,
            (view) => ({
                pin: {
                    ...tableActionRef(view, {
                        kind: "object",
                        slot: view.battlefield[0],
                    }),
                    row: "lands",
                },
            }),
            { draw: { seat: 0, count: 1 } },
        ]);
        const view = await tableView();
        const hand = view.table.orders.you.hand.length;
        const [selected, hovered] = view.battlefield.map((slot) =>
            page.locator(`.card-layer > [data-slot="${slot}"]`),
        );
        const ref = (slot) => tableActionRef(view, { kind: "object", slot });
        const source = ref(view.battlefield[0]);
        const topTwo = view.players.you.library.slice(-2).reverse().map(ref);
        const [top] = topTwo;
        const graveyard = { graveyard: 0 };
        await selected.click({ modifiers: [mod] });
        await hovered.hover();

        await page.route("**/invoke/table_act", (route) =>
            route.fulfill({ json: { ok: { ok: true } } }),
        );
        const cases = [
            [
                mod + "+b",
                { move: { ...source, to: { library: 0 }, position: "bottom" } },
            ],
            [
                mod + (mod === "Meta" ? "+Backspace" : "+Delete"),
                { move: { ...source, to: graveyard } },
            ],
            [mod + "+Shift+h", { sort_hand: { seat: 0, by: "type" } }],
            [mod + "+m", { mulligan: { seat: 0, count: 3 } }, "Mulligan", "3"],
            [mod + "+Shift+m", { mulligan: { seat: 0, count: hand } }],
            [mod + "+Shift+Alt+m", { mulligan: { seat: 0, count: hand - 1 } }],
            [mod + "+Shift+d", { undo: { entry: view.table.lastDraw } }],
            [
                mod + "+Shift+w",
                { look: { seat: 0, portion: { bottom: 3 } } },
                "Look at bottom cards",
                "3",
            ],
            [mod + "+n", { always_reveal_top: { seat: 0, on: true } }],
            [mod + "+Shift+n", { always_look_at_top: { seat: 0, on: true } }],
            [
                mod + "+Shift+e",
                { move: { ...top, to: "battlefield", face: "down" } },
            ],
            ["Alt+y", { move: { ...top, to: graveyard } }],
            [
                "Alt+m",
                {
                    move: {
                        ...top,
                        objects: topTwo.flatMap(({ objects }) => objects),
                        to: graveyard,
                    },
                },
                "Mill cards",
                "2",
            ],
            [mod + "+y", { move: { ...top, to: "stack" } }],
            [mod + "+z", { undo: {} }],
            ["F2", { concede: {} }],
        ];
        for (const [shortcut, action, field, value] of cases) {
            const caught = page.waitForRequest((request) =>
                request.url().endsWith("/invoke/table_act"),
            );
            await page.keyboard.press(shortcut);
            if (shortcut === "F2") {
                await page
                    .getByRole("button", { name: "Confirm Concede" })
                    .click();
            } else if (field) {
                await textbox(page, field).fill(value);
                await page.keyboard.press("Enter");
            }
            assert.deepEqual(
                (await caught).postDataJSON(),
                { request: { action } },
                shortcut,
            );
        }
    },
    tabletop,
);
