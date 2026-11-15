// These checks need live host replies and references. Stage audits cover the layout and
// animation but cannot tell which manual action a press sends, or what the host accepts.
import assert from "node:assert/strict";
import { check, committed } from "./kept-harness.mjs";
import {
    clickTable,
    startTable,
    showTableView,
    tableActionRef,
    tableCommand,
    tableRoute,
    tableView,
} from "./table.mjs";

const LANDS = "24 Island";
const SPELLS = "24 Counterspell";
const ENTERS_TAPPED = "24 Tranquil Cove";
const tabletop = { route: tableRoute, ready: ".start-panel", serial: true };
const object = (view, zone) => view.objects.find((o) => o.zone === zone);
const card = (page, slot) =>
    page.locator(`.card-layer > [data-slot="${slot}"]`);
const pile = (page, zone) =>
    page.locator(`button[data-pile="${zone}"][data-side="you"]`);
const counter = (page, name) =>
    page.locator(`[data-side="you"] [data-counter="${name}"]`);
const hand = (view) => view.objects.filter((card) => card.zone === "hand");
const frontHand = (view) => view.table.orders.you.hand.at(-1);
const ref = (view, slot) => tableActionRef(view, { kind: "object", slot });

const moveHand = (to) => (view) => ({
    move: { to, ...ref(view, hand(view)[0].slot) },
});

await check(
    "manual table click starts a solo table and draws",
    async (page) => {
        await page.getByRole("button", { name: "Solo table" }).click();
        await page.locator("[data-manual-table]").waitFor();
        const before = await tableView();
        assert.equal(before.table.seats, 1);
        assert.equal(hand(before).length, 7);
        assert.equal(before.players.you.library.length, 33);
        const after = await clickTable(page, pile(page, "library"));
        assert.equal(hand(after).length, 8);
        assert.equal(
            after.players.you.library.length,
            before.players.you.library.length - 1,
        );
        assert.equal(
            await page.locator(".card-layer > [data-zone=hand]").count(),
            8,
        );
    },
    tabletop,
);

await check(
    "manual table click plays a land, then taps and untaps it",
    async (page) => {
        await startTable(page, ENTERS_TAPPED);
        const first = card(page, frontHand(await tableView()));
        let view = await clickTable(page, first);
        const land = object(view, "battlefield");
        assert.ok(land.tapped, "the host enters this land tapped");
        const permanent = card(page, land.slot);
        view = await clickTable(page, permanent);
        assert.equal(object(view, "battlefield").tapped, false);
        view = await clickTable(page, permanent);
        assert.equal(object(view, "battlefield").tapped, true);
    },
    tabletop,
);

await check(
    "manual table click plays and resolves a spell and either matching marker",
    async (page) => {
        await startTable(page, SPELLS, [
            moveHand("battlefield"),
            ...[0, 1].map(() => (view) => ({
                marker: {
                    text: "Draw a card.",
                    ...ref(view, object(view, "battlefield").slot),
                },
            })),
        ]);
        let view = await clickTable(
            page,
            card(page, frontHand(await tableView())),
        );
        const spell = object(view, "stack");
        assert.ok(spell);
        view = await clickTable(page, card(page, spell.slot));
        assert.equal(object(view, "graveyard").name, spell.name);
        const ids = view.stack.map((item) => item.manual.id);
        assert.equal(ids.length, 2);
        view = await clickTable(
            page,
            page.locator(`[data-stage="a${ids[1]}"]`),
        );
        assert.deepEqual(
            view.stack.map((item) => item.manual.id),
            [ids[0]],
        );
        view = await clickTable(
            page,
            page.locator(`[data-stage="a${ids[0]}"]`),
        );
        assert.deepEqual(view.stack, []);
    },
    tabletop,
);

await check(
    "manual table click opens graveyard and exile and plays from their viewers",
    async (page) => {
        await startTable(page, SPELLS, [
            moveHand({ graveyard: 0 }),
            moveHand({ exile: 0 }),
        ]);
        await pile(page, "graveyard").click();
        const graveyard = page.getByRole("dialog", { name: "Your graveyard" });
        const cardInGraveyard = graveyard.locator("[data-viewer-slot]");
        let view = await clickTable(page, cardInGraveyard);
        assert.ok(object(view, "stack"));
        assert.equal(object(view, "graveyard"), undefined);
        await graveyard.getByRole("button", { name: "Close" }).click();
        await page.clock.runFor(250);
        await pile(page, "exile").click();
        const exile = page.getByRole("dialog", { name: "Your exile" });
        view = await clickTable(page, exile.locator("[data-viewer-slot]"), {
            modifiers: ["Shift"],
        });
        assert.equal(object(view, "exile"), undefined);
        assert.equal(object(view, "battlefield").face, "down");
    },
    tabletop,
);

await check(
    "manual table click shifts a hand card face down and hides an unknown preview",
    async (page) => {
        await startTable(page, LANDS, [
            moveHand("battlefield"),
            (view) => ({
                move: {
                    to: "battlefield",
                    face: "down",
                    ...ref(view, view.players.you.library[0]),
                },
            }),
        ]);
        const before = await tableView();
        const known = before.objects.find((o) => o.slot === frontHand(before));
        const field = before.objects.filter((o) => o.zone === "battlefield");
        const visible = field.find((o) => o.name !== null);
        const unknown = field.find((o) => o.name === null);
        assert.equal(unknown.name, null);

        await card(page, visible.slot).hover();
        await committed(page);
        assert.equal(await page.locator(".stage .inspect").count(), 1);
        await card(page, unknown.slot).hover();
        await committed(page);
        assert.equal(await page.locator(".stage .inspect").count(), 0);
        const view = await clickTable(page, card(page, known.slot), {
            modifiers: ["Shift"],
        });
        assert.equal(
            view.objects.find(
                (o) =>
                    o.zone === "battlefield" &&
                    o.name === known.name &&
                    o.face === "down",
            )?.face,
            "down",
        );
    },
    tabletop,
);

await check(
    "manual table click and right click adjust mana and a named counter",
    async (page) => {
        await startTable(page, LANDS, [
            { player_counter: { seat: 0, name: "energy", set: 1 } },
        ]);
        const mana = counter(page, "W");
        let view = await clickTable(page, mana);
        assert.equal(view.players.you.mana.white, 1);
        view = await clickTable(page, mana, { button: "right" });
        assert.equal(view.players.you.mana.white, 0);
        const energy = counter(page, "energy");
        view = await clickTable(page, energy);
        assert.deepEqual(view.players.you.namedCounters, [
            { name: "energy", count: 2 },
        ]);
        view = await clickTable(page, energy, { button: "right" });
        assert.deepEqual(view.players.you.namedCounters, [
            { name: "energy", count: 1 },
        ]);
    },
    tabletop,
);

await check(
    "manual table click shakes the refused card without changing the view",
    async (page) => {
        await startTable(page, LANDS, [moveHand("battlefield")]);
        const before = await tableView();
        const permanent = card(page, object(before, "battlefield").slot);
        await page.evaluate(() => {
            const animate = Element.prototype.animate;
            window.tableShakes = [];
            Element.prototype.animate = function (frames, options) {
                if (options?.id === "table-refusal")
                    window.tableShakes.push([
                        Number(this.closest("[data-slot]")?.dataset.slot),
                        frames,
                    ]);
                return animate.call(this, frames, options);
            };
        });
        // Invalidate only the request that came off the board. The real host returns its
        // stale-reference refusal and publishes no update.
        await page.route("**/invoke/table_act", async (route) => {
            const body = route.request().postDataJSON();
            body.request.action.tap.objects[0].view = "stale";
            await route.continue({ postData: JSON.stringify(body) });
        });
        const refused = await clickTable(page, permanent);
        assert.equal(refused.refused, "stale");
        await page.waitForFunction(() => window.tableShakes.length > 0);
        const [[shaken, frames]] = await page.evaluate(
            () => window.tableShakes,
        );
        assert.equal(shaken, object(before, "battlefield").slot);
        assert.ok(
            frames.some(({ translate }) => Number.parseInt(translate, 10) < 0),
        );
        assert.deepEqual(await tableView(), before);
    },
    tabletop,
);

await check(
    "manual table click draws and plays on the phone stage",
    async (page) => {
        await startTable(page, LANDS);
        let view = await clickTable(page, pile(page, "library"), { tap: true });
        assert.equal(hand(view).length, 8);
        view = await clickTable(page, card(page, frontHand(view)), {
            tap: true,
        });
        assert.ok(object(view, "battlefield"));
    },
    {
        ...tabletop,
        route: (page) => tableRoute(page, true),
        phone: true,
        viewport: { width: 393, height: 759 },
    },
);

await check(
    "manual table click shows the press and cancels it when the view changes",
    async (page) => {
        await startTable(page, LANDS, [moveHand("battlefield")]);
        const before = await tableView();
        const permanent = card(page, object(before, "battlefield").slot);
        await permanent.hover();
        await page.mouse.down();
        assert.equal(
            await permanent
                .locator(".card-slot")
                .evaluate((node) => getComputedStyle(node).filter),
            "brightness(0.85)",
        );
        assert.deepEqual(await tableView(), before);

        const reply = await tableCommand("table_act", {
            request: { action: { life: { seat: 0, add: 1 } } },
        });
        assert.deepEqual(reply, { ok: true });
        const changed = await tableView();
        await showTableView(page, changed);
        await page.mouse.up();

        const actions = await page.evaluate(() =>
            window.berylLive.log.filter(
                (entry) =>
                    entry.kind === "invoke" && entry.command === "table_act",
            ),
        );
        assert.deepEqual(actions, []);
    },
    tabletop,
);
