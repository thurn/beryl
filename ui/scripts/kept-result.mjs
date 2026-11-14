// The kept check of the game-over screen's centering (kept.mjs), over the `result-effect`
// fixture that kept-screens.mjs derives.
import assert from "node:assert/strict";
import { check, load } from "./kept-harness.mjs";

const overViews = await load("game-over");
const lastPass = overViews.findLastIndex((v) => v.status.kind === "deciding");
const overAt = { fixture: "game-over", at: lastPass + 1, ready: "[data-life]" };

// Why no audit: where the result's title sits against the window's middle, which no generic
// check measures. The range rect is the glyphs' box; trailing letter-spacing is compensated.
for (const phone of [false, true]) {
    await check(
        `the result's title is centered in the window on ${phone ? "phone" : "desktop"}`,
        async (page) => {
            await page.getByRole("dialog", { name: "Victory" }).waitFor();
            const { title, reason, card, width, height } = await page.evaluate(
                () => {
                    const center = (el) => {
                        const r = el.getBoundingClientRect();
                        return r.left + r.width / 2;
                    };
                    const h = document.querySelector(".game-over-title");
                    const range = document.createRange();
                    range.selectNodeContents(h);
                    const box = range.getBoundingClientRect();
                    const spacing = parseFloat(
                        getComputedStyle(h).letterSpacing,
                    );
                    return {
                        title: box.left + (box.width - spacing) / 2,
                        reason: center(
                            document.querySelector(".game-over-reason"),
                        ),
                        card: center(document.querySelector(".game-over-card")),
                        width: window.innerWidth,
                        height: window.innerHeight,
                    };
                },
            );
            assert.ok(Math.abs(title - width / 2) <= 1, `title ${title}`);
            assert.ok(Math.abs(reason - width / 2) <= 1, `reason ${reason}`);
            assert.ok(Math.abs(card - width / 2) <= 1, `card ${card}`);
            const group = await page.evaluate(() => {
                const top = document
                    .querySelector(".game-over-title")
                    .getBoundingClientRect().top;
                const bottom = document
                    .querySelector(".game-over-card")
                    .getBoundingClientRect().bottom;
                return (top + bottom) / 2;
            });
            assert.ok(Math.abs(group - height / 2) <= 2, `group ${group}`);
        },
        {
            ...overAt,
            fixture: "result-effect",
            at: overViews.length,
            phone,
            viewport: phone ? { width: 393, height: 759 } : undefined,
        },
    );
}
