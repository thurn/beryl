import assert from "node:assert/strict";
import { check } from "./kept-harness.mjs";

// The audits cannot compare the viewport sent to Rust with the one at the start of selection.
await check(
    "a bug report uses the viewport from before the keyboard opened",
    async (page) => {
        await page
            .getByRole("button", { name: "Report bug", exact: true })
            .click();
        const surface = page.locator(".bug-surface");
        await surface.waitFor();
        const before = await page.evaluate(() => [innerWidth, innerHeight]);
        await page.mouse.move(80, 190);
        await page.mouse.down();
        await page.mouse.move(220, 320);
        await page.mouse.up();
        await page.setViewportSize({ width: 368, height: 450 });
        await page
            .getByPlaceholder("What went wrong?")
            .fill("A cropped report");
        await page.getByRole("button", { name: "Submit", exact: true }).click();
        const report = await page.waitForFunction(() => window.savedReport);
        const { page: viewport, region } = await report.jsonValue();
        assert.deepEqual([viewport.width, viewport.height], before);
        assert.deepEqual(region, { x: 80, y: 190, width: 140, height: 130 });
    },
    {
        viewport: { width: 402, height: 874 },
        phone: true,
        hash: "#/",
        ready: ".bug-button",
        init: () => {
            history.replaceState(null, "", "?live=/report-test&phone#/");
            window.EventSource = class {
                static OPEN = 1;
                readyState = 1;
                addEventListener() {}
                removeEventListener() {}
            };
            const fetchOriginal = window.fetch.bind(window);
            window.fetch = async (url, options) => {
                if (!String(url).startsWith("/report-test/invoke/")) {
                    return fetchOriginal(url, options);
                }
                const command = String(url).split("/").at(-1);
                if (command === "report_bug") {
                    window.savedReport = JSON.parse(options.body).request;
                }
                const responses = {
                    app_info: {
                        version: "test",
                        smoke: false,
                        platform: "linux",
                        simulator: false,
                    },
                    list_agents: [],
                };
                return Response.json({ ok: responses[command] ?? null });
            };
        },
    },
);
