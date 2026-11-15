// These checks need the live host to recompute previews and both seats to
// receive the resulting action, animation script, and undo.
import assert from "node:assert/strict";
import { center, check } from "./kept-harness.mjs";
import {
    clickAnswer,
    clickTable,
    clickTableResult,
    pair,
    showTableView,
    tableActionRef,
    tableCommand,
    tableRoute,
    tableView,
} from "./table.mjs";

const live = {
    route: (page) => tableRoute(page, false, "host"),
    ready: ".start-panel",
    serial: true,
    pages: 2,
};
const mod = process.platform === "darwin" ? "Meta" : "Control";
const button = (page, name) => page.getByRole("button", { name, exact: true });
const checkboxes = (panel) => panel.getByRole("checkbox");
const upkeep = (page) => page.getByRole("region", { name: "Turn upkeep" });
const combat = (page) => page.getByRole("region", { name: "Combat" });
const life = (page, side) => page.locator(`[data-life="${side}"]`);
const card = (page, slot) =>
    page.locator(`.card-layer > [data-slot="${slot}"]`);
const named = (view, name) =>
    view.objects.filter((object) => object.name === name);
const field = (view, name) =>
    named(view, name).filter(({ zone }) => zone === "battlefield");
const reference = (view, slot) =>
    tableActionRef(view, { kind: "object", slot });

async function assertLines(panel, names) {
    assert.equal(await checkboxes(panel).count(), names.length);
    for (const name of names) {
        assert.ok(
            await panel
                .getByRole("checkbox", { name, exact: true })
                .isVisible(),
            name,
        );
    }
}

async function act(seat, action) {
    assert.deepEqual(
        await tableCommand("table_act", { request: { action } }, seat),
        { ok: true },
    );
}

async function linkedCheck(title, run) {
    await check(
        `manual table assists ${title}`,
        async (page) => {
            const { host, guest } = await pair(page);
            try {
                await run(host, guest);
            } finally {
                await tableCommand("table_leave", {}, "guest");
                await tableCommand("table_leave", {}, "host");
                await guest.close();
            }
        },
        live,
    );
}

async function start(hostCard, guestCard) {
    for (const [seat, name] of [
        ["host", hostCard],
        ["guest", guestCard],
    ]) {
        await tableCommand(
            "table_lobby",
            {
                request: {
                    change: { kind: "load_deck", main: [[name, 24]], side: [] },
                },
            },
            seat,
        );
    }
    await act("host", { start: { first: 0 } });
}

async function show(page, seat) {
    // The host can finish a command before the guest receives its update.
    const host = await tableView("host");
    await showTableView(page, host);
    return seat === "host" ? host : tableView(seat);
}

async function play(page, seat, tapped = false) {
    const view = await show(page, seat);
    await act(seat, {
        move: {
            to: "battlefield",
            tapped,
            ...reference(view, view.table.orders.you.hand[0]),
        },
    });
}

async function enable(page, name) {
    await button(page, "Settings").click();
    await button(page, "Assists").click();
    const dialog = page.getByRole("dialog", { name: "Assists" });
    await clickTable(page, dialog.getByRole("switch", { name }));
    await button(dialog, "Close").click();
}

async function press(page, key) {
    const result = clickTableResult(page);
    await page.keyboard.press(key);
    return result;
}

async function undoBatch(page, view) {
    const open = button(page, "Show log");
    if (await open.isVisible()) await open.click();
    const entry = page.locator(`[data-log-entry="${view.table.log.at(-1).n}"]`);
    const result = await clickTable(page, button(entry, "Undo"));
    const close = button(page, "Hide log");
    if (await close.isVisible()) await close.click();
    return result;
}

async function drawArrow(page, source, target) {
    const from = center(await source.boundingBox());
    const to = center(await target.boundingBox());
    await page.mouse.move(from.x, from.y);
    await page.mouse.down({ button: "right" });
    await page.mouse.move(to.x, to.y, { steps: 4 });
    const result = clickTableResult(page);
    await page.mouse.up({ button: "right" });
    return result;
}

async function removeBlockArrow(page) {
    const at = await page.locator('[data-line^="block "]').evaluate((path) => {
        const point = path.getPointAtLength(path.getTotalLength() / 2);
        const { x, y } = point.matrixTransform(path.getScreenCTM());
        return { x, y };
    });
    const result = clickTableResult(page);
    await page.mouse.click(at.x, at.y, { button: "right" });
    return result;
}

function combatState(view) {
    const [attacker] = field(view, "Grizzly Bears");
    const [blocker] = named(view, "Llanowar Elves").filter(
        ({ zone }) => zone === "battlefield" || zone === "graveyard",
    );
    return {
        damage: attacker.damage,
        blocker: blocker.zone,
        owner: blocker.owner,
        phase: view.table.phase,
        arrows: view.table.arrows,
        life: [view.players.you.life, view.players.opponent.life],
    };
}

await linkedCheck(
    "preview blocks and let either seat apply and undo combat",
    async (host, guest) => {
        await start("Grizzly Bears", "Llanowar Elves");
        await play(host, "host");
        await play(guest, "guest");
        await act("host", { phase: { phase: "declare_attackers" } });
        const hostView = await show(host, "host");
        await show(guest, "guest");
        await enable(host, "Combat");
        assert.equal(await combat(host).count(), 0);
        const attack = await drawArrow(
            host,
            card(host, field(hostView, "Grizzly Bears")[0].slot),
            life(host, "opponent"),
        );
        assert.deepEqual(
            attack.table.arrows.map(({ arrow }) => arrow),
            ["attack"],
        );
        const unblocked = [
            "Seat 2 loses 2 theirs",
            "Go to end of combat yours",
        ];
        await assertLines(combat(host), unblocked);

        await show(guest, "guest");
        assert.equal(await combat(guest).count(), 0);
        await enable(guest, "Combat");
        await act("host", { phase: { phase: "declare_blockers" } });
        const guestView = await show(guest, "guest");
        const block = () =>
            drawArrow(
                guest,
                card(guest, field(guestView, "Llanowar Elves")[0].slot),
                card(guest, field(guestView, "Grizzly Bears")[0].slot),
            );

        await block();
        const blocked = await show(host, "host");
        assert.deepEqual(
            blocked.table.arrows.map(({ arrow }) => arrow),
            ["attack", "block"],
        );
        await assertLines(combat(host), [
            "Grizzly Bears takes 1 yours",
            "Llanowar Elves dies theirs",
            "Go to end of combat yours",
        ]);
        await assertLines(combat(guest), [
            "Grizzly Bears takes 1 theirs",
            "Llanowar Elves dies yours",
            "Go to end of combat theirs",
        ]);
        assert.equal(
            blocked.table.proposals[0].id,
            attack.table.proposals[0].id,
        );
        assert.equal(field(blocked, "Grizzly Bears")[0].damage, 0);
        assert.equal(field(blocked, "Llanowar Elves").length, 1);

        await removeBlockArrow(guest);
        await show(host, "host");
        await assertLines(combat(host), unblocked);
        await block();
        const before = await show(host, "host");
        const guestBefore = await show(guest, "guest");

        const applied = await clickAnswer(
            guest,
            button(combat(guest), "Apply"),
            "applyProposal",
        );
        const hostAfter = await show(host, "host");
        assert.equal(hostAfter.table.log.length, before.table.log.length + 1);
        for (const [view, owner] of [
            [hostAfter, "opponent"],
            [applied, "you"],
        ]) {
            assert.equal(combatState(view).damage, 1);
            assert.equal(combatState(view).blocker, "graveyard");
            assert.equal(combatState(view).owner, owner);
            assert.equal(view.table.phase, "endOfCombat");
            assert.deepEqual(view.table.proposals, []);
        }
        const restored = await undoBatch(host, hostAfter);
        assert.equal(restored.table.log.length, hostAfter.table.log.length + 1);
        assert.deepEqual(combatState(restored), combatState(before));
        assert.deepEqual(
            combatState(await show(guest, "guest")),
            combatState(guestBefore),
        );

        const disabled = combat(host).getByRole("checkbox", {
            name: "Grizzly Bears takes 1 yours",
        });
        const switched = await clickAnswer(host, disabled, "proposalLine");
        assert.equal(switched.table.log.length, restored.table.log.length);
        await showTableView(host, switched, switched.table.proposals[0]);
        assert.equal(await disabled.isChecked(), false);
        assert.equal(await combat(guest).locator("input:checked").count(), 3);
        const partial = await clickAnswer(
            host,
            button(combat(host), "Apply"),
            "applyProposal",
        );
        assert.equal(partial.table.log.length, restored.table.log.length + 1);
        assert.equal(combatState(partial).damage, 0);
        assert.equal(combatState(partial).blocker, "graveyard");
        assert.equal(partial.table.phase, "endOfCombat");
        const guestUndo = await undoBatch(guest, await show(guest, "guest"));
        assert.equal(guestUndo.table.log.length, partial.table.log.length + 1);
        assert.deepEqual(combatState(guestUndo), combatState(guestBefore));
        assert.deepEqual(
            combatState(await show(host, "host")),
            combatState(before),
        );

        // With the block gone, one attacker hits the player and one hits a planeswalker.
        await removeBlockArrow(guest);
        await play(host, "host");
        await act("guest", {
            token: {
                count: 1,
                card: {
                    custom: {
                        name: "Walker",
                        types: "Legendary Planeswalker",
                        colors: [],
                    },
                },
            },
        });
        const walkerView = await tableView("guest");
        await act("guest", {
            counter: {
                ...reference(walkerView, field(walkerView, "Walker")[0].slot),
                name: "loyalty",
                set: 4,
            },
        });
        const unblockedView = await show(host, "host");
        const [extra] = field(unblockedView, "Grizzly Bears").filter(
            ({ slot }) => slot !== field(hostView, "Grizzly Bears")[0].slot,
        );
        await drawArrow(
            host,
            card(host, extra.slot),
            card(host, field(unblockedView, "Walker")[0].slot),
        );
        await assertLines(combat(host), [
            "Walker loses 2 loyalty theirs",
            ...unblocked,
        ]);
        const tally = (view) => ({
            loyalty: field(view, "Walker")[0].counters.find(
                ({ kind }) => kind === "loyalty",
            ).count,
            life: view.players.you.life,
            phase: view.table.phase,
        });
        const beforeHits = await show(guest, "guest");
        await clickAnswer(host, button(combat(host), "Apply"), "applyProposal");
        const hit = await show(guest, "guest");
        assert.deepEqual(tally(hit), {
            life: 18,
            loyalty: 2,
            phase: "endOfCombat",
        });
        assert.deepEqual(tally(await undoBatch(guest, hit)), tally(beforeHits));
    },
);

await linkedCheck(
    "offer upkeep on turn pass and play its untap and draw together",
    async (host, guest) => {
        await enable(host, "Turn upkeep");
        await enable(guest, "Turn upkeep");
        await start("Island", "Mountain");
        const first = await show(host, "host");
        await show(guest, "guest");
        assert.equal(first.turn.number, 1);
        await assertLines(upkeep(host), ["Go to first main yours"]);
        assert.equal(await upkeep(guest).count(), 0);
        const firstMain = await clickAnswer(
            host,
            button(upkeep(host), "Apply"),
            "applyProposal",
        );
        assert.equal(firstMain.table.phase, "firstMain");
        assert.deepEqual(
            firstMain.table.orders.you.hand,
            first.table.orders.you.hand,
        );

        await play(guest, "guest", true);
        await play(guest, "guest", true);
        const lands = field(await tableView("guest"), "Mountain");
        await act("guest", {
            flag: {
                doesnt_untap: true,
                ...reference(await tableView("guest"), lands[1].slot),
            },
        });
        await show(host, "host");
        const passed = await press(host, mod + "+Enter");
        assert.equal(passed.turn.active, "opponent");
        assert.equal(passed.table.phase, "untap");
        assert.equal(await upkeep(host).count(), 0);
        const pending = await show(guest, "guest");
        assert.equal(pending.turn.number, 2);
        await assertLines(upkeep(guest), [
            "Untap Mountain yours",
            "Draw a card yours",
            "Go to first main yours",
        ]);
        assert.equal(
            field(pending, "Mountain").filter(({ tapped }) => tapped).length,
            2,
        );

        const applied = await clickAnswer(
            guest,
            button(upkeep(guest), "Apply"),
            "applyProposal",
        );
        assert.equal(applied.table.phase, "firstMain");
        assert.equal(applied.table.log.length, pending.table.log.length + 1);
        assert.equal(
            applied.table.orders.you.hand.length,
            pending.table.orders.you.hand.length + 1,
        );
        const [untapped, skipped] = field(applied, "Mountain");
        assert.equal(untapped.tapped, false);
        assert.equal(skipped.tapped, true);
        assert.equal(skipped.doesntUntap, true);
        const motions = applied.script.beats.flat().map(({ act }) => act);
        assert.equal(
            motions.filter(
                ({ verb, object }) =>
                    verb === "tap" &&
                    object.slot === untapped.slot &&
                    !object.tapped,
            ).length,
            1,
        );
        assert.equal(
            motions.filter(({ verb, why }) => verb === "fly" && why === "draw")
                .length,
            1,
        );
        await upkeep(guest).waitFor({ state: "hidden" });
        assert.equal((await show(host, "host")).table.phase, "firstMain");
    },
);

await linkedCheck(
    "apply card nudges and keep concede private",
    async (host, guest) => {
        const walker = "Teferi, Hero of Dominaria";
        await start("Llanowar Elves", walker);
        await show(host, "host");
        await show(guest, "guest");
        await enable(host, "State nudges");
        await enable(guest, "State nudges");
        await play(host, "host");
        const [elf] = field(await show(host, "host"), "Llanowar Elves");
        const creature = card(host, elf.slot);
        const lethal = button(creature, "Move to graveyard");
        assert.equal(await lethal.count(), 0);
        await creature.click({ button: "right" });
        await host.getByRole("menuitem", { name: "Every action…" }).click();
        await host
            .getByRole("dialog", { name: "Command list" })
            .getByRole("option", { name: "Set damage…" })
            .click();
        const damage = host.getByRole("textbox", { name: "Set damage" });
        const [marked] = await Promise.all([
            clickTableResult(host),
            damage.press("Enter"),
        ]);
        assert.equal(field(marked, "Llanowar Elves")[0].damage, 1);
        const buried = await clickAnswer(host, lethal, "applyLabel");
        assert.equal(field(buried, "Llanowar Elves").length, 0);
        assert.equal(
            named(buried, "Llanowar Elves").filter(
                ({ zone }) => zone === "graveyard",
            ).length,
            1,
        );

        await play(guest, "guest");
        await play(guest, "guest");
        const duplicate = await show(guest, "guest");
        await show(host, "host");
        const [keep, remove] = field(duplicate, walker);
        await button(card(guest, keep.slot), "Legend rule").waitFor();
        const chosen = await clickAnswer(
            guest,
            button(card(guest, remove.slot), "Legend rule"),
            "applyLabel",
        );
        assert.deepEqual(
            field(chosen, walker).map(({ slot }) => slot),
            [keep.slot],
        );
        assert.equal(
            named(chosen, walker).filter(({ zone }) => zone === "graveyard")
                .length,
            1,
        );
        assert.equal(await button(guest, "Legend rule").count(), 0);

        await act("guest", {
            counter: {
                ...reference(chosen, keep.slot),
                name: "loyalty",
                set: 0,
            },
        });
        await show(guest, "guest");
        const empty = await clickAnswer(
            guest,
            button(card(guest, keep.slot), "No loyalty"),
            "applyLabel",
        );
        assert.equal(field(empty, walker).length, 0);
        assert.equal(
            named(empty, walker).filter(({ zone }) => zone === "graveyard")
                .length,
            2,
        );

        await act("guest", { life: { seat: 1, set: 0 } });
        const atZero = await show(guest, "guest");
        const hostView = await show(host, "host");
        assert.equal(atZero.players.you.life, 0);
        assert.equal(hostView.players.opponent.life, 0);
        assert.deepEqual(hostView.table.labels, []);
        assert.equal(
            await button(life(host, "opponent"), "Concede").count(),
            0,
        );
        const conceded = await clickAnswer(
            guest,
            button(life(guest, "you"), "Concede"),
            "applyLabel",
        );
        assert.equal(conceded.table.log.length, atZero.table.log.length + 1);
        assert.match(conceded.table.log.at(-1).text, /conced/i);
        assert.equal(conceded.turn.number, 0);
        assert.deepEqual(conceded.table.labels, []);
    },
);
