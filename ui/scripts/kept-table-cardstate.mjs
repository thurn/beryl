// The generic audits check layout. These interactions also verify the host's
// stats, counters, token printings and action references.
import assert from "node:assert/strict";
import {
    clippedText,
    crowdedControls,
    screenFaults,
    stageFaults,
} from "./capture.mjs";
import { check } from "./kept-harness.mjs";
import {
    clickTable,
    clickTableResult,
    startTable,
    tableActionRef,
    tableCommand,
    tableRoute,
    tableView,
} from "./table.mjs";

const tabletop = { route: tableRoute, ready: ".start-panel", serial: true };
const mod = async (page) =>
    (await page.evaluate(() => /Mac/.test(navigator.platform)))
        ? "Meta"
        : "Control";
const card = (page, slot) =>
    page.locator(`.card-layer > [data-slot="${slot}"]`);
const object = (view, slot) => view.objects.find((item) => item.slot === slot);
const tokens = (view) => view.objects.filter((item) => item.token);
const row = (page, name) => page.getByRole("menuitem", { name, exact: true });
const field = (page, name) => page.getByRole("textbox", { name, exact: true });
const moveHand = (view) => ({
    move: {
        ...tableActionRef(view, {
            kind: "object",
            slot: view.table.orders.you.hand.at(-1),
        }),
        to: "battlefield",
    },
});
const keyAction = async (page, key) => {
    const [view] = await Promise.all([
        clickTableResult(page),
        page.keyboard.press(key),
    ]);
    assert.ok(view.table, JSON.stringify(view));
    return view;
};
const enter = async (page, name, value) => {
    await field(page, name).fill(value);
    return keyAction(page, "Enter");
};

await check(
    "manual table card state edits stats counters and notes",
    async (page) => {
        await startTable(page, "24 Llanowar Elves", [moveHand]);
        const command = await mod(page);
        const [slot] = (await tableView()).battlefield;
        const permanent = card(page, slot);
        await permanent.click({ modifiers: [command] });
        for (const [key, power, toughness] of [
            [command + "+=", 2, 1],
            ["Alt+=", 2, 2],
            [command + "+Alt+=", 3, 3],
            [command + "+-", 2, 3],
            ["Alt+-", 2, 2],
            [command + "+Alt+-", 1, 1],
        ]) {
            assert.deepEqual(object(await keyAction(page, key), slot).pt, {
                power,
                toughness,
            });
        }

        for (const [text, stat] of [
            ["3/3", 3],
            ["+1/+1", 4],
        ]) {
            await page.keyboard.press(command + "+p");
            const view = await enter(page, "Power and toughness", text);
            assert.deepEqual(object(view, slot).pt, {
                power: stat,
                toughness: stat,
            });
        }
        assert.deepEqual(
            object(await keyAction(page, command + "+Alt+0"), slot).pt,
            {
                power: 1,
                toughness: 1,
            },
        );

        const boosted = object(await keyAction(page, "Alt+."), slot);
        assert.deepEqual(boosted.pt, { power: 2, toughness: 2 });
        await keyAction(page, "Alt+,");
        await page.keyboard.press("Alt+/");
        await enter(page, "Set first counter", "1");
        await keyAction(page, command + "+.");
        await keyAction(page, command + "+,");
        await page.keyboard.press(command + "+/");
        await enter(page, "Set second counter", "1");
        await keyAction(page, command + "+Shift+.");
        await keyAction(page, command + "+Shift+,");
        await page.keyboard.press(command + "+Shift+/");
        const set = object(await enter(page, "Set third counter", "3"), slot);
        assert.deepEqual(set.namedCounters, [{ name: "counter", count: 3 }]);

        await permanent.click({ button: "right" });
        await row(page, "Add counter…").click();
        const suggestions = await page
            .getByRole("group", { name: "Common counters" })
            .getByRole("button")
            .allTextContents();
        assert.deepEqual(suggestions, [
            "+1/+1",
            "−1/−1",
            "loyalty",
            "charge",
            "lore",
            "stun",
        ]);
        await enter(page, "Add counter", "shield");
        const incremented = object(
            await keyAction(page, command + "+Shift+a"),
            slot,
        );
        assert.equal(
            incremented.namedCounters.find(({ name }) => name === "counter")
                .count,
            4,
        );
        assert.ok(incremented.counters.every(({ count }) => count === 2));

        await page.keyboard.press("Alt+n");
        const note = await enter(page, "Note", "Hold for next turn");
        assert.equal(object(note, slot).note, "Hold for next turn");
        await permanent.getByText("Hold for next turn").waitFor();
    },
    tabletop,
);

await check(
    "manual table stats editor decrements a capped base stat by one",
    async (page) => {
        const max = 2147483647;
        const subject = (view) =>
            tableActionRef(view, { kind: "object", slot: view.battlefield[0] });
        await startTable(page, "24 Llanowar Elves", [
            moveHand,
            (view) => ({ stats: { ...subject(view), stats: [max, 1] } }),
            (view) => ({
                counter: { ...subject(view), name: "+1/+1", add: 1 },
            }),
        ]);
        const before = await tableView();
        const [slot] = before.battlefield;
        assert.equal(object(before, slot).pt.power, max);

        const command = await mod(page);
        await card(page, slot).click({ modifiers: [command] });
        await page.keyboard.press(command + "+p");
        const input = field(page, "Power and toughness");
        await input.waitFor();
        assert.equal(await input.inputValue(), `${max}/1`);
        const edited = await enter(page, "Power and toughness", "-1/1");
        assert.deepEqual(edited.table.cardCommands[slot].baseStats, {
            power: max - 1,
            toughness: 1,
        });
        assert.equal(object(edited, slot).pt.power, max);
    },
    tabletop,
);

await check(
    "manual table card state searches and repeats store and custom tokens",
    async (page) => {
        await startTable(page, "24 Academy Manufactor", [moveHand]);
        const command = await mod(page);
        const before = await tableView();
        await card(page, before.battlefield[0]).click({ modifiers: [command] });
        await page.keyboard.press(command + "+t");
        const search = page.getByRole("combobox", { name: "Search tokens" });
        await search.waitFor();
        const related = page.getByRole("group", { name: "Related tokens" });
        assert.deepEqual(
            await related.getByRole("option").allTextContents(),
            before.table.deckTokens.map(({ name }) => name),
        );
        assert.deepEqual(await screenFaults(page), []);
        assert.deepEqual(await stageFaults(page), []);
        assert.deepEqual(await crowdedControls(page), []);
        assert.deepEqual(await clippedText(page), []);

        await search.fill("Soldier Token");
        const soldiers = page.getByRole("option", { name: /^Soldier Token/ });
        await soldiers.nth(1).waitFor();
        const storeHit = soldiers.nth(1);
        const loaded = await clickTable(page, storeHit);
        const [soldier] = tokens(loaded);
        const hits = await tableCommand("card_search", {
            request: { tokens: true, text: "Soldier Token", limit: 36 },
        });
        const secondSoldier = hits.filter(
            ({ name }) => name === "Soldier Token",
        )[1];
        assert.ok(soldier.tokenPrinting.endsWith(secondSoldier.scryfallId));
        assert.deepEqual(soldier.pt, {
            power: Number(secondSoldier.power),
            toughness: Number(secondSoldier.toughness),
        });
        assert.equal(tokens(await keyAction(page, command + "+g")).length, 2);

        await page.keyboard.press(command + "+t");
        await page
            .getByRole("option", { name: "Custom token", exact: true })
            .click();
        await field(page, "Name").fill("Test Pilot");
        await field(page, "Type line").fill("Token Creature — Pilot");
        await page.getByRole("checkbox", { name: "Blue" }).check();
        await field(page, "Power").fill("2");
        await field(page, "Toughness").fill("3");
        await field(page, "Note").fill("Can crew");
        const customView = await clickTable(
            page,
            page.getByRole("button", { name: "Create token", exact: true }),
        );
        const custom = tokens(customView).find(
            ({ name }) => name === "Test Pilot",
        );
        assert.deepEqual(custom.pt, { power: 2, toughness: 3 });
        assert.equal(custom.note, "Can crew");
        assert.equal(custom.typeLine, "Token Creature — Pilot");
        assert.deepEqual(custom.customColors, ["U"]);
        await card(page, custom.slot).getByText("Test Pilot").waitFor();
        const repeated = tokens(await keyAction(page, command + "+g")).filter(
            ({ name }) => name === "Test Pilot",
        );
        assert.equal(repeated.length, 2);
        assert.ok(repeated.every(({ note }) => note === "Can crew"));
    },
    { ...tabletop, viewport: { width: 1024, height: 600 } },
);

await check(
    "manual table card state uses deck and card token keys and rows",
    async (page) => {
        await startTable(page, "24 Academy Manufactor", [moveHand]);
        const command = await mod(page);
        const original = await tableView();
        const [slot] = original.battlefield;
        const permanent = card(page, slot);
        const [first, second] = original.table.deckTokens;
        const [deckToken] = tokens(await keyAction(page, "Alt+1"));
        assert.equal(deckToken.name, first.name);
        const deckTokens = tokens(await keyAction(page, "Alt+2"));
        assert.deepEqual(
            new Set(deckTokens.map(({ name }) => name)),
            new Set([first.name, second.name]),
        );

        await permanent.click({ button: "right" });
        const faceDown = tokens(
            await clickTable(page, row(page, `Token: ${first.name}`), {
                modifiers: ["Shift"],
            }),
        ).find(({ facing }) => facing === "down");
        assert.equal(faceDown.name, first.name);

        await permanent.click({ button: "right" });
        const attached = tokens(
            await clickTable(page, row(page, `Token: Attach to ${first.name}`)),
        ).find(({ attachedTo }) => attachedTo === slot);
        assert.equal(attached.name, first.name);
        const attachment = card(page, attached.slot);
        const box = await attachment.boundingBox();
        await attachment.click({
            modifiers: [command],
            position: { x: box.width * 0.96, y: box.height * 0.65 },
        });
        assert.equal(
            object(await keyAction(page, command + "+Alt+u"), attached.slot)
                .attachedTo,
            null,
        );
        await page.keyboard.press("Escape");
        await permanent.click({ modifiers: [command] });
        const all = tokens(await keyAction(page, command + "+Shift+t"));
        assert.equal(all.length, 4 + original.table.deckTokens.length);
        assert.ok(
            original.table.deckTokens.every(({ scryfallId }) =>
                all.some(({ tokenPrinting }) =>
                    tokenPrinting.endsWith(scryfallId),
                ),
            ),
        );

        const flags = object(await keyAction(page, "Alt+u"), slot);
        assert.equal(flags.doesntUntap, true);
        const cloned = await keyAction(page, command + "+j");
        assert.equal(tokens(cloned).length, all.length + 1);
        const turned = await keyAction(page, "Alt+f");
        assert.equal(object(turned, slot).facing, "down");
        const peek = page.waitForRequest((request) =>
            request.url().endsWith("/invoke/table_act"),
        );
        await keyAction(page, "Alt+l");
        assert.deepEqual(
            (await peek).postDataJSON().request.action.peek,
            tableActionRef(turned, { kind: "object", slot }),
        );
        assert.equal(object(await keyAction(page, "Alt+f"), slot).facing, "up");
    },
    tabletop,
);
