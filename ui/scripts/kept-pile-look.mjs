// The kept check of how a pile looks (kept.mjs): which way its cards' edges show, where its top
// face ends, and which of its cards casts a shadow.
import assert from "node:assert/strict";
import { check, load } from "./kept-harness.mjs";

const pilesCrowded = await load("piles-crowded");

// Why no audit: where a pile's layers lie against its top card is the pile's own look, which no
// generic check knows. As on Arena (game/pileLook.ts), your piles show the right edges of their
// cards, level with the top card, and the opponent's show their near edges below it; the top
// faces of a side's piles share their bottom edge, however thick each is (by-jcju); and only
// the deepest card drawn casts a shadow, so none darkens the edges beneath it (by-wu0c).
await check(
    "a pile's edges show toward the table's middle, under faces in line",
    async (page) => {
        await page.clock.runFor(2000);
        const layers = await page.evaluate(() =>
            [
                ...document.querySelectorAll(
                    ".card-layer > .card-el:is([data-zone=library], [data-zone=graveyard], [data-zone=exile])",
                ),
            ]
                .filter((e) => getComputedStyle(e).opacity !== "0")
                .map((e) => {
                    const r = e.getBoundingClientRect();
                    const back = e.querySelector(
                        ".card-back, .card, .pile-card-rim",
                    );
                    return {
                        side: e.classList.contains("card-el-you")
                            ? "you"
                            : "opponent",
                        zone: e.dataset.zone,
                        z: Number(getComputedStyle(e).zIndex),
                        x: r.x,
                        y: r.y,
                        bottom: r.bottom,
                        stacked: e.classList.contains("card-el-stacked"),
                        shadow: back
                            ? getComputedStyle(back).boxShadow
                            : "none",
                    };
                }),
        );
        for (const side of ["you", "opponent"]) {
            const bottoms = [];
            for (const zone of ["library", "graveyard", "exile"]) {
                const pile = layers
                    .filter((l) => l.side === side && l.zone === zone)
                    .sort((a, b) => b.z - a.z);
                assert.ok(pile.length >= 4, `${side}: the ${zone} is thick`);
                const [top, deepest] = [pile[0], pile.at(-1)];
                bottoms.push(top.bottom);
                if (side === "you") {
                    assert.ok(
                        deepest.x - top.x >= 3 &&
                            Math.abs(deepest.y - top.y) <= 1,
                        `your ${zone}'s deepest card lies level with its top and right of it, by ${(deepest.x - top.x).toFixed(1)} × ${(deepest.y - top.y).toFixed(1)} px`,
                    );
                } else {
                    assert.ok(
                        deepest.y - top.y >= 2 && deepest.x < top.x,
                        `the opponent's ${zone}'s deepest card lies below its top and left of it, by ${(deepest.x - top.x).toFixed(1)} × ${(deepest.y - top.y).toFixed(1)} px`,
                    );
                }
                for (const layer of pile.slice(0, -1)) {
                    assert.ok(
                        layer.stacked,
                        `${side}: a ${zone} card above the deepest is marked stacked`,
                    );
                    assert.equal(
                        layer.shadow,
                        "none",
                        `${side}: a ${zone} card above the deepest casts a shadow`,
                    );
                }
                assert.notEqual(
                    deepest.shadow,
                    "none",
                    `${side}: the ${zone}'s deepest card casts the pile's shadow`,
                );
            }
            assert.ok(
                Math.max(...bottoms) - Math.min(...bottoms) <= 1,
                `${side}: the piles' top faces end at ${bottoms.map((b) => b.toFixed(1)).join(", ")}`,
            );
        }
    },
    {
        fixture: "piles-crowded",
        at: pilesCrowded.length,
        ready: "[data-pile=exile][data-side=opponent]",
        viewport: { width: 1600, height: 900 },
    },
);
