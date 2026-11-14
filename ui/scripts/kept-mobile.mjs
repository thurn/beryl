// Checks for phone behavior that needs an event or gesture to verify.
import assert from "node:assert/strict";
import { card, check, clicksSent } from "./kept-harness.mjs";

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

// Why no audit: an audit sees a screen, not a gesture. This plays the touches a double tap is
// made of and checks that they open the report without playing or inspecting a card. Playwright has no multi-touch
// for WebKit, so the page gets synthetic pointer events, and pointer capture is stubbed for
// their made-up pointer ids.
await check(
    "a double tap opens the bug report and a multi-finger one plays nothing",
    async (page) => {
        await page.clock.runFor(2000);
        const box = await page.locator(card).boundingBox();
        const onCard = { x: box.x + box.width / 2, y: box.y + box.height / 2 };
        const felt = await page.evaluate(() => {
            for (let y = 100; y < innerHeight; y += 10)
                for (let x = 10; x < innerWidth; x += 10)
                    if (
                        document.elementFromPoint(x, y)?.dataset.felt !==
                        undefined
                    )
                        return { x, y };
            return null;
        });
        assert.ok(felt, "the phone board shows some bare felt");
        /** One tap with a finger at each of `points`, landing and lifting together. */
        const tap = (points) =>
            page.evaluate((points) => {
                const send = (kind, { x, y }, i) =>
                    document.elementFromPoint(x, y).dispatchEvent(
                        new PointerEvent(kind, {
                            pointerId: (window.nextFinger ?? 100) + i,
                            pointerType: "touch",
                            isPrimary: i === 0,
                            clientX: x,
                            clientY: y,
                            bubbles: true,
                            cancelable: true,
                        }),
                    );
                points.forEach((p, i) => send("pointerdown", p, i));
                points.forEach((p, i) => send("pointerup", p, i));
                window.nextFinger = (window.nextFinger ?? 100) + points.length;
            }, points);
        const reports = () => page.evaluate(() => window.reports);
        const twoFingers = [onCard, { x: onCard.x + 20, y: onCard.y }];

        await tap([felt]);
        await page.clock.runFor(500);
        assert.equal(await reports(), 0, "one tap on the felt");
        await tap([felt]);
        await page.clock.runFor(100);
        await tap([felt]);
        assert.equal(await reports(), 1, "a double tap on the felt");
        await page.clock.runFor(500);
        // Each tap is checked on its own: a second tap on a card would close the inspector
        // the first one opened.
        const inspected = () => page.locator(".inspect").count();
        const untouched = async () => {
            await page.clock.runFor(100);
            assert.deepEqual(await clicksSent(page), [], "the card was played");
            assert.equal(await inspected(), 0, "the card was inspected");
        };
        await tap(twoFingers);
        await untouched();
        await tap(twoFingers);
        assert.equal(await reports(), 2, "a two-finger double tap on a card");
        await untouched();
        await page.clock.runFor(500);
        // A plain tap on the card still inspects it.
        await tap([onCard]);
        await page.clock.runFor(500);
        assert.equal(await inspected(), 1, "a one-finger tap inspects");
    },
    {
        phone: true,
        viewport: { width: 393, height: 759 },
        init: () => {
            window.reports = 0;
            window.addEventListener("beryl:report-bug", () => window.reports++);
            for (const name of [
                "setPointerCapture",
                "releasePointerCapture",
                "hasPointerCapture",
            ]) {
                const real = Element.prototype[name];
                Element.prototype[name] = function (id) {
                    return id >= 100 ? false : real.call(this, id);
                };
            }
        },
    },
);
