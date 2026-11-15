import assert from "node:assert/strict";
import { check } from "./kept-harness.mjs";
import {
    clickAnswer,
    pair,
    showTableView,
    tableCommand,
    tableRoute,
    tableView,
} from "./table.mjs";

const button = (page, name) => page.getByRole("button", { name, exact: true });

function lobby(seat, change) {
    return tableCommand("table_lobby", { request: { change } }, seat);
}

async function showBottomSelection(page, selected) {
    await page.evaluate(
        (count) =>
            new Promise((resolve) => {
                const stage = window.berylStage;
                const show = () => {
                    stage.skip();
                    const prompt = stage.shown.view?.table?.prompt;
                    if (
                        prompt?.kind === "opening" &&
                        prompt.toBottom === 1 &&
                        prompt.bottom.length === count
                    ) {
                        stop();
                        resolve();
                    }
                };
                const stop = stage.subscribe(() => queueMicrotask(show));
                show();
            }),
        selected,
    );
}

await check(
    "manual table guided start rolls, mulligans, and bottoms a card across two seats",
    async (page) => {
        const { host, guest } = await pair(page, "memory", { fromStart: true });
        try {
            const { invite } = await tableCommand(
                "table_host",
                {
                    request: { link: "memory", settings: { name: "Host" } },
                },
                "host",
            );
            await tableCommand(
                "table_join",
                {
                    request: { invite, name: "Guest" },
                },
                "guest",
            );

            for (const [key, tab, name] of [
                ["host", host, "Island"],
                ["guest", guest, "Mountain"],
            ]) {
                await lobby(key, {
                    kind: "load_deck",
                    main: [[name, 18]],
                    side: [],
                });
                await tab.evaluate(() => (window.location.hash = "#/lobby"));
                await tab.getByRole("main", { name: "Table lobby" }).waitFor();
            }
            await button(guest, "Ready").click();
            await button(host, "Start").click();

            await Promise.all(
                [host, guest].map(async (tab) => {
                    await tab.locator("[data-manual-table]").waitFor();
                    await tab.evaluate(() => {
                        // Sample the roll after the banner has entered.
                        const stage = window.berylStage;
                        stage.clock.advance(300);
                    });
                    await tab
                        .getByText("Rolled dice (d6)", { exact: true })
                        .waitFor();
                }),
            );

            const first = await tableView("host");
            const chooser =
                first.table.prompt?.kind === "choose" ? host : guest;
            for (const tab of [host, guest])
                await tab.evaluate(() => window.berylStage.skip());

            await button(
                chooser.getByRole("dialog", { name: "Play or draw?" }),
                "Draw",
            ).click();
            const opening = (tab) =>
                tab.getByRole("dialog", { name: "Opening hand" });
            await Promise.all(
                [host, guest].map((tab) => opening(tab).waitFor()),
            );
            assert.equal(
                await opening(host).locator('[data-card="Mountain"]').count(),
                0,
            );
            assert.equal(
                await opening(guest).locator('[data-card="Island"]').count(),
                0,
            );

            await clickAnswer(host, button(host, "Keep"), "keep");
            await opening(host).waitFor({ state: "hidden" });
            assert.equal((await tableView("host")).turn.number, 0);

            await clickAnswer(guest, button(guest, "Mulligan"), "mulligan");
            const redrawn = guest.getByRole("dialog", {
                name: "Mulligan to 6",
            });
            await redrawn.waitFor();
            await button(redrawn, "Keep").click();
            await showBottomSelection(guest, 0);
            const bottom = guest.getByRole("dialog", {
                name: "Put on the bottom",
            });
            await bottom.locator(".opening-hand button").first().click();
            await showBottomSelection(guest, 1);
            await bottom.locator(".opening-slots button").waitFor();
            await clickAnswer(guest, button(bottom, "Done"), "done");
            await bottom.waitFor({ state: "hidden" });
            for (const [key, tab, cards] of [
                ["host", host, 7],
                ["guest", guest, 6],
            ]) {
                const view = await tableView(key);
                await showTableView(tab, view);
                assert.equal(view.turn.number, 1);
                assert.equal(view.table.phase, "untap");
                assert.equal(view.table.orders.you.hand.length, cards);
            }
            assert.equal(
                (await tableView("host")).turn.active,
                chooser === guest ? "you" : "opponent",
            );
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
