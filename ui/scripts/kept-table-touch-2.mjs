import assert from "node:assert/strict";
import { screenFaults } from "./capture.mjs";
import { check } from "./kept-harness.mjs";
import {
    card,
    deck,
    hold,
    moveLand,
    phone,
    pressTouch,
    row,
} from "./kept-table-touch.mjs";
import {
    clickTable,
    clickTableResult,
    pair,
    showTableView,
    startTable,
    tableCommand,
    tableRoute,
    tableView,
} from "./table.mjs";

const button = (page, name) => page.getByRole("button", { name, exact: true });

await check(
    "manual table touch drags to the bottom of the library",
    async (page) => {
        await startTable(page, deck);
        const before = await tableView();
        const source = card(page, before.table.orders.you.hand.at(-1));
        const library = page.locator(
            'button[data-pile="library"][data-side="you"]',
        );
        const box = await library.boundingBox();
        assert.ok(box);
        const touch = await pressTouch(source);
        await page.clock.runFor(160);
        const drop = {
            ...touch,
            clientX: box.x + box.width / 2,
            clientY: box.y + box.height * 0.75,
        };
        await source.dispatchEvent("pointermove", { ...drop, buttons: 1 });
        const selected = page.locator("[data-table-drop=library] [data-over]");
        assert.equal(await selected.getAttribute("data-position"), "bottom");
        const [after] = await Promise.all([
            clickTableResult(page),
            source.dispatchEvent("pointerup", drop),
        ]);
        assert.equal(
            after.table.orders.you.hand.length,
            before.table.orders.you.hand.length - 1,
        );
        assert.deepEqual(
            after.players.you.library.slice(1),
            before.players.you.library,
        );
    },
    phone,
);

await check(
    "manual table touch draws an arrow with the card sheet",
    async (page) => {
        await startTable(page, deck, [moveLand]);
        const [land] = (await tableView()).battlefield;
        await hold(page, card(page, land));
        await row(page, "Draw arrow…").tap();
        const after = await clickTable(
            page,
            page.locator('[data-life="you"]'),
            { tap: true },
        );
        assert.deepEqual(after.table.arrows, [
            {
                arrow: "friendly-target",
                from: { kind: "object", slot: land },
                to: { kind: "player", player: "you" },
                own: true,
            },
        ]);
        assert.ok(
            await page
                .locator(`[data-line="friendly-target ${land} you"]`)
                .getAttribute("d"),
        );
    },
    phone,
);

await check(
    "manual table touch runs a command from the full-screen list",
    async (page) => {
        await startTable(page, deck, [moveLand]);
        const before = await tableView();
        const open = button(page, "Search commands");
        const list = page.getByRole("dialog", { name: "Command list" });
        await open.tap();
        const bounds = await list.boundingBox();
        assert.deepEqual(bounds, { x: 0, y: 0, ...page.viewportSize() });
        assert.equal(await list.locator("kbd").count(), 0);
        await button(list, "Close").tap();
        await list.waitFor({ state: "detached" });

        await open.tap();
        await list.getByRole("combobox").fill("Draw card");
        const after = await clickTable(
            page,
            list.getByRole("option", { name: /^Draw card Your library$/i }),
            { tap: true },
        );
        assert.equal(
            after.table.orders.you.hand.length,
            before.table.orders.you.hand.length + 1,
        );

        await open.tap();
        await list.getByRole("combobox").fill("Draw cards");
        await list.getByRole("option", { name: /^Draw cards…/ }).tap();
        const number = page.locator(".phone-table-ask").getByRole("textbox");
        const field = await number.boundingBox();
        assert.ok(field);
        assert.ok(field.y + field.height < bounds.height / 2);
        await number.fill("2");
        const drewTwo = await clickTable(page, button(page, "Draw 2"), {
            tap: true,
        });
        assert.equal(
            drewTwo.table.orders.you.hand.length,
            after.table.orders.you.hand.length + 2,
        );

        const [land] = drewTwo.battlefield;
        await hold(page, card(page, land));
        await row(page, "Note…").tap();
        const note = page.locator(".phone-table-ask").getByRole("textbox");
        assert.ok((await note.boundingBox()).y < bounds.height / 2);
        await note.fill("Remember");
        const annotated = await clickTable(page, button(page, "Set note"), {
            tap: true,
        });
        assert.equal(
            annotated.objects.find(({ slot }) => slot === land).note,
            "Remember",
        );
    },
    phone,
);

await check(
    "manual table touch joins a desktop host from the phone lobby",
    async (page) => {
        const guestViewport = { width: 375, height: 647 };
        const { host, guest } = await pair(page, "local", {
            fromStart: true,
            guestViewport,
        });
        const lobby = (seat) => seat.getByRole("main", { name: "Table lobby" });
        try {
            await button(host, "Host a table").click();
            await host.getByLabel("Name", { exact: true }).fill("Host");
            await button(host, "Host").click();
            const invite = await lobby(host)
                .getByLabel("Invite code")
                .inputValue();
            assert.ok(invite);

            const choices = guest
                .getByRole("navigation", { name: "Manual tables" })
                .getByRole("button");
            assert.deepEqual(await choices.allTextContents(), [
                "Join a table",
                "Solo table",
            ]);
            await button(guest, "Join a table").tap();
            await guest.getByLabel("Name", { exact: true }).fill("Guest");
            await guest.getByLabel("Invite code").fill(invite);
            await button(guest, "Join").tap();
            for (const seat of [host, guest])
                await lobby(seat)
                    .getByText(/^Direct/)
                    .waitFor();

            await button(lobby(guest), "Change").tap();
            await button(lobby(guest), "Paste decklist").tap();
            assert.equal(await button(lobby(guest), "Open file").count(), 0);
            await lobby(guest).getByLabel("Deck name").fill("Guest deck");
            await lobby(guest).getByLabel("Decklist").fill(deck);
            await button(lobby(guest), "Import").tap();
            await button(lobby(guest), "Ready").tap();
            await lobby(guest)
                .getByRole("button", { name: "Ready", pressed: true })
                .waitFor();
            await lobby(host).getByText("24 main · 0 sideboard").waitFor();
            assert.deepEqual(await screenFaults(guest), []);

            await tableCommand(
                "table_lobby",
                {
                    request: {
                        change: {
                            kind: "load_deck",
                            main: [["Forest", 24]],
                            side: [],
                        },
                    },
                },
                "host",
            );
            assert.deepEqual(
                await tableCommand(
                    "table_act",
                    { request: { action: { start: { first: 0 } } } },
                    "host",
                ),
                { ok: true },
            );
            await guest.locator(".phone-board[data-manual-table]").waitFor();
            const view = await tableView("guest");
            await showTableView(guest, view);
            const hand = card(guest, view.table.orders.you.hand.at(-1));
            const handBounds = await hand.boundingBox();
            assert.ok(handBounds);
            await tableCommand("test_cut_table_link", {}, "host");
            const banner = guest
                .getByRole("status")
                .getByText("Host is reconnecting");
            await banner.waitFor();
            const notice = await banner.boundingBox();
            assert.ok(notice && notice.y + notice.height < handBounds.y);
        } finally {
            await Promise.all([
                tableCommand("table_leave", {}, "host"),
                tableCommand("table_leave", {}, "guest"),
            ]);
            await guest.close();
        }
    },
    {
        route: (page) => tableRoute(page, false, "host"),
        ready: ".start-panel",
        phone: true,
        pages: 2,
        serial: true,
    },
);
