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
        viewport: { width: 1536, height: 1024 },
    },
);
