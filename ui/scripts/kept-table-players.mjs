// The generic audits cannot check typed adjustments, which seat a shortcut
// affects, or whether the banner matches the roll recorded by the host.
import assert from "node:assert/strict";
import { check } from "./kept-harness.mjs";
import {
    clickTable,
    clickTableResult,
    startTable,
    tableActionRef,
    tableRoute,
    tableView,
} from "./table.mjs";

const deck = "24 Llanowar Elves";
const live = { route: tableRoute, ready: ".start-panel", serial: true };
const life = (page, side = "you") =>
    page.locator(`[data-life="${side}"] .life-hit`);
const counter = (page, name) =>
    page.locator(`[data-side="you"] [data-counter="${name}"]`);
const field = (page, name) => page.getByRole("textbox", { name, exact: true });
const row = (page, name) => page.getByRole("menuitem", { name, exact: true });

async function setup(page, actions = []) {
    await startTable(page, deck, actions);
    return await page.evaluate(() =>
        /Mac/.test(navigator.platform) ? "Meta" : "Control",
    );
}

async function sendKey(page, key) {
    const [view] = await Promise.all([
        clickTableResult(page),
        page.keyboard.press(key),
    ]);
    return view;
}

await check(
    "manual table players edit life by a relative or absolute amount",
    async (page) => {
        const mod = await setup(page);
        const before = (await tableView()).players.you.life;
        await life(page).click();
        const input = field(page, "Set life");
        assert.equal(await input.inputValue(), String(before));
        await input.fill("+3");
        assert.equal(
            (await sendKey(page, "Enter")).players.you.life,
            before + 3,
        );

        await page.keyboard.press(mod + "+l");
        assert.equal(await input.inputValue(), String(before + 3));
        await input.fill("17");
        assert.equal((await sendKey(page, "Enter")).players.you.life, 17);

        await life(page).click({ button: "right" });
        const view = await clickTable(page, row(page, "−5"));
        assert.equal(view.players.you.life, 12);
    },
    live,
);

await check(
    "manual table players adjust life with F11, F12, minus and equals",
    async (page) => {
        await setup(page);
        const start = (await tableView()).players.you.life;
        for (const [key, offset] of [
            ["F12", 1],
            ["=", 2],
            ["F11", 1],
            ["-", 0],
        ])
            assert.equal(
                (await sendKey(page, key)).players.you.life,
                start + offset,
            );
    },
    live,
);

await check(
    "manual table players add and remove mana with the mouse",
    async (page) => {
        await setup(page);
        const white = counter(page, "W");
        assert.equal((await clickTable(page, white)).players.you.mana.white, 1);
        assert.equal(await white.getAttribute("aria-label"), "W counter: 1");
        const removed = await clickTable(page, white, { button: "right" });
        assert.equal(removed.players.you.mana.white, 0);
        assert.equal(await white.getAttribute("aria-label"), "W counter: 0");
    },
    live,
);

await check(
    "manual table players show storm at one and can set and remove it",
    async (page) => {
        const mod = await setup(page);
        const storm = counter(page, "storm");
        assert.equal(await storm.count(), 0);
        await sendKey(page, mod + "+]");
        assert.equal(
            await storm.getAttribute("aria-label"),
            "storm counter: 1",
        );
        await page.keyboard.press(mod + "+Backslash");
        const input = field(page, "Set storm counter");
        assert.equal(await input.inputValue(), "1");
        await input.fill("2");
        await sendKey(page, "Enter");
        await sendKey(page, mod + "+[");
        assert.equal(
            await storm.getAttribute("aria-label"),
            "storm counter: 1",
        );
        await clickTable(page, storm, { button: "right" });
        assert.equal(await storm.count(), 0);
    },
    live,
);

await check(
    "manual table players add a named counter from the life menu",
    async (page) => {
        await setup(page);
        await life(page).click({ button: "right" });
        await row(page, "Add counter…").click();
        await field(page, "Add counter").fill("quest");
        await page.keyboard.press("Enter");
        await field(page, "Number").fill("3");
        const added = await sendKey(page, "Enter");
        assert.deepEqual(added.players.you.namedCounters, [
            { name: "quest", count: 3 },
        ]);
        const quest = counter(page, "quest");
        assert.equal(
            await quest.getAttribute("aria-label"),
            "quest counter: 3",
        );
        const incremented = await clickTable(page, quest);
        assert.equal(incremented.players.you.namedCounters[0].count, 4);
    },
    live,
);

async function announce(page, key) {
    const response = page.waitForResponse((result) =>
        result.url().endsWith("/invoke/table_act"),
    );
    await page.keyboard.press(key);
    assert.deepEqual((await (await response).json()).ok, { ok: true });
    await page.evaluate(
        () =>
            new Promise((resolve) => {
                const stage = window.berylStage;
                const check = () => {
                    if (!stage.shown.busy) return;
                    stop();
                    resolve();
                };
                const stop = stage.subscribe(check);
                check();
            }),
    );
    await page.evaluate(() => window.berylStage.clock.advance(250));
    return (await tableView()).table.log.at(-1).text;
}

await check(
    "manual table players announce the host's dice and coin outcomes",
    async (page) => {
        const mod = await setup(page);
        await page.evaluate(() => window.berylStage.skip());
        await page.keyboard.press(mod + "+i");
        const sides = field(page, "Sides");
        assert.equal(await sides.inputValue(), "6");
        await sides.fill("12");
        await page.keyboard.press("Enter");
        const count = field(page, "Dice");
        assert.equal(await count.inputValue(), "1");
        await count.fill("2");
        const dice = /^Rolled dice: (\d+, \d+)\.$/.exec(
            await announce(page, "Enter"),
        );
        assert.ok(dice);
        const banner = page.locator(".step-banner");
        await banner.getByText("Rolled dice (d12)", { exact: true }).waitFor();
        assert.equal(
            await banner.locator(".step-banner-detail").textContent(),
            dice[1],
        );

        await page.evaluate(() => window.berylStage.skip());
        const coin = /^Flipped a coin: (heads|tails)\.$/.exec(
            await announce(page, mod + "+Shift+i"),
        );
        assert.ok(coin);
        await banner.getByText(new RegExp(`^${coin[1]}$`, "i")).waitFor();
    },
    live,
);

await check(
    "manual table players target the pointed life total and reduce life by shown power",
    async (page) => {
        // The live server has one seat. Project a second seat for the browser
        // and observe the request addressed to it. The server can apply the
        // reduction from the opposing creature to the real seat.
        await page.route("**/invoke/table_current", async (route) => {
            const response = await route.fetch();
            const reply = await response.json();
            const view = reply.ok.view;
            view.table.seats = 2;
            view.players.opponent.life = 20;
            for (const card of view.objects) {
                if (card.zone === "battlefield") card.owner = "opponent";
            }
            delete view.script;
            await route.fulfill({ response, json: reply });
        });
        const moveCreature = (view) => ({
            move: {
                to: "battlefield",
                ...tableActionRef(view, {
                    kind: "object",
                    slot: view.table.orders.you.hand.at(-1),
                }),
            },
        });
        const setPower = (index, power) => (view) => ({
            stats: {
                stats: [power, 3],
                ...tableActionRef(view, {
                    kind: "object",
                    slot: view.battlefield[index],
                }),
            },
        });
        const mod = await setup(page, [
            moveCreature,
            moveCreature,
            setPower(0, 3),
            setPower(1, -2),
        ]);
        const initial = await tableView();
        for (const slot of initial.battlefield) {
            await page
                .locator(`.card-layer > [data-slot="${slot}"]`)
                .click({ modifiers: [mod] });
        }
        await life(page, "opponent").hover();
        const response = page.waitForResponse((response) =>
            response.url().endsWith("/invoke/table_act"),
        );
        await page.keyboard.press("=");
        const request = (await response).request();
        assert.deepEqual(request.postDataJSON().request.action, {
            life: { seat: 1, add: 1 },
        });

        const after = await sendKey(page, mod + "+Shift+l");
        assert.equal(after.players.you.life, initial.players.you.life - 3);
    },
    live,
);
