import assert from "node:assert/strict";
import { check } from "./kept-harness.mjs";
import { tableRoute } from "./table.mjs";

const reportPath = "/invoke/report_bug";

// The audits cannot compare the viewport sent to Rust with the one at the start of selection.
await check(
    "a bug report uses the viewport from before the keyboard opened",
    async (page) => {
        await page.route(
            (url) => url.pathname === reportPath,
            (route) =>
                route.fulfill({
                    headers: { "Access-Control-Allow-Origin": "*" },
                    json: { ok: { id: "test", path: "" } },
                }),
        );
        await page.evaluate(() =>
            window.dispatchEvent(new Event("beryl:report-bug")),
        );
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
        const [request] = await Promise.all([
            page.waitForRequest(
                (request) => new URL(request.url()).pathname === reportPath,
            ),
            page.getByRole("button", { name: "Submit", exact: true }).click(),
        ]);
        const { page: viewport, region } = request.postDataJSON().request;
        assert.deepEqual([viewport.width, viewport.height], before);
        assert.deepEqual(region, { x: 80, y: 190, width: 140, height: 130 });
    },
    {
        viewport: { width: 402, height: 874 },
        phone: true,
        route: (page) => tableRoute(page, true),
        ready: "body.phone",
    },
);
