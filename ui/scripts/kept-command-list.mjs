import assert from "node:assert/strict";
import { check } from "./kept-harness.mjs";

// The screen audits cannot tell which command a key ran or opened for binding.
await check(
    "command list keyboard navigation runs, binds and closes",
    async (page) => {
        const open = page.getByRole("button", { name: "Empty query" });
        const search = page.getByRole("combobox", { name: "Search commands" });
        const selected = page.getByRole("option", { selected: true });
        const result = page.locator("output");

        await open.click();
        assert.equal(
            await search.evaluate((el) => el === document.activeElement),
            true,
        );
        await page.keyboard.type("top");
        assert.equal(await page.getByRole("group").count(), 2);
        assert.match(await selected.innerText(), /^Look at top cards…/);
        assert.equal(await selected.locator("kbd").innerText(), "⌘W");
        await page.keyboard.press("ArrowDown");
        assert.equal(await selected.innerText(), "Move to top of library");
        await page.keyboard.press("ArrowUp");
        await page.keyboard.press("Alt+Enter");
        assert.equal(await result.innerText(), "Bind Look at top cards…");

        await open.click();
        await page.keyboard.press("Enter");
        assert.equal(await result.innerText(), "Run Draw cards…");

        await open.focus();
        await page.keyboard.press("Enter");
        await search.fill("untap");
        await page.getByText("No matching commands").waitFor();
        await page.keyboard.press("Enter");
        assert.equal(await search.isVisible(), true);
        await page.keyboard.press("Escape");
        await search.waitFor({ state: "hidden" });
        assert.equal(
            await open.evaluate((el) => el === document.activeElement),
            true,
        );
    },
    {
        hash: "#/gallery",
        ready: ".gallery",
        viewport: { width: 1024, height: 600 },
    },
);

// The gallery audits cannot tell where focus goes when an overlay closes.
await check(
    "manual table overlays return keyboard focus after closing",
    async (page) => {
        const corner = page.getByRole("button", {
            name: "Bottom-right corner",
        });
        await corner.focus();
        await page.keyboard.press("Enter");
        await page
            .getByRole("menuitem", { name: "Draw", exact: true })
            .waitFor();
        await page.keyboard.press("Escape");
        assert.equal(
            await corner.evaluate((el) => el === document.activeElement),
            true,
        );

        const field = page.getByRole("textbox", { name: "Cards to draw" });
        await page
            .getByRole("button", { name: "Cancel", exact: true })
            .first()
            .click();
        const open = page.getByRole("button", {
            name: "Draw cards",
            exact: true,
        });
        await open.focus();
        await page.keyboard.press("Enter");
        assert.equal(
            await field.evaluate((el) => el === document.activeElement),
            true,
        );
        await page.keyboard.press("Escape");
        await field.waitFor({ state: "hidden" });
        assert.equal(
            await open.evaluate((el) => el === document.activeElement),
            true,
        );

        await page.keyboard.press("Enter");
        await page.keyboard.type("3");
        await page
            .getByRole("button", { name: "Draw 3", exact: true })
            .waitFor();
        await page.keyboard.press("Enter");
        await field.waitFor({ state: "hidden" });
        assert.equal(
            await open.evaluate((el) => el === document.activeElement),
            true,
        );
    },
    {
        hash: "#/gallery",
        ready: ".gallery",
        viewport: { width: 1024, height: 600 },
    },
);
