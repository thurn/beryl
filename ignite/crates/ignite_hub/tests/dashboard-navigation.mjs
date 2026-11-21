// Run against an isolated Ignite preview:
// node ignite/crates/ignite_hub/tests/dashboard-navigation.mjs http://127.0.0.1:<port> <project>
// Set PLAYWRIGHT_MODULE to the installed Playwright entry point if needed.
import assert from "node:assert/strict";

const {chromium} = await import(process.env.PLAYWRIGHT_MODULE ?? "playwright");

const [origin, project] = process.argv.slice(2);
assert.match(origin ?? "", /^http:\/\/127\.0\.0\.1:\d+$/);
assert.ok(project, "Pass the preview's project slug");

async function batch(op) {
    const response = await fetch(`${origin}/${project}/api/batch`, {
        method: "POST",
        headers: {
            "Content-Type": "application/json",
            "Tailscale-User-Login": "dashboard-test@example.com",
            "X-Ignite-Dashboard": "1",
        },
        body: JSON.stringify({ops: [op]}),
    });
    assert.equal(response.status, 200, await response.clone().text());
    return (await response.json()).results[0].id;
}

const browser = await chromium.launch({channel: "chrome"});
try {
    for (const width of [375, 1280]) {
        for (const colorScheme of ["light", "dark"]) {
            const id = await batch({
                op: "create",
                fields: {title: "Dashboard navigation fixture", type: "task", status: "in_progress"},
            });
            const firstStart = performance.now();
            const first = await fetch(origin + "/");
            assert.equal(first.status, 200);
            await first.text();
            const cold = performance.now() - firstStart;
            const start = performance.now();
            const staleBoard = await (await fetch(origin + "/")).text();
            console.log(`${width}px ${colorScheme}: home ${cold.toFixed(1)} ms first, ${(performance.now() - start).toFixed(1)} ms warm`);
            assert.match(staleBoard, new RegExp(`href="/${id}"`));

            await batch({op: "close", id, reason: "Navigation fixture"});
            const context = await browser.newContext({
                viewport: {width, height: 720}, colorScheme, reducedMotion: "reduce",
            });
            const page = await context.newPage();
            await page.goto(`${origin}/${id}`);
            assert.match(await page.locator(".c-eyebrow").innerText(), /Closed/);
            await page.waitForFunction(() => navigator.serviceWorker.controller);
            // Put the board viewed before closure back in the service worker's page
            // cache. The detail's feed starts after closure, so it won't replay it.
            await page.evaluate(async (html) => {
                const cache = await caches.open("cirrus-pages-v1");
                await cache.put(location.origin + "/", new Response(html, {
                    headers: {"Content-Type": "text/html; charset=utf-8"},
                }));
            }, staleBoard);
            await page.evaluate((id) => {
                const observer = new MutationObserver((records) => {
                    for (const record of records) {
                        for (const node of record.addedNodes) {
                            if (node.id !== "c-main" || location.pathname !== "/") continue;
                            observer.disconnect();
                            window.firstBoardCard = node.querySelector(`a.c-card[href="/${id}"]`)?.textContent ?? "";
                            return;
                        }
                    }
                });
                observer.observe(document.body, {childList: true, subtree: true});
            }, id);
            // Click without hover or focus prefetch so first navigation reads the seeded cache.
            await page.locator('nav a[href="/"]').first().evaluate((link) => link.click());
            await page.waitForFunction(() => typeof window.firstBoardCard === "string");
            const shown = await page.evaluate(() => window.firstBoardCard);
            assert.doesNotMatch(shown, /In progress/, `${id} reappeared as In progress`);
            assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth));
            await context.close();
        }
    }
} finally {
    await browser.close();
}
