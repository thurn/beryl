// These checks need live host replies to tell which action was undone, what
// knowledge was kept, and which numbered entries block undo.
import assert from "node:assert/strict";
import { screenFaults } from "./capture.mjs";
import { check } from "./kept-harness.mjs";
import {
    clickTable,
    clickTableResult,
    startTable,
    tableActionRef,
    tableRoute,
    tableView,
} from "./table.mjs";

const deck = "12 Island\n12 Plains";
const live = { route: tableRoute, ready: ".start-panel", serial: true };
const last = (view) => view.table.log.at(-1);
const row = (page, n) => page.locator(`[data-log-entry="${n}"]`);
const ref = (view, slot) => tableActionRef(view, { kind: "object", slot });
const land = (view) => view.objects.find((card) => card.zone === "battlefield");
const permanent = (page, view) =>
    page.locator(`.card-layer > [data-slot="${land(view).slot}"]`);
const playLand = (view) => ({
    move: { to: "play", ...ref(view, view.table.orders.you.hand.at(-1)) },
});
const draw = { draw: { seat: 0, count: 1 } };
const shuffle = { shuffle: { seat: 0, portion: "all" } };
const topName = (view) =>
    view.objects.find(({ slot }) => slot === view.players.you.library.at(-1))
        .name;

async function key(page, chord) {
    const mod = await page.evaluate(() =>
        /Mac/.test(navigator.platform) ? "Meta" : "Control",
    );
    const [result] = await Promise.all([
        clickTableResult(page),
        page.keyboard.press(`${mod}+${chord}`),
    ]);
    return result;
}

async function openLog(page) {
    const open = page.getByRole("button", { name: "Show log" });
    if (await open.isVisible()) await open.click();
}

await check(
    "manual table log Cmd+Z and the corner button undo a tap",
    async (page) => {
        await startTable(page, deck, [playLand]);
        const card = permanent(page, await tableView());

        const tapped = await clickTable(page, card);
        assert.equal(land(tapped).tapped, true);
        const undone = await key(page, "z");
        assert.equal(land(undone).tapped, false);
        assert.match(last(undone).text, /^Undid: Tapped /);
        await openLog(page);
        const undoLine = row(page, last(undone).n);
        await undoLine.getByText(last(undone).text, { exact: false }).waitFor();
        assert.match(await undoLine.textContent(), /^\d+ You Undid: Tapped /);

        await clickTable(page, card);
        const corner = page.locator(".undo-button");
        const undoneByButton = await clickTable(page, corner);
        assert.equal(land(undoneByButton).tapped, false);
    },
    live,
);

await check(
    "manual table log Undo last draw restores a known card after a tap",
    async (page) => {
        await startTable(page, deck, [playLand, draw]);
        const before = await tableView();
        const drawn = before.table.orders.you.hand.at(-1);
        const name = before.objects.find(({ slot }) => slot === drawn).name;
        await clickTable(page, permanent(page, before));

        const after = await key(page, "Shift+d");
        assert.equal(land(after).tapped, true);
        assert.equal(
            after.table.orders.you.hand.length,
            before.table.orders.you.hand.length - 1,
        );
        assert.equal(topName(after), name);
        await openLog(page);
        await row(page, last(after).n)
            .getByText(/Undid: Drew 1 card \(seen\)/)
            .waitFor();
    },
    live,
);

await check(
    "manual table log rewinds a shuffle and strikes its line",
    async (page) => {
        await startTable(page, deck, [
            { look: { seat: 0, portion: { top: 1 } } },
            { close_look: {} },
        ]);
        const before = await tableView();
        const name = topName(before);
        assert.ok(name);

        const library = page.locator(
            'button[data-pile="library"][data-side="you"]',
        );
        await library.click({ button: "right" });
        const shuffled = await clickTable(
            page,
            page.getByRole("menuitem", { name: "Shuffle", exact: false }),
        );
        assert.equal(topName(shuffled) ?? "", "");
        await openLog(page);
        await row(page, last(before).n).click({ button: "right" });
        const restored = await clickTable(
            page,
            page.getByRole("menuitem", { name: "Rewind to here" }),
        );
        assert.equal(topName(restored), name);
        const shuffleLine = row(page, last(shuffled).n);
        const struck = shuffleLine.locator(".table-log-struck");
        await struck.waitFor();
        assert.equal(
            await struck.evaluate(
                (node) => getComputedStyle(node).textDecorationLine,
            ),
            "line-through",
        );
        assert.deepEqual(await screenFaults(page), []);

        await struck.hover();
        const restoredShuffle = await clickTable(
            page,
            shuffleLine.getByRole("button", { name: "Rewind to here" }),
        );
        assert.equal(
            restoredShuffle.table.log.find(({ n }) => n === last(shuffled).n)
                .struck,
            false,
        );
    },
    live,
);

await check(
    "manual table log a blocked draw undo opens the log and marks its blocker",
    async (page) => {
        await startTable(page, deck, [
            draw,
            playLand,
            { life: { seat: 0, add: 1 } },
        ]);
        const before = await tableView();
        const blocker = before.table.log.at(-2).n;

        const refusal = await key(page, "Shift+d");
        assert.deepEqual(refusal, {
            refused: "invalid",
            blocked_by: [blocker],
        });
        const marked = page.locator("[data-blocks-undo]");
        await marked.waitFor();
        assert.deepEqual(
            await marked.evaluateAll((nodes) =>
                nodes.map((node) => Number(node.dataset.logEntry)),
            ),
            [blocker],
        );
        assert.equal(
            await page
                .getByRole("button", { name: "Hide log" })
                .getAttribute("aria-expanded"),
            "true",
        );
        assert.deepEqual(await tableView(), before);
    },
    live,
);

await check(
    "manual table log a batch line undoes the whole batch",
    async (page) => {
        await startTable(page, deck, [
            { batch: { actions: [{ life: { seat: 0, add: -2 } }, draw] } },
        ]);
        const before = await tableView();
        await openLog(page);
        const after = await clickTable(
            page,
            row(page, last(before).n).getByRole("button", { name: "Undo" }),
        );
        assert.equal(after.players.you.life, before.players.you.life + 2);
        assert.equal(
            after.table.orders.you.hand.length,
            before.table.orders.you.hand.length - 1,
        );
        assert.match(last(after).text, /^Undid: /);
        assert.equal(
            await row(page, last(before).n)
                .getByRole("button", { name: "Undo" })
                .count(),
            0,
        );
    },
    live,
);
