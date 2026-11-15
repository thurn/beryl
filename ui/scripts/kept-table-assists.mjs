// Play the assist controls against the host. The regular screen audits cover the
// pending panel on a full battlefield.
import assert from "node:assert/strict";
import {
    clippedText,
    crowdedControls,
    phoneFaults,
    screenFaults,
    stageFaults,
} from "./capture.mjs";
import { check } from "./kept-harness.mjs";
import {
    clickAnswer,
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
const assist = (assist) => ({ set_assist: { assist, on: true } });
const moveLand = (view, tapped = true) => ({
    move: {
        to: "battlefield",
        tapped,
        ...tableActionRef(view, {
            kind: "object",
            slot: view.table.orders.you.hand[0],
        }),
    },
});

function token(index) {
    return {
        token: {
            card: {
                custom: {
                    name: `Scout ${index + 1}`,
                    types: "Token Creature — Scout",
                    colors: ["G"],
                    stats: [1, 1],
                },
            },
            count: 1,
        },
    };
}

const panel = (page) => page.getByRole("region", { name: "Turn upkeep" });
const mark = (page) => page.locator(".stage-mark-suggested");

async function showProposal(page, revision) {
    await page.evaluate(
        (expected) =>
            new Promise((resolve) => {
                const stage = window.berylStage;
                const show = () => {
                    stage.skip();
                    if (
                        stage.shown.view?.table.proposals[0]?.revision ===
                        expected
                    ) {
                        stop();
                        resolve();
                    }
                };
                const stop = stage.subscribe(() => queueMicrotask(show));
                show();
            }),
        revision,
    );
}

async function audit(page) {
    assert.deepEqual(await screenFaults(page), []);
    assert.deepEqual(await stageFaults(page), []);
    assert.deepEqual(await crowdedControls(page), []);
    assert.deepEqual(await clippedText(page), []);
}

await check(
    "manual table assists keep a crowded board clear and apply only checked upkeep",
    async (page) => {
        await startTable(page, deck, [
            moveLand,
            {
                batch: {
                    actions: Array.from({ length: 16 }, (_, i) => token(i)),
                },
            },
            assist("turn_upkeep"),
            { next_turn: {} },
            // Open the table from a later snapshot whose script begins no new marks.
            assist("turn_upkeep"),
        ]);
        const before = await tableView();
        assert.equal(before.battlefield.length, 17);
        await panel(page).waitFor();
        await mark(page).waitFor();
        await audit(page);
        await page.getByRole("button", { name: "Show log" }).click();
        await page.getByRole("button", { name: "Hide log" }).waitFor();
        await audit(page);
        await page.getByRole("button", { name: "Hide log" }).click();

        const untouched = panel(page).getByRole("checkbox", {
            name: "Untap Island yours",
        });
        assert.equal(await untouched.isChecked(), true);
        const changed = await clickAnswer(page, untouched, "proposalLine");
        assert.deepEqual(changed.table.log, before.table.log);
        await showProposal(page, changed.table.proposals[0].revision);
        await panel(page).locator("input:not(:checked)").waitFor();
        await mark(page).waitFor({ state: "hidden" });

        const after = await clickAnswer(
            page,
            panel(page).getByRole("button", { name: "Apply" }),
            "applyProposal",
        );
        assert.equal(after.table.phase, "firstMain");
        assert.equal(after.table.log.length, before.table.log.length + 1);
        assert.equal(after.table.log.at(-1).undoable, true);
        assert.equal(
            after.objects.find(
                ({ name, zone }) => name === "Island" && zone === "battlefield",
            ).tapped,
            true,
        );
        assert.equal(
            after.table.orders.you.hand.length,
            before.table.orders.you.hand.length + 1,
        );
        await panel(page).waitFor({ state: "hidden" });
    },
    tabletop,
);

await check(
    "manual table assists show a tappable upkeep panel clear of cards on a phone",
    async (page) => {
        await startTable(page, deck, [moveLand, { next_turn: {} }]);
        // Compare the same table before the proposal. The mana controls
        // already fall below the phone audit's minimum size.
        const faultNames = async () =>
            (await phoneFaults(page)).map((fault) =>
                fault.replace(/ is \d+×\d+$/, ""),
            );
        const baseline = await faultNames();
        assert.deepEqual(
            await tableCommand("table_act", {
                request: { action: assist("turn_upkeep") },
            }),
            { ok: true },
        );
        await showTableView(page, await tableView());
        await panel(page).waitFor();
        await audit(page);
        assert.deepEqual(await faultNames(), baseline);
        const after = await clickAnswer(
            page,
            panel(page).getByRole("button", { name: "Apply" }),
            "applyProposal",
            { tap: true },
        );
        assert.equal(after.table.phase, "firstMain");
        assert.ok(
            !after.objects.find(({ zone }) => zone === "battlefield").tapped,
        );
    },
    {
        ...tabletop,
        route: (page) => tableRoute(page, true),
        phone: true,
        viewport: { width: 375, height: 647 },
    },
);

await check(
    "manual table assists offer no contextual rows while off and let a nudge move its card",
    async (page) => {
        await startTable(page, "24 Llanowar Elves", [
            (view) => moveLand(view, false),
            (view) => ({
                damage: {
                    damage: 1,
                    ...tableActionRef(view, {
                        kind: "object",
                        slot: view.battlefield[0],
                    }),
                },
            }),
        ]);
        const before = await tableView();
        const card = page.locator(
            `.card-layer > [data-slot="${before.battlefield[0]}"]`,
        );
        const label = card.getByRole("button", { name: "Move to graveyard" });
        assert.equal(await panel(page).count(), 0);
        assert.equal(await label.count(), 0);
        for (const target of [
            card,
            page.locator('button[data-pile="library"][data-side="you"]'),
        ]) {
            await target.click({ button: "right" });
            const menu = page.getByRole("menu");
            await menu.waitFor();
            assert.equal(
                await menu.getByText("Assists", { exact: true }).count(),
                0,
            );
            await page.keyboard.press("Escape");
            await menu.waitFor({ state: "hidden" });
        }

        // Enable the nudge only after the lethal damage is on the table.
        await page.getByRole("button", { name: "Settings" }).click();
        await page.getByRole("button", { name: "Assists" }).click();
        await clickTable(
            page,
            page.getByRole("switch", { name: "State nudges" }),
        );
        await page
            .getByRole("dialog", { name: "Assists" })
            .getByRole("button", { name: "Close" })
            .click();
        await label.waitFor();
        const after = await clickAnswer(page, label, "applyLabel");
        assert.equal(after.table.log.length, before.table.log.length + 2);
        assert.equal(after.battlefield.length, 0);
        assert.equal(
            after.objects.filter(({ zone }) => zone === "graveyard").length,
            1,
        );
        assert.deepEqual(after.table.labels, []);
    },
    tabletop,
);
