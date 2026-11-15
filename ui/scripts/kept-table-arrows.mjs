// These checks follow the pointer from a card to a target and verify the sent
// action and the visible line. The layout audits cannot express those properties.
import assert from "node:assert/strict";
import { center, check } from "./kept-harness.mjs";
import {
    clickTableResult,
    showTableView,
    startTable,
    tableActionRef,
    tableRoute,
    tableView,
} from "./table.mjs";

const live = { route: tableRoute, ready: ".start-panel", serial: true };
const card = (page, slot) =>
    page.locator('.card-layer > [data-slot="' + slot + '"]');
const life = (page) => page.locator('[data-life="you"]');
const trace = (page) => page.locator("[data-aimed]");
const ownLife = { kind: "player", player: "you" };
const object = (slot) => ({ kind: "object", slot });
const line = (page, kind, from, to) =>
    page.locator('[data-line="' + [kind, from, to].join(" ") + '"]');
const field = (view) => view.battlefield;
const lastHand = (view) => view.table.orders.you.hand.at(-1);
const playLand = (view) => ({
    move: { to: "play", ...tableActionRef(view, object(lastHand(view))) },
});

async function aim(page, from, to) {
    await page.mouse.move(from.x, from.y);
    await page.mouse.down({ button: "right" });
    await page.mouse.move(to.x, to.y, { steps: 4 });
    assert.equal(await trace(page).count(), 1, "live pointer trace");
}

async function draw(page, from, to) {
    await aim(page, from, to);
    const view = clickTableResult(page);
    await page.mouse.up({ button: "right" });
    assert.equal(await trace(page).count(), 0, "trace ends on release");
    return view;
}

await check(
    "manual table arrow to a card outside combat",
    async (page) => {
        await startTable(page, "24 Island", [playLand, playLand]);
        const [from, to] = field(await tableView());
        const view = await draw(
            page,
            center(await card(page, from).boundingBox()),
            center(await card(page, to).boundingBox()),
        );
        assert.deepEqual(view.table.arrows, [
            {
                arrow: "friendly-target",
                from: object(from),
                to: object(to),
                own: true,
            },
        ]);
        assert.ok(
            await line(page, "friendly-target", from, to).getAttribute("d"),
        );
    },
    live,
);

await check(
    "manual table arrow from hand plays the card first",
    async (page) => {
        await startTable(page, "24 Island");
        const from = lastHand(await tableView());
        const sent = [];
        page.on("request", (request) => {
            if (request.url().endsWith("/invoke/table_act"))
                sent.push(request.postDataJSON().request.action);
        });
        const arrowReply = page.waitForResponse(
            (response) =>
                response.url().endsWith("/invoke/table_act") &&
                !!response.request().postDataJSON().request.action.arrow,
        );
        const box = await card(page, from).boundingBox();
        await aim(
            page,
            { x: box.x + box.width * 0.6, y: box.y + box.height * 0.15 },
            center(await life(page).boundingBox()),
        );
        await page.mouse.up({ button: "right" });
        assert.deepEqual(await (await arrowReply).json(), { ok: { ok: true } });
        assert.equal(sent.length, 2);
        assert.equal(sent[0].move.to, "play");
        assert.ok("hand" in sent[0].move.in);
        assert.equal(sent[1].arrow.in, "battlefield");
        assert.notEqual(
            sent[0].move.objects[0].view,
            sent[1].arrow.objects[0].view,
        );
        const view = await tableView();
        await showTableView(page, view);
        assert.ok(field(view).includes(from));
        assert.deepEqual(view.table.arrows, [
            {
                arrow: "friendly-target",
                from: object(from),
                to: ownLife,
                own: true,
            },
        ]);
    },
    live,
);

await check(
    "manual table arrow selection batches every card and shortcut clears",
    async (page) => {
        await startTable(page, "24 Island", [playLand, playLand]);
        const cards = field(await tableView());
        const modifier = await page.evaluate(() =>
            /Mac/.test(navigator.platform) ? "Meta" : "Control",
        );
        for (const slot of cards)
            await card(page, slot).click({ modifiers: [modifier] });
        const request = page.waitForRequest((request) =>
            request.url().endsWith("/invoke/table_act"),
        );
        const view = await draw(
            page,
            center(await card(page, cards[0]).boundingBox()),
            center(await life(page).boundingBox()),
        );
        assert.equal(
            (await request).postDataJSON().request.action.batch.actions.length,
            2,
        );
        assert.deepEqual(
            view.table.arrows.map(({ from }) => from.slot).sort(),
            [...cards].sort(),
        );
        const cleared = clickTableResult(page);
        await page.keyboard.press(modifier + "+r");
        assert.equal((await cleared).table.arrows, undefined);
        assert.equal(await page.locator("[data-line*=' you']").count(), 0);
    },
    live,
);

await check(
    "manual table arrow right-click removes the drawn curve",
    async (page) => {
        await startTable(page, "24 Island", [playLand]);
        const [from] = field(await tableView());
        await draw(
            page,
            center(await card(page, from).boundingBox()),
            center(await life(page).boundingBox()),
        );
        const path = line(page, "friendly-target", from, "you");
        const at = await path.evaluate((path) => {
            const point = path.getPointAtLength(path.getTotalLength() * 0.23);
            const pixel = point.matrixTransform(path.getScreenCTM());
            return { x: pixel.x, y: pixel.y };
        });
        const cleared = clickTableResult(page);
        await page.mouse.click(at.x, at.y, { button: "right" });
        assert.equal((await cleared).table.arrows, undefined);
        assert.equal(await path.count(), 0);
    },
    live,
);

await check(
    "manual table arrow Escape cancels the drag",
    async (page) => {
        await startTable(page, "24 Island", [playLand]);
        const before = await tableView();
        await aim(
            page,
            center(await card(page, field(before)[0]).boundingBox()),
            center(await life(page).boundingBox()),
        );
        await page.keyboard.press("Escape");
        assert.equal(await trace(page).count(), 0);
        await page.mouse.up({ button: "right" });
        assert.deepEqual((await tableView()).table.log, before.table.log);
    },
    live,
);

await check(
    "manual table arrow menu target click and Escape",
    async (page) => {
        await startTable(page, "24 Island", [playLand, playLand]);
        const before = await tableView();
        const [from, to] = field(before);
        const pick = () =>
            page.evaluate((from) => {
                window.tableTarget = window.berylPickTableTarget(from);
            }, from);
        await pick();
        const at = center(await card(page, to).boundingBox());
        await page.mouse.move(at.x, at.y);
        assert.equal(await trace(page).count(), 1);
        await page.mouse.click(at.x, at.y);
        assert.deepEqual(await page.evaluate(() => window.tableTarget), {
            name: to,
        });
        assert.equal(await trace(page).count(), 0);
        assert.deepEqual((await tableView()).table.log, before.table.log);
        await pick();
        await page.mouse.move(at.x + 1, at.y);
        assert.equal(await trace(page).count(), 1);
        await page.keyboard.press("Escape");
        assert.equal(await page.evaluate(() => window.tableTarget), undefined);
        assert.equal(await trace(page).count(), 0);
    },
    live,
);

await check(
    "manual table arrow attack and block use the host's projected styles",
    async (page) => {
        await startTable(page, "24 Island", [playLand, playLand]);
        const view = await tableView();
        const [attacker, blocker] = field(view);
        // The solo server has one seat. Replay the two projected combat spans
        // here and check classification and per-seat clearing on the Rust table.
        view.table.seats = 2;
        view.table.arrows = [
            {
                arrow: "attack",
                from: object(attacker),
                to: { kind: "player", player: "opponent" },
                own: true,
            },
            {
                arrow: "block",
                from: object(blocker),
                to: object(attacker),
                own: false,
            },
        ];
        const block = view.objects.find(({ slot }) => slot === blocker);
        block.controller = "opponent";
        view.turn.step = "declareBlockers";
        view.table.phase = "declareBlockers";
        const script = {
            reset: { abilities: [] },
            beats: [
                view.table.arrows.map(({ arrow, from, to }) => ({
                    act: { verb: "point", span: "begin", arrow, from, to },
                })),
            ],
        };
        await page.evaluate(
            ({ view, script }) => window.berylStage.play(view, script),
            { view, script },
        );
        assert.match(
            await line(page, "attack", attacker, "opponent").getAttribute("d"),
            /^M.* L/,
        );
        const path = line(page, "block", blocker, attacker);
        assert.match(await path.getAttribute("d"), /^M.* L/);
        const at = await path.evaluate((path) => {
            const point = path.getPointAtLength(path.getTotalLength() / 2);
            const pixel = point.matrixTransform(path.getScreenCTM());
            return { x: pixel.x, y: pixel.y };
        });
        await page.mouse.click(at.x, at.y, { button: "right" });
        assert.match(await path.getAttribute("d"), /^M.* L/);
        assert.ok(
            !(await page.evaluate(() =>
                window.berylLive.log.some(
                    ({ kind, command }) =>
                        kind === "invoke" && command === "table_act",
                ),
            )),
            "another seat's line did not send a clear action",
        );
    },
    live,
);
