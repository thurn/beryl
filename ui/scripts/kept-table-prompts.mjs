// The stage audit sees the tray. This check needs the host's receipt to verify
// that editing two cards did not change the library until Done.
import assert from "node:assert/strict";
import { check } from "./kept-harness.mjs";
import {
    showTableView,
    startTable,
    tableCommand,
    tableRoute,
    tableView,
} from "./table.mjs";

await check(
    "manual table sorts two cards in a host-held prompt",
    async (page) => {
        await startTable(page, "18 Island\n18 Plains");
        assert.deepEqual(await tableCommand("test_open_table_sort"), {
            ok: true,
        });
        const opened = await tableView();
        await showTableView(page, opened);
        const original = opened.players.you.library;
        const [first, second] = opened.table.prompt.cards;
        const dialog = page.getByRole("dialog", { name: "Scry" });
        const card = (slot) => dialog.locator(`[data-sort-card="${slot}"]`);
        const click = async (target) => {
            const [response] = await Promise.all([
                page.waitForResponse(
                    (result) =>
                        new URL(result.url()).pathname ===
                        "/invoke/table_answer",
                ),
                target.click(),
            ]);
            assert.deepEqual((await response.json()).ok, { ok: true });
            const view = await tableView();
            await showTableView(page, view);
            return view;
        };
        for (const slot of [first, second]) {
            const editing = await click(card(slot));
            assert.deepEqual(editing.players.you.library, original);
            assert.deepEqual(editing.table.log, opened.table.log);
            assert.ok(editing.table.prompt.placed[0].includes(slot));
            await dialog
                .locator(`[data-sort-card="${slot}"][data-sort-pile="bottom"]`)
                .waitFor();
        }
        const done = await click(dialog.getByRole("button", { name: "Done" }));
        await dialog.waitFor({ state: "detached" });
        assert.equal(done.table.prompt, undefined);
        assert.equal(done.table.log.length, opened.table.log.length + 1);
        assert.equal(done.table.log.at(-1).undoable, true);
        assert.deepEqual(done.players.you.library.slice(0, 2), [second, first]);
    },
    { route: tableRoute, ready: ".start-panel", serial: true },
);
