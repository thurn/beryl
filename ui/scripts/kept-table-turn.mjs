// The stage audit sees the lit icons and the button's placement. These checks
// also compare them with the host's phase and verify the actions they send.
import assert from "node:assert/strict";
import { check } from "./kept-harness.mjs";
import {
    clickTable,
    clickTableResult,
    showTableView,
    startTable,
    tableActionRef,
    tableRoute,
    tableView,
} from "./table.mjs";

const deck = "24 Island";
const tabletop = { route: tableRoute, ready: ".start-panel", serial: true };
const step = (page, name, side = "you") =>
    page.locator(`.step-stops-${side} [data-step="${name}"] button`);
const current = (page) => page.locator(".step-current");
const next = (page) => page.getByRole("button", { name: "Next", exact: true });
const phase = (phase) => ({ phase: { phase } });
const ref = (view, slot) => tableActionRef(view, { kind: "object", slot });
const moveTappedLand = (view) => ({
    move: {
        to: "battlefield",
        tapped: true,
        ...ref(view, view.table.orders.you.hand[0]),
    },
});

async function press(page, key) {
    const [view] = await Promise.all([
        clickTableResult(page),
        page.keyboard.press(key),
    ]);
    assert.ok(view.table, JSON.stringify(view));
    return view;
}

const lands = (view) =>
    view.objects.filter((object) => object.zone === "battlefield");

await check(
    "manual table turn lights the active step and lets either track set the phase",
    async (page) => {
        await startTable(page, deck, [phase("upkeep")]);
        assert.equal(await current(page).getAttribute("data-step"), "upkeep");
        assert.equal(await page.locator(".life-orb-active").count(), 1);

        const damage = step(page, "damage", "opponent");
        assert.equal(await damage.getAttribute("aria-pressed"), null);
        const after = await clickTable(page, damage);
        assert.equal(after.table.phase, "combatDamage");
        assert.equal(await current(page).getAttribute("data-step"), "damage");
        assert.equal(
            await current(page).locator("..").getAttribute("class"),
            "step-stops step-stops-you",
        );

        const end = await clickTable(page, step(page, "cleanup"));
        assert.equal(end.table.phase, "end");
        assert.equal(await current(page).getAttribute("data-step"), "end");
    },
    tabletop,
);

await check(
    "manual table turn current Untap untaps eligible permanents",
    async (page) => {
        await startTable(page, deck, [
            moveTappedLand,
            moveTappedLand,
            (view) => ({
                flag: { doesnt_untap: true, ...ref(view, lands(view)[0].slot) },
            }),
            phase("upkeep"),
        ]);
        const entered = await clickTable(page, step(page, "untap"));
        assert.ok(lands(entered).every(({ tapped }) => tapped));
        const untapped = await clickTable(page, step(page, "untap"));
        assert.equal(untapped.table.phase, "untap");
        assert.equal(lands(untapped).length, 2);
        for (const land of lands(untapped))
            assert.equal(!!land.tapped, !!land.doesntUntap);
        const repeated = await clickTable(page, step(page, "untap"));
        assert.deepEqual(lands(repeated), lands(untapped));
    },
    tabletop,
);

await check(
    "manual table turn current Draw draws only from the lit track",
    async (page) => {
        await startTable(page, deck, [phase("upkeep")]);
        const count = (await tableView()).table.orders.you.hand.length;
        const entered = await clickTable(page, step(page, "draw"));
        assert.equal(entered.table.phase, "draw");
        assert.equal(entered.table.orders.you.hand.length, count);
        const other = await clickTable(page, step(page, "draw", "opponent"));
        assert.equal(other.table.orders.you.hand.length, count);
        const drawn = await clickTable(page, step(page, "draw"));
        assert.equal(drawn.table.orders.you.hand.length, count + 1);
    },
    tabletop,
);

for (const key of ["Space", "Tab"]) {
    await check(
        `manual table turn ${key} advances without drawing`,
        async (page) => {
            await startTable(page, deck, [phase("upkeep")]);
            const before = await tableView();
            const after = await press(page, key);
            assert.equal(after.table.phase, "draw");
            assert.deepEqual(
                after.table.orders.you.hand,
                before.table.orders.you.hand,
            );
        },
        tabletop,
    );
}

await check(
    "manual table turn Shift+Tab enters Draw and draws a card",
    async (page) => {
        await startTable(page, deck, [phase("upkeep")]);
        const before = await tableView();
        const after = await press(page, "Shift+Tab");
        assert.equal(after.table.phase, "draw");
        assert.equal(
            after.table.orders.you.hand.length,
            before.table.orders.you.hand.length + 1,
        );
    },
    tabletop,
);

await check(
    "manual table turn Shift+Tab wraps to Untap and untaps",
    async (page) => {
        await startTable(page, deck, [moveTappedLand, phase("end")]);
        const before = await tableView();
        await showTableView(page, before);
        const after = await press(page, "Shift+Tab");
        assert.equal(after.table.phase, "untap");
        assert.equal(after.turn.number, before.turn.number);
        assert.equal(lands(after).length, 1);
        assert.equal(lands(after)[0].tapped, false);
    },
    tabletop,
);

for (const [key, destination, icon] of [
    ["F5", "untap", "untap"],
    ["F6", "draw", "draw"],
    ["F7", "firstMain", "main1"],
    ["F8", "beginningOfCombat", "beginCombat"],
    ["F9", "secondMain", "main2"],
    ["F10", "end", "end"],
]) {
    await check(
        `manual table turn ${key} jumps to ${destination}`,
        async (page) => {
            await startTable(page, deck, [phase("upkeep")]);
            const after = await press(page, key);
            assert.equal(after.table.phase, destination);
            assert.equal(await current(page).getAttribute("data-step"), icon);
        },
        tabletop,
    );
}

await check(
    "manual table turn Cmd+Return passes a solo turn back to the same player",
    async (page) => {
        await startTable(page, deck, [moveTappedLand, phase("second_main")]);
        const before = await tableView();
        const mod = (await page.evaluate(() => /Mac/.test(navigator.platform)))
            ? "Meta"
            : "Control";
        const after = await press(page, `${mod}+Enter`);
        assert.equal(after.turn.number, before.turn.number + 1);
        assert.equal(after.turn.active, "you");
        assert.equal(after.table.phase, "untap");
        assert.equal(lands(after)[0].tapped, true);
        assert.equal(await current(page).getAttribute("data-step"), "untap");
    },
    tabletop,
);

await check(
    "manual table turn Next labels and advances every phase",
    async (page) => {
        await startTable(page, deck, [phase("untap")]);
        const before = await tableView();
        for (const [destination, caption] of [
            ["upkeep", "Upkeep"],
            ["draw", "Draw"],
            ["firstMain", "Main 1"],
            ["beginningOfCombat", "Beginning of combat"],
            ["declareAttackers", "Attackers"],
            ["declareBlockers", "Blockers"],
            ["combatDamage", "Combat damage"],
            ["endOfCombat", "End of combat"],
            ["secondMain", "Main 2"],
            ["end", "End step"],
            ["untap", "Untap"],
        ]) {
            assert.equal(
                await next(page)
                    .locator("..")
                    .locator(".turn-caption")
                    .textContent(),
                caption,
            );
            const after = await clickTable(page, next(page));
            assert.equal(after.table.phase, destination);
            assert.equal(after.turn.number, before.turn.number);
        }
    },
    tabletop,
);
