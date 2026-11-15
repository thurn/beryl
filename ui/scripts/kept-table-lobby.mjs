import assert from "node:assert/strict";
import { crowdedControls, phoneFaults } from "./capture.mjs";
import { check } from "./kept-harness.mjs";
import {
    clickTableResult,
    showTableView,
    tableRoute,
    tableView,
} from "./table.mjs";

// The stage audit has no access to the deck library or a game's starting settings.
await check(
    "manual table lobby imports, swaps, reloads and starts a solo deck",
    async (page) => {
        const lobby = page.getByRole("main", { name: "Table lobby" });
        await page.getByRole("button", { name: "Host a table" }).click();
        await page.getByRole("button", { name: "Host", exact: true }).waitFor();
        await page.getByRole("button", { name: "Cancel" }).click();
        await page.getByRole("button", { name: "Solo table" }).click();
        await lobby.getByText("No deck loaded").waitFor();
        const start = lobby.getByRole("button", { name: "Start", exact: true });
        assert.equal(await start.isEnabled(), false);
        assert.equal(
            await lobby.getByLabel("Starting life").inputValue(),
            "20",
        );
        assert.equal(
            await lobby.getByLabel("Starting hand size").inputValue(),
            "7",
        );
        const mac = process.platform === "darwin";
        await page.keyboard.press(mac ? "Meta+Shift+v" : "Control+Shift+v");
        await lobby.getByLabel("Deck name").fill("Solo check");
        await lobby
            .getByLabel("Decklist")
            .fill("10 Island\n1 Misspelled Islan\n\nSideboard\n2 Plains");
        await lobby.getByRole("button", { name: "Import" }).click();
        await lobby.getByText("Misspelled Islan", { exact: true }).waitFor();
        assert.deepEqual(await crowdedControls(page), []);
        await page.keyboard.press(mac ? "Meta+Alt+u" : "Control+Alt+u");
        await lobby.getByText("No deck loaded").waitFor();
        await lobby.getByRole("button", { name: "Change" }).click();
        await lobby.getByRole("button", { name: "Solo check" }).click();
        await lobby.getByText("11 main · 2 sideboard").waitFor();
        // The view can arrive before the load command returns and enables shortcuts.
        await lobby
            .getByRole("button", { name: "Sideboard", exact: true })
            .click({ trial: true });
        await page.keyboard.press(mac ? "Meta+Shift+b" : "Control+Shift+b");
        await lobby
            .getByRole("button", { name: "Move Plains to main deck" })
            .click();
        await lobby.getByText("12 main · 1 sideboard").waitFor();
        await lobby.getByRole("button", { name: "Done" }).click();
        const upkeep = lobby.getByRole("switch", { name: "Turn upkeep" });
        assert.equal(await upkeep.isChecked(), false);
        await upkeep.click();
        await lobby
            .getByRole("switch", { name: "Turn upkeep", checked: true })
            .waitFor();
        await lobby.getByLabel("Starting life").fill("18");
        await lobby.getByLabel("Starting hand size").fill("3");
        await lobby.getByLabel("First").selectOption("0");
        const save = lobby.getByRole("button", { name: "Save settings" });
        await save.click();
        await start.click();
        await page.locator("[data-manual-table]").waitFor();
        const view = await tableView("host");
        await showTableView(page, view);
        assert.equal(view.players.you.life, 18);
        assert.equal(
            view.objects.filter((card) => card.zone === "hand").length,
            3,
        );
        const [drawn] = await Promise.all([
            clickTableResult(page),
            page.keyboard.press(mac ? "Meta+d" : "Control+d"),
        ]);
        assert.equal(
            drawn.objects.filter((card) => card.zone === "hand").length,
            4,
        );
        await page.keyboard.press("F2");
        await page.getByRole("button", { name: "Confirm Concede" }).click();
        await lobby.getByText("12 main · 1 sideboard").waitFor();
        assert.equal(
            await lobby.getByLabel("Starting life").inputValue(),
            "18",
        );
    },
    {
        route: (page) => tableRoute(page, false, "host"),
        ready: ".start-panel",
        serial: true,
    },
);

await check(
    "manual table lobby fits the phone and offers join and solo",
    async (page) => {
        assert.equal(
            await page.getByRole("button", { name: "Host a table" }).count(),
            0,
        );
        await page.getByRole("button", { name: "Join a table" }).click();
        await page.getByLabel("Invite code").waitFor();
        await page.getByRole("button", { name: "Solo table" }).click();
        await page.getByRole("button", { name: "Change" }).waitFor();
        assert.deepEqual(await phoneFaults(page), []);
        await page.getByRole("button", { name: "Change" }).click();
        assert.equal(
            await page.getByRole("button", { name: "Open file" }).count(),
            0,
        );
        await page.getByRole("button", { name: "Paste decklist" }).waitFor();
    },
    {
        route: (page) => tableRoute(page, true, "guest"),
        ready: ".start-panel",
        serial: true,
        viewport: { width: 375, height: 647 },
        phone: true,
    },
);
