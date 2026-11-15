// The generic layout audits run with the library split. Action checks assert the
// live table's destination, order, face and permissions after pointer gestures.
import assert from "node:assert/strict";
import { duplicates, record } from "./audit.mjs";
import { crowdedControls, screenFaults, stageFaults } from "./capture.mjs";
import { check, committed } from "./kept-harness.mjs";
import {
    clickTableResult,
    startTable,
    tableActionRef,
    tableRoute,
    tableView,
} from "./table.mjs";

const live = { route: tableRoute, ready: ".start-panel", serial: true };
const ref = (view, slot) => tableActionRef(view, { kind: "object", slot });
const card = (page, slot) =>
    page.locator(`.card-layer > [data-slot="${slot}"]`);
const pile = (page, zone) =>
    page.locator(`button[data-pile="${zone}"][data-side=you]`);
const hand = (view, name) =>
    view.objects.find((o) => o.zone === "hand" && (!name || o.name === name))
        ?.slot;
const move = (to, slot) => (view) => ({
    move: { to, ...ref(view, slot ? slot(view) : hand(view)) },
});
const point = async (locator, x = 0.5, y = 0.5) => {
    const box = await locator.boundingBox();
    assert.ok(box, String(locator));
    return { x: box.x + x * box.width, y: box.y + y * box.height };
};
const stagePoint = (page, x, y) =>
    page.locator(".card-layer").evaluate(
        (layer, at) => {
            const box = layer.getBoundingClientRect();
            const scale = box.width / layer.offsetWidth;
            return { x: box.x + at.x * scale, y: box.y + at.y * scale };
        },
        { x, y },
    );
async function begin(page, from, to) {
    await page.mouse.move(from.x, from.y);
    await page.mouse.down();
    await page.mouse.move(to.x, to.y, { steps: 4 });
    await committed(page);
}
async function finish(page) {
    const [result] = await Promise.all([
        clickTableResult(page),
        page.mouse.up(),
    ]);
    return result;
}
async function drag(page, from, to) {
    await begin(page, from, to);
    return finish(page);
}

for (const position of ["top", "bottom"]) {
    await check(
        `manual table drag hand to library ${position}`,
        async (page) => {
            await startTable(page, "24 Island");
            const before = await tableView();
            const slot = before.table.orders.you.hand.at(-1);
            await begin(
                page,
                await point(card(page, slot), 0.6, 0.15),
                await point(
                    pile(page, "library"),
                    0.5,
                    position === "top" ? 0.25 : 0.75,
                ),
            );
            const halves = page.locator(
                "[data-table-drop=library] [data-position]",
            );
            assert.deepEqual(await halves.allTextContents(), ["Top", "Bottom"]);
            assert.equal(
                await page
                    .locator("[data-table-drop=library] [data-over]")
                    .getAttribute("data-position"),
                position,
            );
            await page.evaluate(record);
            const frames = await page.evaluate(() => {
                window.__motion.frame(0);
                window.__motion.unhit();
                return window.__motion.frames;
            });
            assert.deepEqual(duplicates(frames), []);
            assert.deepEqual(await screenFaults(page), []);
            assert.deepEqual(await stageFaults(page), []);
            assert.deepEqual(await crowdedControls(page), []);
            const after = await finish(page);
            const index = position === "top" ? -1 : 0;
            assert.equal(
                after.players.you.library.length,
                before.players.you.library.length + 1,
            );
            assert.equal(
                after.players.you.library.at(position === "top" ? 0 : -1),
                before.players.you.library.at(position === "top" ? 0 : -1),
            );
            assert.equal(
                after.table.orders.you.hand.length,
                before.table.orders.you.hand.length - 1,
            );
            assert.ok(
                after.players.you.library.at(index) !==
                    before.players.you.library.at(index),
            );
        },
        live,
    );
}

await check(
    "manual table drag two cards into library order",
    async (page) => {
        await startTable(page, "24 Island");
        const before = await tableView();
        const slots = before.table.orders.you.hand.slice(-2);
        const modifier = await page.evaluate(() =>
            /Mac/.test(navigator.platform) ? "Meta" : "Control",
        );
        for (const slot of slots) {
            const at = await point(card(page, slot), 0.3, 0.08);
            await page.keyboard.down(modifier);
            await page.mouse.click(at.x, at.y);
            await page.keyboard.up(modifier);
        }
        await begin(
            page,
            await point(card(page, slots[1]), 0.3, 0.12),
            await point(pile(page, "library"), 0.5, 0.25),
        );
        assert.equal(await page.locator(".table-drag-stack .card").count(), 1);
        await page.mouse.up();
        const order = page.getByRole("dialog", { name: "Order Cards" });
        await order.waitFor();
        assert.equal(
            (await tableView()).table.log.length,
            before.table.log.length,
        );
        const first = order.locator("[data-order-slot]").first();
        await first.press("ArrowDown");
        const reordered = await order
            .locator("[data-order-slot]")
            .evaluateAll((nodes) =>
                nodes.map((node) => Number(node.dataset.orderSlot)),
            );
        assert.deepEqual(reordered, slots);
        const request = page.waitForRequest((request) =>
            request.url().endsWith("/invoke/table_act"),
        );
        const update = clickTableResult(page);
        await order.getByRole("button", { name: "Done" }).click();
        const sent = (await request).postDataJSON().request.action;
        assert.deepEqual(
            sent.move.objects,
            slots.map((slot) => ref(before, slot).objects[0]),
        );
        assert.equal(
            (await update).players.you.library.length,
            before.players.you.library.length + 2,
        );
    },
    live,
);

await check(
    "manual table drag library top and Option bottom to hand",
    async (page) => {
        await startTable(page, "24 Island");
        const before = await tableView();
        const library = pile(page, "library");
        const top = before.players.you.library.at(-1);
        const bottom = before.players.you.library[0];
        const destination = await stagePoint(page, 600, 850);
        const afterTop = await drag(page, await point(library), destination);
        assert.deepEqual(
            afterTop.players.you.library,
            before.players.you.library.slice(0, -1),
        );
        assert.notEqual(top, bottom);
        await page.keyboard.down("Alt");
        await begin(page, await point(library), destination);
        await page.keyboard.up("Alt");
        const afterBottom = await finish(page);
        assert.deepEqual(
            afterBottom.players.you.library,
            before.players.you.library.slice(1, -1),
        );
    },
    live,
);

await check(
    "manual table drag through stack, graveyard, exile and viewer",
    async (page) => {
        await startTable(page, "24 Island");
        const slot = (await tableView()).table.orders.you.hand.at(-1);
        const stack = await stagePoint(page, 1390, 400);
        const afterStack = await drag(
            page,
            await point(card(page, slot), 0.6, 0.12),
            stack,
        );
        const stacked = afterStack.objects.find((o) => o.zone === "stack").slot;
        const grave = await stagePoint(page, 180, 825);
        const afterGrave = await drag(
            page,
            await point(card(page, stacked)),
            grave,
        );
        assert.equal(afterGrave.players.you.graveyard.length, 1);
        const exile = await stagePoint(page, 295, 825);
        const afterExile = await drag(
            page,
            await point(pile(page, "graveyard")),
            exile,
        );
        assert.equal(afterExile.players.you.graveyard.length, 0);
        assert.equal(afterExile.table.orders.you.exile.length, 1);
        await pile(page, "exile").click();
        const viewer = page.locator(".zone-viewer-card");
        await viewer.waitFor();
        await page.keyboard.down("Shift");
        const afterField = await drag(
            page,
            await point(viewer),
            await stagePoint(page, 480, 475),
        );
        await page.keyboard.up("Shift");
        assert.equal(afterField.table.orders.you.exile.length, 0);
        assert.equal(
            afterField.objects.find((o) => o.zone === "battlefield").face,
            "down",
        );
    },
    live,
);

await check(
    "manual table drag reorders a row and pins a land",
    async (page) => {
        await startTable(page, "24 Island", [
            move("battlefield"),
            move("battlefield"),
            move("battlefield"),
        ]);
        const before = await tableView();
        const [a, b, c] = before.battlefield;
        const target = await point(card(page, c), 0.94, 0.6);
        const afterReorder = await drag(
            page,
            await point(card(page, a), 0.15, 0.5),
            target,
        );
        assert.deepEqual(afterReorder.battlefield, [b, c, a]);
        const afterPin = await drag(
            page,
            await point(card(page, a), 0.88, 0.55),
            await stagePoint(page, 480, 470),
        );
        assert.equal(
            afterPin.objects.find((o) => o.slot === a).pinned,
            "creatures",
        );
    },
    live,
);

await check(
    "manual table drag attaches to a permanent and unattaches",
    async (page) => {
        await startTable(page, "24 Bonesplitter", [
            move("battlefield"),
            move("battlefield"),
        ]);
        const [host, item] = (await tableView()).battlefield;
        const afterAttach = await drag(
            page,
            await point(card(page, item), 0.88, 0.5),
            await point(card(page, host), 0.14, 0.55),
        );
        assert.equal(
            afterAttach.objects.find((o) => o.slot === item).attachedTo,
            host,
        );
        const afterUnattach = await drag(
            page,
            await point(card(page, item), 0.96, 0.65),
            await stagePoint(page, 1080, 580),
        );
        assert.equal(
            afterUnattach.objects.find((o) => o.slot === item).attachedTo,
            null,
        );
    },
    live,
);

await check(
    "manual table drag to the open zone viewer",
    async (page) => {
        await startTable(page, "24 Island", [move({ graveyard: 0 })]);
        const before = await tableView();
        await pile(page, "graveyard").click();
        const viewer = page.getByRole("dialog", { name: "Your graveyard" });
        await viewer.waitFor();
        const slot = before.table.orders.you.hand.at(-1);
        const after = await drag(
            page,
            await point(card(page, slot), 0.6, 0.12),
            await point(viewer, 0.35, 0.6),
        );
        assert.equal(after.players.you.graveyard.length, 2);
        assert.equal(
            after.table.orders.you.hand.length,
            before.table.orders.you.hand.length - 1,
        );
    },
    live,
);

await check(
    "manual table drag sends give from the guest seat and cannot pick up a private pile",
    async (page) => {
        // The live CLI currently exposes solo play. Serve the browser a guest's
        // projection and inspect its request before a linked host exists.
        await page.route("**/invoke/table_current", async (route) => {
            const response = await route.fetch();
            const payload = await response.json();
            const { view } = payload.ok;
            const slot = view.players.you.library.shift();
            view.players.opponent.library.push(slot);
            view.objects.find((object) => object.slot === slot).owner =
                "opponent";
            view.table.seats = 2;
            view.table.seat = 1;
            view.table.objectActions = view.table.objectActions.filter(
                (action) => action.subject.slot !== slot,
            );
            await route.fulfill({ response, json: payload });
        });
        await startTable(page, "24 Island", [move("battlefield")]);
        const before = await tableView();
        const [permanent] = before.battlefield;
        const library = page.locator(
            "button[data-pile=library][data-side=opponent]",
        );
        await begin(
            page,
            await point(library),
            await stagePoint(page, 1150, 830),
        );
        assert.equal(
            await page.evaluate(() => window.berylStage.pointer.held),
            undefined,
        );
        await page.mouse.up();
        let action;
        await page.route("**/invoke/table_act", (route) => {
            action = route.request().postDataJSON().request.action;
            return route.fulfill({
                headers: { "Access-Control-Allow-Origin": "*" },
                json: { ok: { refused: "permission" } },
            });
        });
        const result = await drag(
            page,
            await point(card(page, permanent)),
            await stagePoint(page, 480, 260),
        );
        assert.deepEqual(result, { refused: "permission" });
        assert.deepEqual(action, {
            give: { to: 0, ...ref(before, permanent) },
        });
        assert.deepEqual((await tableView()).battlefield, [permanent]);
    },
    live,
);

await check(
    "manual table drag released outside a target returns the card",
    async (page) => {
        await startTable(page, "24 Island");
        const before = await tableView();
        const slot = before.table.orders.you.hand.at(-1);
        const rests = () =>
            page.evaluate((id) => window.berylStage.pose(id), slot);
        const original = await rests();
        await begin(
            page,
            await point(card(page, slot), 0.6, 0.12),
            await stagePoint(page, 1400, 75),
        );
        assert.equal(await page.locator("[data-table-drop]").count(), 0);
        await page.mouse.up();
        assert.notDeepEqual(await rests(), original);
        await page.evaluate(() => window.berylStage.clock.advance(1000));
        assert.deepEqual(await rests(), original);
        assert.equal(
            (await tableView()).table.log.length,
            before.table.log.length,
        );
    },
    live,
);
