// The generic tray checks cover its layout. This check compares the linked
// views and scripts to verify the other seat sees the motion without names.
import assert from "node:assert/strict";
import { check } from "./kept-harness.mjs";
import {
    clickAnswer,
    clickTable,
    pair,
    showTableView,
    tableActionRef,
    tableCommand,
    tableRoute,
    tableView,
} from "./table.mjs";

const button = (page, name) => page.getByRole("button", { name, exact: true });
const library = (page) =>
    page.locator('button[data-pile="library"][data-side="you"]');
const menu = (page, name) => page.getByRole("menuitem", { name, exact: true });
const motions = (view) => view.script.beats.flat().map(({ act }) => act);

async function act(seat, action) {
    assert.deepEqual(
        await tableCommand("table_act", { request: { action } }, seat),
        { ok: true },
    );
}

async function enable(page) {
    await button(page, "Settings").click();
    await button(page, "Assists").click();
    const dialog = page.getByRole("dialog", { name: "Assists" });
    await clickTable(
        page,
        dialog.getByRole("switch", { name: "Library flows" }),
    );
    await button(dialog, "Close").click();
}

async function observe(hostView, guest) {
    await showTableView(guest, hostView);
    const seen = await tableView("guest");
    assert.ok(!JSON.stringify(seen).includes("Island"));
    return seen;
}

await check(
    "manual table library flows: scry keeps lifted cards private",
    async (page) => {
        const { host, guest } = await pair(page, "memory");
        try {
            for (const [player, seat, card] of [
                ["host", 0, "Island"],
                ["guest", 1, "Preordain"],
            ]) {
                await act(player, {
                    load_deck: { seat, main: [[card, 24]], side: [] },
                });
            }
            await act("host", { start: { first: 0 } });
            const start = await tableView("host");
            await showTableView(host, start);
            const original = (await observe(start, guest)).players.opponent
                .library;
            await library(host).click({ button: "right" });
            assert.equal(await menu(host, "Scry…").count(), 0);
            assert.equal(await menu(host, "Surveil…").count(), 0);
            await host.keyboard.press("Escape");
            await enable(host);
            await library(host).click({ button: "right" });
            await menu(host, "Scry…").click();
            await host.getByRole("textbox", { name: "Scry" }).fill("2");
            const opened = await clickTable(host, button(host, "Scry 2"));
            const tray = host.getByRole("dialog", {
                name: "Scry",
                exact: true,
            });
            await tray.waitFor();
            const lifted = await observe(opened, guest);
            assert.deepEqual(lifted.players.opponent.library, original);
            assert.deepEqual(lifted.handling, {
                zone: "pile",
                side: "opponent",
                pile: "library",
                count: 2,
                bottom: false,
            });
            assert.ok(
                motions(lifted).some(
                    ({ verb, span }) => verb === "lift" && span === "begin",
                ),
            );
            assert.equal(lifted.table.prompt, undefined);

            const [top] = opened.table.prompt.cards;
            await clickAnswer(
                host,
                tray.locator(`[data-sort-card="${top}"]`),
                "place",
            );
            const done = await clickAnswer(host, button(tray, "Done"), "done");
            await tray.waitFor({ state: "detached" });
            const settled = await observe(done, guest);
            assert.equal(settled.handling, undefined);
            assert.equal(settled.players.opponent.library[0], original.at(-1));
            assert.equal(
                settled.players.opponent.library.at(-1),
                original.at(-2),
            );
            assert.ok(
                motions(settled).some(
                    ({ verb, span }) => verb === "lift" && span === "end",
                ),
            );
            assert.ok(
                motions(settled).some(
                    ({ verb, from, object, at }) =>
                        verb === "fly" &&
                        from === "library" &&
                        object.slot === original.at(-1) &&
                        object.zone === "library" &&
                        at === 0,
                ),
            );
            assert.equal(
                settled.table.log.at(-1).text,
                "Scried 2: 1 on top, 1 on the bottom.",
            );

            // A printed count starts the tray immediately from the stack menu.
            await enable(guest);
            const ready = await tableView("guest");
            await act("guest", {
                move: {
                    to: "stack",
                    ...tableActionRef(ready, {
                        kind: "object",
                        slot: ready.table.orders.you.hand[0],
                    }),
                },
            });
            const cast = await tableView("host");
            await showTableView(guest, cast);
            const stack = guest.locator(
                '.card-layer > [data-zone="stack"][data-slot]',
            );
            await stack.click({ button: "right" });
            const fromCard = await clickTable(guest, menu(guest, "Scry 2"));
            assert.equal(fromCard.table.prompt.cards.length, 2);
            await guest
                .getByRole("dialog", { name: "Scry", exact: true })
                .waitFor();
        } finally {
            await tableCommand("table_leave", {}, "guest");
            await tableCommand("table_leave", {}, "host");
            await guest.close();
        }
    },
    {
        route: (page) => tableRoute(page, false, "host"),
        ready: ".start-panel",
        serial: true,
        pages: 2,
    },
);
