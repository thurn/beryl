// Checks for phone behavior that needs an event or gesture to verify.
import assert from "node:assert/strict";
import { check } from "./kept-harness.mjs";

// Why no audit: Android's Back is a native event; a screen audit cannot check which overlay
// receives it or whether it changes a game with no overlay open.
await check(
    "Android Back closes phone overlays without leaving a game",
    async (page) => {
        await page.clock.runFor(2000);
        const back = () =>
            page.evaluate(() =>
                document.body.dispatchEvent(
                    new CustomEvent("beryl:android-back", { bubbles: true }),
                ),
            );
        await page.locator(".phone-menu").click();
        await page.getByRole("dialog", { name: "Menu" }).waitFor();
        await back();
        assert.equal(
            await page.getByRole("dialog", { name: "Menu" }).count(),
            0,
        );
        await page.locator(".phone-menu").click();
        await page.getByRole("button", { name: "Log", exact: true }).click();
        await page.getByRole("dialog", { name: "Game log" }).waitFor();
        await back();
        assert.equal(
            await page.getByRole("dialog", { name: "Game log" }).count(),
            0,
        );
        await back();
        assert.equal(new URL(page.url()).hash, "#/game");
    },
    { phone: true, viewport: { width: 393, height: 759 } },
);

// Why no audit: a native keyboard inset arrives without resizing Android's visual viewport;
// the focused field must move above that inset and return to its original place afterward.
await check(
    "the phone seed stays above the native keyboard",
    async (page) => {
        const seed = page.locator(".start-seed input");
        const before = await seed.boundingBox();
        assert.equal(await seed.getAttribute("inputmode"), "numeric");
        const inset = 250;
        await page.evaluate(
            (height) =>
                window.dispatchEvent(
                    new CustomEvent("beryl:keyboard-inset", { detail: height }),
                ),
            inset,
        );
        await page.waitForFunction(
            () =>
                parseFloat(
                    getComputedStyle(
                        document.querySelector(".phone-start-screen"),
                    ).paddingBottom,
                ) > 250,
        );
        const raised = await seed.boundingBox();
        assert.ok(
            raised.y + raised.height <= 759 - inset,
            "Seed is above the keyboard",
        );
        await page.evaluate(() =>
            window.dispatchEvent(
                new CustomEvent("beryl:keyboard-inset", { detail: 0 }),
            ),
        );
        await page.waitForFunction(
            () =>
                parseFloat(
                    getComputedStyle(
                        document.querySelector(".phone-start-screen"),
                    ).paddingBottom,
                ) < 100,
        );
        const after = await seed.boundingBox();
        assert.ok(
            Math.abs(after.y - before.y) <= 1,
            "the field returns to its place",
        );
    },
    {
        at: 0,
        hash: "#/",
        phone: true,
        ready: ".start-seed input",
        viewport: { width: 393, height: 759 },
    },
);
