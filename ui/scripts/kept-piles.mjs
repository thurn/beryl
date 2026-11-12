// The kept checks of the piles (kept.mjs): where they lie, what their viewers list and how they
// close, and what a decision offers from them.
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { join } from "node:path";
import { slotOf as offeredSlot, twins } from "../src/game/decisions.ts";
import {
    SAME,
    check,
    chosen,
    derive,
    drawn,
    exiledFrom,
    fixtures,
    load,
    offeredToken,
    pilesAt,
    recorded,
    scry,
    tokenRun,
    tokenView,
    writeFixture,
} from "./kept-harness.mjs";

/** Steps the clock until nothing matches `selector`, failing if it outlasts `limit` ms. A
 * timer set in a React effect starts when the effect flushes, which is real time and not the
 * paused clock's, so a fixed `runFor` past its duration can land before it fires. */
async function untilGone(page, selector, limit, step = 16) {
    for (let ran = 0; ran <= limit; ran += step) {
        if (!(await page.$(selector))) return ran;
        await page.clock.runFor(step);
    }
    throw new Error(`${selector} outlasted ${limit} ms`);
}

// A table with tapped and untapped lands and creatures, and a card Portable Hole exiled.
const beneathViews = await load("exiled-beneath");
const beneathView = beneathViews.at(-1);
const exiled = beneathView.objects.find(
    (o) => o.zone === "exile" && o.exiledBy !== null,
);
const beneathRun = {
    fixture: "exiled-beneath",
    at: beneathViews.length,
    ready: `[data-slot="${exiled.slot}"]`,
};

// Why no audit: where a tucked card peeks out, that its strip takes the pointer, and what hovering
// it shows.
await check(
    "a card exiled by a permanent peeks out from beneath it, previewed on hover",
    async (page) => {
        await page.clock.runFor(2000);
        const under = page.locator(`[data-slot="${exiled.slot}"]`);
        const host = page.locator(`[data-slot="${exiled.exiledBy}"]`);
        const [mine, theirs] = [
            await under.boundingBox(),
            await host.boundingBox(),
        ];
        assert.ok(
            mine.x < theirs.x && mine.y > theirs.y,
            `${exiled.name} peeks out down and to the left of what exiled it`,
        );
        const z = (l) => l.evaluate((e) => Number(getComputedStyle(e).zIndex));
        assert.ok((await z(under)) < (await z(host)), "it lies beneath");
        // Its peeking strip, left of what exiled it.
        const at = { x: (mine.x + theirs.x) / 2, y: mine.y + mine.height / 2 };
        assert.equal(
            await page.evaluate(
                ({ x, y }) =>
                    document.elementFromPoint(x, y)?.closest("[data-slot]")
                        ?.dataset.slot,
                at,
            ),
            String(exiled.slot),
            "its strip shows",
        );
        await page.mouse.move(at.x, at.y);
        await page.waitForSelector(
            `.inspect[aria-label="${exiled.name}, inspected"]`,
        );
    },
    {
        ...beneathRun,
    },
);

const pilesView = scry.views[pilesAt];
const pile = (side, zone) => `[data-pile="${zone}"][data-side="${side}"]`;
const pileRun = {
    fixture: "choice-scry",
    at: pilesAt + 1,
    ready: pile("you", "graveyard"),
};
await derive("choice-scry", "known-library", pilesAt, (v) => ({
    ...v,
    objects: v.objects.map((o) =>
        o.zone === "library"
            ? {
                  ...o,
                  name:
                      o.slot === v.players[o.owner].library.at(-2)
                          ? "Jace, Vryn's Prodigy"
                          : null,
              }
            : o,
    ),
}));
// Why no audit: which cards a viewer lists, of those its player knows.
await check(
    "known library browser never reveals unknown cards",
    async (page) => {
        await page.clock.runFor(2000);
        for (const side of ["you", "opponent"]) {
            await page
                .locator(`button[data-pile="library"][data-side="${side}"]`)
                .click();
            await page.clock.runFor(1000);
            const dialog = page.getByRole("dialog");
            assert.match(await dialog.innerText(), /Known cards/);
            assert.equal(await dialog.locator(".zone-viewer-card").count(), 1);
            assert.match(await dialog.innerText(), /Jace/);
            await dialog
                .getByRole("button", { name: "Close", exact: true })
                .click();
            await page.clock.runFor(1000);
        }
    },
    {
        ...pileRun,
        fixture: "known-library",
        at: 1,
    },
);

const overlaps = (a, b) =>
    a.x < b.x + b.width &&
    b.x < a.x + a.width &&
    a.y < b.y + b.height &&
    b.y < a.y + a.height;

for (const viewport of [
    { width: 1600, height: 900 },
    { width: 1280, height: 800 },
]) {
    // Why no audit: a pile against the hand's fan: cards against cards, which the audit of controls
    // leaves alone.
    await check(
        `your graveyard and exile lie clear of the hand at ${viewport.width}`,
        async (page) => {
            await page.clock.runFor(2000);
            const hand = await page.locator(".card-el-hand").all();
            assert.ok(hand.length >= 6, "the hand fans out as far as it goes");
            for (const zone of ["graveyard", "exile"]) {
                const box = await page.locator(pile("you", zone)).boundingBox();
                for (const card of hand) {
                    const name = await card.getAttribute("data-card");
                    assert.ok(
                        !overlaps(box, await card.boundingBox()),
                        `${name} covers your ${zone}`,
                    );
                }
            }
        },
        {
            ...pileRun,
            viewport,
        },
    );
}

// Six other permanents a side, the opponent's back row reaching under their piles' corner, and
// thick piles on both sides (by-lomm): no layer of any library, graveyard or exile pile lies
// over a permanent, at the reported window and at narrower and wider ones. Cards lean by
// skewing sideways only, so their boxes' heights are their own.
const pilesCrowded = await load("piles-crowded");
for (const viewport of [
    { width: 1728, height: 963 },
    { width: 1600, height: 900 },
    { width: 1280, height: 800 },
    { width: 2134, height: 900 },
]) {
    // Why no audit: every layer of a pile against every permanent, at four windows: cards against
    // cards.
    await check(
        `the piles lie clear of every permanent at ${viewport.width}×${viewport.height}`,
        async (page) => {
            await page.clock.runFor(2000);
            const { piles, field } = await page.evaluate(() => {
                const boxes = (selector) =>
                    [...document.querySelectorAll(selector)]
                        .filter((e) => {
                            const s = getComputedStyle(e);
                            return (
                                s.visibility !== "hidden" &&
                                Number(s.opacity) > 0
                            );
                        })
                        .map((e) => {
                            const r = e.getBoundingClientRect();
                            return {
                                name: `${e.dataset.card ?? "a back"} (${e.dataset.zone})`,
                                x: r.x,
                                y: r.y,
                                right: r.right,
                                bottom: r.bottom,
                            };
                        });
                return {
                    piles: boxes(
                        ".card-layer > .card-el:is([data-zone=library], [data-zone=graveyard], [data-zone=exile])",
                    ),
                    field: boxes(
                        ".card-layer > .card-el[data-zone=battlefield]",
                    ),
                };
            });
            for (const zone of ["library", "graveyard", "exile"]) {
                assert.ok(
                    piles.filter((p) => p.name.endsWith(`(${zone})`)).length >=
                        6,
                    `both ${zone} piles are thick`,
                );
            }
            assert.ok(field.length >= 20, "both sides' permanents are drawn");
            for (const p of piles) {
                for (const b of field) {
                    const across =
                        Math.min(p.right, b.right) - Math.max(p.x, b.x);
                    const down =
                        Math.min(p.bottom, b.bottom) - Math.max(p.y, b.y);
                    assert.ok(
                        across <= 0.5 || down <= 0.5,
                        `${p.name} covers ${b.name} by ${across.toFixed(1)} × ${down.toFixed(1)} px`,
                    );
                }
            }
        },
        {
            fixture: "piles-crowded",
            at: pilesCrowded.length,
            ready: "[data-pile=exile][data-side=opponent]",
            viewport,
        },
    );
}

/** The names of the cards a pile in the piles' view holds. */
function pileNames(side, zone) {
    const slots =
        zone === "graveyard"
            ? pilesView.players[side].graveyard
            : exiledFrom(pilesView, side).map((o) => o.slot);
    return slots.map(
        (slot) => pilesView.objects.find((o) => o.slot === slot).name,
    );
}

// Why no audit: a viewer's list against its pile, and each way to close it.
await check(
    "a pile opens a viewer listing each of its cards, which closes",
    async (page) => {
        await page.clock.runFor(2000);
        // A pile is not pointed at card by card: hovering it shows no card whole.
        await page.hover(pile("you", "graveyard"));
        await page.clock.runFor(500);
        assert.equal(await page.$(".inspect"), null, "a pile card is hovered");
        const close = [
            (p) => p.keyboard.press("Escape"),
            (p) => p.getByRole("button", { name: "Close" }).click(),
            (p) => p.mouse.click(20, 450),
        ];
        const piles = [
            ["you", "graveyard"],
            ["opponent", "graveyard"],
            ["opponent", "exile"],
        ];
        for (const [i, [side, zone]] of piles.entries()) {
            const names = pileNames(side, zone);
            assert.ok(names.length > 0, `the ${side}'s ${zone} has cards`);
            await page.click(pile(side, zone));
            const viewer = `.zone-viewer${pile(side, zone)}`;
            await page.waitForSelector(viewer);
            const listed = await page.$$eval(
                `${viewer} [data-viewer-slot]`,
                (cards) => cards.map((c) => c.dataset.card),
            );
            assert.deepEqual(
                [...listed].sort(),
                [...names].sort(),
                `the ${side}'s ${zone} lists each of its cards`,
            );
            const slots = await page.$$eval(
                `${viewer} [data-viewer-slot]`,
                (cards) => cards.map((c) => c.dataset.viewerSlot),
            );
            for (const slot of slots) {
                const original = page.locator(
                    `.card-layer > [data-slot="${slot}"]`,
                );
                assert.equal(await original.count(), 1);
                assert.equal(
                    await original.isVisible(),
                    false,
                    "the browser owns the card",
                );
            }
            // Once the flight has arrived, each card shows whole on hover.
            await page.clock.runFor(500);
            await page.hover(`${viewer} [data-card="${listed[0]}"]`);
            await page
                .getByLabel(`${listed[0]}, inspected`)
                .waitFor({ state: "visible" });
            await close[i](page);
            await untilGone(page, viewer, 2000);
            for (const slot of slots) {
                assert.equal(
                    await page
                        .locator(`.card-layer > [data-slot="${slot}"]`)
                        .isVisible(),
                    true,
                    "the table takes the original back",
                );
            }
        }
        assert.deepEqual(await chosen(page), [], "browsing answers nothing");
    },
    {
        ...pileRun,
    },
);

// Why no audit: what a viewer says of a pile with nothing known in it.
await check(
    "an empty known library opens without revealing cards",
    async (page) => {
        await page.locator(`button${pile("you", "library")}`).click();
        const dialog = page.getByRole("dialog", { name: /Known cards/ });
        assert.match(await dialog.innerText(), /Nothing here/);
        assert.equal(await dialog.locator("[data-viewer-slot]").count(), 0);
        await dialog.getByRole("button", { name: "Close" }).click();
    },
    {
        ...pileRun,
        reducedMotion: "reduce",
    },
);

// A full row and a scrolling row both borrow from the same live pile. Use distinct slots
// even where the fixture repeats the same printing.
for (const count of [1, 10, 11]) {
    const fixture = `graveyard-${count}`;
    await derive("choice-scry", fixture, pilesAt, (view) => {
        const you = view.players.you;
        const top = view.objects.find((o) => o.slot === you.graveyard.at(-1));
        const first = Math.max(...view.objects.map((o) => o.slot)) + 1;
        const cards = Array.from({ length: count }, (_, i) => ({
            ...top,
            slot: first + i,
        }));
        return {
            ...view,
            objects: [
                ...view.objects.filter((o) => !you.graveyard.includes(o.slot)),
                ...cards,
            ],
            players: {
                ...view.players,
                you: { ...you, graveyard: cards.map((o) => o.slot) },
            },
        };
    });
    // Why no audit: a deep pile's fan, and its viewer scrolled to its last card.
    await check(
        `a ${count}-card graveyard opens with the table cards borrowed`,
        async (page) => {
            const button = page.locator(`button${pile("you", "graveyard")}`);
            await page.clock.runFor(1000);
            const layers = await page
                .locator('.card-layer > [data-zone="graveyard"].card-el-you')
                .evaluateAll((cards) =>
                    cards.map((card) => ({
                        slot: Number(card.dataset.slot),
                        top: card.getBoundingClientRect().top,
                        z: Number(getComputedStyle(card).zIndex),
                    })),
                );
            assert.equal(layers.length, count);
            const ordered = layers.sort((a, b) => b.slot - a.slot);
            assert.ok(
                ordered.every(
                    (layer, i) =>
                        i === 0 ||
                        (layer.top < ordered[i - 1].top &&
                            layer.z < ordered[i - 1].z),
                ),
                "deeper cards fan behind the top card in distinct layers",
            );
            await button.click();
            const dialog = page.getByRole("dialog", { name: "Your graveyard" });
            const listed = dialog.locator("[data-viewer-slot]");
            assert.equal(await listed.count(), count);
            const slot = await listed.last().getAttribute("data-viewer-slot");
            const original = page.locator(
                `.card-layer > [data-slot="${slot}"]`,
            );
            assert.equal(await original.isVisible(), false);
            if (count === 11) {
                await page.clock.runFor(500);
                await listed.last().scrollIntoViewIfNeeded();
                const within = await listed.last().evaluate((card) => {
                    const list = card.parentElement.getBoundingClientRect();
                    const box = card.getBoundingClientRect();
                    return box.bottom <= list.bottom;
                });
                assert.ok(within, "the last card scrolls into view");
            }
            await dialog.getByRole("button", { name: "Close" }).click();
            assert.equal(await original.isVisible(), true);
        },
        {
            ...pileRun,
            fixture,
            viewport: { width: 1180, height: 820 },
        },
    );
}

// Why no audit: how the layout groups copies, over views written for each difference.
await check(
    "a pile splits on any difference, and on a target",
    async (page) => {
        // The board's layout of variants of the fixture's view, by the page's own module.
        const counts = await page.evaluate(
            async ({ view, map }) => {
                const { layout } = await import("/src/game/layout.ts");
                const { zonePiles } = await import("/src/game/zoneViews.ts");
                const order = new Map();
                const borrowed = new Set();
                // Each card of the pile of `map`'s copies that wears a count, with the count.
                const counted = (v) =>
                    [
                        ...layout(
                            v,
                            "held",
                            false,
                            {},
                            order,
                            borrowed,
                            zonePiles(v, order),
                            [],
                        ).values(),
                    ]
                        .filter((p) => p.count !== undefined)
                        .filter((p) =>
                            v.objects.some(
                                (o) => o.slot === p.slot && o.name === map.name,
                            ),
                        )
                        .map((p) => p.count);
                const vary = (change) => {
                    const v = structuredClone(view);
                    const copy = { ...structuredClone(map), slot: 250 };
                    v.objects.push(copy);
                    v.battlefield.push(copy.slot);
                    change(
                        v,
                        v.objects.find((o) => o.slot === copy.slot),
                    );
                    return counted(v);
                };
                const aura = view.objects.find(
                    (o) => o.zone === "battlefield" && o.attachedTo !== null,
                ).slot;
                return {
                    fixture: counted(view),
                    third: vary(() => {}),
                    types: vary((_, o) => o.types.push("creature")),
                    owner: vary((_, o) => (o.owner = "opponent")),
                    tapped: vary((_, o) => (o.tapped = true)),
                    enchanted: vary((v, o) => {
                        v.objects.find((a) => a.slot === aura).attachedTo =
                            o.slot;
                    }),
                    targeted: vary((v, o) =>
                        v.stack.push({
                            kind: "ability",
                            source: null,
                            ability: 0,
                            granted: false,
                            name: "Test",
                            triggered: true,
                            nth: 0,
                            controller: "opponent",
                            targets: [{ kind: "object", slot: o.slot }],
                            x: 0,
                        }),
                    ),
                };
            },
            { view: tokenView, map: offeredToken },
        );
        assert.deepEqual(counts, {
            fixture: [2],
            third: [3],
            types: [2],
            owner: [2],
            tapped: [2],
            enchanted: [2],
            targeted: [2],
        });
    },
    {
        ...tokenRun,
    },
);

// Every object an open decision offers something for has an element a player can see and point
// at: the card on the table or in a choice's tray, a twin the engine offers it through, or the
// button of the pile it lies in. Every exiled card can be seen the same way, if not pointed at
// under a screen over the table. Checked at the last decision of every fixture, with motion
// reduced so that nothing is caught halfway through fading in.
for (const fixture of recorded) {
    const fixtureViews = await load(fixture);
    const last = fixtureViews.findLastIndex(
        (v) => v.status.kind === "deciding",
    );
    if (last < 0) continue;
    const view = fixtureViews[last];
    const byId = new Map(view.objects.map((o) => [o.slot, o]));
    const describe = (slot, why) =>
        `slot ${slot} (${byId.get(slot)?.name ?? "unknown"}, ${why})`;
    const wanted = [];
    for (const c of view.status.decision.candidates) {
        const slot = offeredSlot(c.option);
        if (slot === undefined) continue;
        const o = byId.get(slot);
        const twinSlots = o
            ? view.objects.filter((t) => twins(o, t)).map((t) => t.slot)
            : [];
        wanted.push({
            what: describe(slot, `${c.option.kind} "${c.label}"`),
            slots: [slot, ...twinSlots],
            point: true,
        });
    }
    for (const o of view.objects) {
        if (o.zone === "exile") {
            wanted.push({
                what: describe(o.slot, "exiled"),
                slots: [o.slot],
                point: false,
            });
        }
    }
    const owners = Object.fromEntries(
        view.objects.map((o) => [o.slot, o.owner]),
    );
    // Why no audit: that every object a decision offers takes the pointer where it lies: a hit test
    // of each, in every recording.
    await check(
        `${fixture}: what the decision offers and what is exiled can be seen`,
        async (page) => {
            await page.clock.runFor(5000);
            const look = () =>
                page.evaluate(
                    ({ wanted, owners }) => {
                        const seen = (el, point) => {
                            for (let e = el; e; e = e.parentElement) {
                                const style = getComputedStyle(e);
                                if (Number(style.opacity) < 0.5) return false;
                                if (style.visibility === "hidden") return false;
                            }
                            const r = el.getBoundingClientRect();
                            if (r.width < 1 || r.height < 1) return false;
                            // Some part of it lies on the screen, and, to be pointed at, is the
                            // topmost thing there.
                            for (let i = 1; i < 10; i++) {
                                for (let j = 1; j < 10; j++) {
                                    const x = r.left + (r.width * i) / 10;
                                    const y = r.top + (r.height * j) / 10;
                                    if (x < 0 || y < 0) continue;
                                    if (x >= innerWidth || y >= innerHeight)
                                        continue;
                                    if (!point) return true;
                                    const hit = document.elementFromPoint(x, y);
                                    if (hit && el.contains(hit)) return true;
                                }
                            }
                            return false;
                        };
                        const shown = (slot, point) =>
                            [
                                ...document.querySelectorAll(
                                    `[data-slot="${slot}"]`,
                                ),
                            ].some((el) => {
                                if (!el.classList.contains("card-el-pile")) {
                                    return seen(el, point);
                                }
                                const pile = document.querySelector(
                                    `[data-pile="${el.dataset.zone}"]` +
                                        `[data-side="${owners[slot]}"]`,
                                );
                                return !!pile && seen(pile, point);
                            });
                        return wanted
                            .filter(
                                (w) => !w.slots.some((s) => shown(s, w.point)),
                            )
                            .map((w) => w.what);
                    },
                    { wanted, owners },
                );
            // Fades run on the compositor's clock, not the page's fake one, and a loaded
            // machine slows them: look until all show, the deadline only a hang guard.
            let unseen = await look();
            for (
                const end = Date.now() + 20_000;
                unseen.length && Date.now() < end;
            ) {
                await page.waitForTimeout(100);
                unseen = await look();
            }
            assert.deepEqual(unseen, [], "every one can be seen");
        },
        // The board draws its first view, the whole fixture's, once the fixture has loaded.
        {
            fixture,
            at: last + 1,
            ready: ".table",
            reducedMotion: "reduce",
        },
    );
}

// The first slot is a valid exile host, just like any other battlefield slot.
{
    const fixture = JSON.parse(
        await readFile(join(fixtures, "exiled-beneath.json"), "utf8"),
    );
    const last = fixture.messages.findLast((m) => m.kind === "event");
    const host = exiled.exiledBy;
    const swap = (slot) => (slot === host ? 0 : slot === 0 ? host : slot);
    last.payload.view = {
        ...beneathView,
        battlefield: beneathView.battlefield.map(swap),
        objects: beneathView.objects.map((o) => ({
            ...o,
            slot: swap(o.slot),
            exiledBy: swap(o.exiledBy),
        })),
    };
    // On the stage the click is the open prompt's first, which has no use for it.
    const asked = last.payload.view.status.decision;
    if (asked) {
        fixture.messages.unshift({
            kind: "response",
            command: "click",
            value: SAME(asked),
        });
    }
    await writeFixture("exiled-beneath-first", fixture);
}

// Why no audit: what a click on a card tucked under its host opens: a viewer no screen at rest
// shows.
await check(
    "a card exiled by the first slot stays under its host",
    async (page) => {
        await page.clock.runFor(2000);
        const host = await page.locator('[data-slot="0"]').boundingBox();
        const under = await page.locator(beneathRun.ready).boundingBox();
        assert.ok(under.x < host.x && under.x + under.width > host.x);
        const at = { x: (under.x + host.x) / 2, y: under.y + under.height / 2 };
        await page.mouse.click(at.x, at.y);
        await page
            .locator(`.zone-viewer [data-viewer-slot="${exiled.slot}"]`)
            .waitFor();
    },
    {
        ...beneathRun,
        fixture: "exiled-beneath-first",
    },
);
