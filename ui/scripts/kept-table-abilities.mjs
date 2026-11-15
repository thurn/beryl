// An ability menu choice must reach the live host as one logged action and
// publish the resulting marker and its animation to both seats.
import assert from "node:assert/strict";
import { check } from "./kept-harness.mjs";
import {
    clickAnswer,
    clickTable,
    pair,
    showTableView,
    tableActionRef,
    tableCommand,
    tableRoute,
    tableView,
} from "./table.mjs";

const walker = "Jace, the Perfected Mind";
const land = "Darksteel Citadel";
const field = (view, name) =>
    view.objects.find(
        (object) => object.zone === "battlefield" && object.name === name,
    );
const card = (page, slot) =>
    page.locator(`.card-layer > [data-slot="${slot}"]`);
const row = (page, name) => page.getByRole("menuitem", { name });
const ref = (view, slot) => tableActionRef(view, { kind: "object", slot });
const loyalty = (view) =>
    field(view, walker).counters.find(({ kind }) => kind === "loyalty")?.count;
const act = async (action) =>
    assert.deepEqual(
        await tableCommand("table_act", { request: { action } }, "host"),
        { ok: true },
    );

async function setup() {
    for (const [seat, main] of [
        [
            "host",
            [
                [walker, 12],
                [land, 12],
            ],
        ],
        ["guest", [["Island", 24]]],
    ]) {
        await tableCommand(
            "table_lobby",
            { request: { change: { kind: "load_deck", main, side: [] } } },
            seat,
        );
    }
    await act({ start: { first: 0 } });
    // Draw the whole deck so the setup works with every shuffled opening.
    await act({ draw: { seat: 0, count: 17 } });
    for (const name of [walker, land]) {
        const view = await tableView("host");
        const { slot } = view.objects.find(
            (object) => object.zone === "hand" && object.name === name,
        );
        await act({ move: { to: "battlefield", ...ref(view, slot) } });
    }
    const view = await tableView("host");
    await act({
        counter: {
            name: "loyalty",
            set: 5,
            ...ref(view, field(view, walker).slot),
        },
    });
}

function expectMarker(view, source, text) {
    assert.equal(view.stack.length, 1);
    const [marker] = view.stack;
    assert.equal(marker.kind, "ability");
    assert.equal(marker.source, field(view, source).slot);
    assert.equal(marker.name, source);
    assert.equal(marker.text, text);
    const emerge = view.script.beats
        .flat()
        .find(({ act }) => act.verb === "emerge");
    assert.equal(emerge?.act.id, marker.manual.id);
    assert.equal(emerge?.act.item.source, marker.source);
    return marker.manual.id;
}

await check(
    "manual table ability menus pay loyalty and place resolvable markers",
    async (page) => {
        const { host, guest } = await pair(page);
        try {
            await setup();
            const before = await tableView("host");
            await showTableView(host, before);
            await showTableView(guest, before);
            const walkerSlot = field(before, walker).slot;
            const permanent = card(host, walkerSlot);
            const plus = row(host, /^\+1: Until your next turn/);
            await permanent.click({ button: "right" });
            await row(host, "Every action…").waitFor();
            assert.equal(await plus.count(), 0);
            assert.equal(
                before.table.cardCommands[walkerSlot].abilities,
                undefined,
            );
            await host.keyboard.press("Escape");

            const choose = (source, row, x = null, seat = "host") =>
                tableCommand(
                    "table_answer",
                    {
                        request: {
                            answer: { kind: "activateAbility", source, row, x },
                        },
                    },
                    seat,
                );
            const oldRef = ref(before, walkerSlot).objects[0];
            assert.deepEqual(await choose(oldRef, 0), {
                refused: "permission",
            });
            await act({ set_assist: { assist: "ability_menus", on: true } });
            const current = await tableView("host");
            await showTableView(host, current);
            await showTableView(guest, current);
            const currentRef = ref(current, walkerSlot).objects[0];
            assert.deepEqual(await choose(oldRef, 0), { refused: "stale" });
            assert.deepEqual(await choose(currentRef, 2), {
                refused: "invalid",
            });
            const guestBefore = await tableView("guest");
            assert.equal(
                guestBefore.table.cardCommands[field(guestBefore, walker).slot]
                    .abilities,
                undefined,
            );

            await permanent.click({ button: "right" });
            const increased = await clickAnswer(host, plus, "activateAbility");
            assert.equal(loyalty(increased), 6);
            assert.equal(
                increased.table.log.length,
                current.table.log.length + 1,
            );
            const text =
                "Until your next turn, up to one target creature gets -3/-0.";
            const marker = expectMarker(increased, walker, text);
            await showTableView(guest, increased);
            assert.equal(
                expectMarker(await tableView("guest"), walker, text),
                marker,
            );

            const markerNode = host.locator(`[data-stage="a${marker}"]`);
            await markerNode.hover();
            await host
                .getByLabel(`${walker}, ability preview`)
                .getByText(text, { exact: true })
                .waitFor();
            assert.deepEqual((await clickTable(host, markerNode)).stack, []);

            const chooseX = async () => {
                await permanent.click({ button: "right" });
                await row(host, /^−X: Target player/).click();
            };
            const x = host.getByRole("textbox", { name: "Choose X" });
            await chooseX();
            await x.press("Escape");
            await x.waitFor({ state: "hidden" });
            assert.deepEqual((await tableView("host")).stack, []);
            await chooseX();
            await x.fill("2");
            const reduced = await clickAnswer(
                host,
                host.getByRole("button", { name: "Set X to 2", exact: true }),
                "activateAbility",
            );
            assert.equal(loyalty(reduced), 4);
            const variable = expectMarker(
                reduced,
                walker,
                "Target player mills three times X cards.",
            );
            await clickTable(host, host.locator(`[data-stage="a${variable}"]`));

            await card(host, field(reduced, land).slot).click({
                button: "right",
            });
            const mana = await clickAnswer(
                host,
                row(host, "{T}: Add {C}."),
                "activateAbility",
            );
            expectMarker(mana, land, "Add {C}.");
            assert.equal(field(mana, land).tapped, false);
        } finally {
            await tableCommand("table_leave", {}, "guest");
            await tableCommand("table_leave", {}, "host");
            await guest.close();
        }
    },
    {
        route: (page) => tableRoute(page, false, "host"),
        ready: ".start-panel",
        serial: true,
        pages: 2,
    },
);
