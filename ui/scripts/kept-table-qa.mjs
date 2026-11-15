// A linked table can leak a name in an animation or log line even when the
// final card looks face down. Check the entire guest update and rendered DOM.
import assert from "node:assert/strict";
import { center, check, committed } from "./kept-harness.mjs";
import {
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
const names = [
    "Adarkar Wastes",
    "Ajani's Pridemate",
    "An Offer You Can't Refuse",
    "Arcane Signet",
    "Birds of Paradise",
    "Brainstorm",
    "Consider",
    "Counterspell",
    "Dark Ritual",
    "Doom Blade",
    "Elvish Mystic",
    "Evolving Wilds",
    "Faithless Looting",
    "Fabled Passage",
    "Giant Growth",
    "Opt",
    "Healing Salve",
    "Hero's Downfall",
    "Impulse",
    "Into the Roil",
    "Llanowar Elves",
    "Negate",
    "No More Lies",
    "Pacifism",
    "Path to Exile",
    "Ponder",
    "Raise the Alarm",
    "Serum Visions",
    "Swords to Plowshares",
    "Tranquil Cove",
    "Unsummon",
    "Wrath of God",
];

const card = (page, slot) =>
    page.locator(`.card-layer > [data-slot="${slot}"]`);
const pile = (page, zone) =>
    page.locator(`button[data-pile="${zone}"][data-side="you"]`);
const ref = (view, slot) => tableActionRef(view, { kind: "object", slot });
const object = (view, slot) =>
    view.objects.find((entry) => entry.slot === slot);
const lastHand = (view, side = "you") => view.table.orders[side].hand.at(-1);

function linked(run) {
    return async (page) => {
        const { host, guest } = await pair(page, "memory");
        try {
            await run(host, guest);
        } finally {
            for (const seat of ["guest", "host"]) {
                await tableCommand("table_leave", {}, seat);
            }
            await guest.close();
        }
    };
}

async function act(action, seat = "host") {
    assert.deepEqual(
        await tableCommand("table_act", { request: { action } }, seat),
        { ok: true },
    );
}

async function start(hostDeck, guestDeck) {
    for (const [seat, main] of [
        ["host", hostDeck],
        ["guest", guestDeck],
    ]) {
        await tableCommand(
            "table_lobby",
            {
                request: { change: { kind: "load_deck", main, side: [] } },
            },
            seat,
        );
    }
    await act({ start: { first: 0 } });
}

async function listen(page) {
    await page.evaluate(async () => {
        window.qaUpdates = [];
        await window.berylLive.listen("game-update", (update, past) => {
            if (!past && update.view.table) window.qaUpdates.push(update);
        });
    });
}

const flight = (update, from, to) =>
    update.view.script.beats
        .flat()
        .map(({ act }, index) => ({
            act,
            timing: update.view.script.timing[index],
        }))
        .find(
            ({ act }) =>
                act.verb === "fly" &&
                act.from === from &&
                act.object.zone === to,
        );

await check(
    "manual table QA keeps private names out of the guest for 30 actions",
    linked(async (host, guest) => {
        await Promise.all([listen(host), listen(guest)]);
        await start(
            names.map((name) => [name, 1]),
            [["Mountain", 24]],
        );
        const known = new Set();
        let consumed = 0;
        let actions = 0;
        let view = await tableView("host");
        await showTableView(host, view);

        async function inspect() {
            const logLength = view.table.log.length;
            await showTableView(guest, view);
            await committed(guest);
            const { payloads, dom } = await guest.evaluate(
                (from) => ({
                    payloads: window.qaUpdates.slice(from),
                    dom: document.body.textContent,
                }),
                consumed,
            );
            assert.ok(
                payloads.some(
                    ({ view }) => view.table.log.length === logLength,
                ),
                `guest received an update for log position ${logLength}`,
            );
            consumed += payloads.length;
            for (const text of [
                dom,
                ...payloads.map((payload) => JSON.stringify(payload)),
            ]) {
                for (const name of names) {
                    const word = new RegExp(`\\b${name}\\b`);
                    if (!known.has(name))
                        assert.ok(
                            !word.test(text),
                            `action ${actions}: ${text.slice(Math.max(0, text.search(word) - 30), text.search(word) + 60)}`,
                        );
                }
            }
            return payloads.findLast(
                ({ view }) => view.table.log.length === logLength,
            );
        }

        async function step(run) {
            await run();
            actions++;
            view = await tableView("host");
            return inspect();
        }

        await inspect();
        const drawn = await step(() => clickTable(host, pile(host, "library")));
        const guestFlight = flight(drawn, "library", "hand");
        assert.ok(guestFlight);
        assert.equal(guestFlight.act.object.name, null);
        const hostDraw = await host.evaluate(
            (length) =>
                window.qaUpdates.findLast(
                    ({ view }) => view.table.log.length === length,
                ),
            view.table.log.length,
        );
        const hostFlight = flight(hostDraw, "library", "hand");
        assert.ok(hostFlight?.act.object.name);
        assert.equal(hostFlight.timing.delay, guestFlight.timing.delay);
        await card(guest, guestFlight.act.object.slot).hover();
        await committed(guest);
        assert.equal(await guest.locator(".stage .inspect").count(), 0);

        const faceDown = await step(() =>
            clickTable(host, card(host, lastHand(view)), {
                modifiers: ["Shift"],
            }),
        );
        const hidden = faceDown.view.objects.find(
            ({ zone }) => zone === "battlefield",
        );
        assert.equal(hidden.face, "down");
        assert.equal(hidden.name, null);
        await card(guest, hidden.slot).hover();
        await committed(guest);
        assert.equal(await guest.locator(".stage .inspect").count(), 0);

        const looked = await step(() =>
            act({ look: { seat: 0, portion: { top: 3 } } }),
        );
        assert.equal(looked.view.table.orders.opponent.hand.length, 7);
        await step(() => act({ close_look: {} }));

        const top = view.players.you.library.at(-1);
        const topName = object(view, top).name;
        assert.ok(topName);
        known.add(topName);
        const revealed = await step(() =>
            act({
                reveal: { ...ref(view, top), to: [1] },
            }),
        );
        assert.ok(revealed.view.objects.some(({ name }) => name === topName));
        known.clear();
        const shuffled = await step(() =>
            act({ shuffle: { seat: 0, portion: "all" } }),
        );
        assert.ok(
            shuffled.view.players.opponent.library.every(
                (slot) => object(shuffled.view, slot)?.name == null,
            ),
        );

        for (let index = 0; index < 22; index++) {
            await step(() => act({ draw: { seat: 0, count: 1 } }));
        }

        for (const { zone, name } of view.objects) {
            if (zone === "hand") known.add(name);
        }
        const hand = await step(() =>
            act({ reveal: { in: { hand: 0 }, to: [1] } }),
        );
        const handSlot = lastHand(hand.view, "opponent");
        const handName = object(hand.view, handSlot).name;
        assert.ok(handName);
        const discarded = await step(() =>
            act(
                {
                    move: { ...ref(hand.view, handSlot), to: { graveyard: 0 } },
                },
                "guest",
            ),
        );
        assert.ok(
            discarded.view.objects.some(
                ({ name, zone }) => name === handName && zone === "graveyard",
            ),
        );
        assert.equal(actions, 30);
    }),
    live,
);

async function showBoth(host, guest, view = undefined) {
    const authority = view ?? (await tableView("host"));
    await Promise.all([
        showTableView(host, authority),
        showTableView(guest, authority),
    ]);
    await Promise.all([committed(host), committed(guest)]);
    return tableView("guest");
}

async function drag(page, from, to, { shift = false, button = "left" } = {}) {
    await page.mouse.move(from.x, from.y);
    await page.mouse.down({ button });
    await page.mouse.move(to.x, to.y, { steps: 4 });
    if (shift) await page.keyboard.down("Shift");
    const [view] = await Promise.all([
        clickTableResult(page),
        page.mouse.up({ button }),
    ]);
    if (shift) await page.keyboard.up("Shift");
    return view;
}

async function handPoint(page, slot) {
    const box = await card(page, slot).boundingBox();
    assert.ok(box, `hand card ${slot}`);
    return { x: box.x + box.width * 0.6, y: box.y + box.height * 0.12 };
}

await check(
    "manual table QA shifts a dragged card face down",
    linked(async (host, guest) => {
        await start([["Island", 20]], [["Mountain", 20]]);
        await showBoth(host, guest);
        const before = await tableView("host");
        const slot = lastHand(before);
        // The lower half of the battlefield is the host's drop target.
        const board = await host.locator(".card-layer").boundingBox();
        assert.ok(board);
        const after = await drag(
            host,
            await handPoint(host, slot),
            {
                x: board.x + board.width * 0.4,
                y: board.y + board.height * 0.66,
            },
            { shift: true },
        );
        assert.equal(object(after, slot)?.face, "down");
        const theirs = await showBoth(host, guest, after);
        const hidden = object(theirs, theirs.battlefield[0]);
        assert.equal(hidden.face, "down");
        assert.equal(hidden.name, null);
    }),
    live,
);

await check(
    "manual table QA previews a public card",
    linked(async (host, guest) => {
        await start([["Llanowar Elves", 20]], [["Mountain", 20]]);
        await act({
            move: {
                to: "battlefield",
                ...ref(
                    await tableView("host"),
                    lastHand(await tableView("host")),
                ),
            },
        });
        const theirs = await showBoth(host, guest);
        await card(guest, theirs.battlefield[0]).hover();
        await committed(guest);
        await guest.locator(".stage .inspect").waitFor();
    }),
    live,
);

await check(
    "manual table QA plays phase and die sounds",
    linked(async (host, guest) => {
        for (const [action, sound] of [
            [{ phase: { phase: "first_main" } }, "step"],
            [{ roll: { sides: 20, count: 1 } }, "coin"],
        ]) {
            const counts = await Promise.all(
                [host, guest].map((page) =>
                    page.evaluate(() => window.berylSounds.length),
                ),
            );
            await act(action);
            const view = await tableView("host");
            await Promise.all(
                [host, guest].map((page) =>
                    showTableView(page, view, undefined, { play: true }),
                ),
            );
            for (const [index, page] of [host, guest].entries()) {
                const sounds = await page.evaluate(
                    (from) =>
                        window.berylSounds.slice(from).map((cue) => cue.sound),
                    counts[index],
                );
                assert.ok(sounds.includes(sound), `seat ${index}: ${sounds}`);
            }
        }
    }),
    live,
);

const menuItem = (page, name) =>
    page.getByRole("menuitem", { name, exact: true });
const shortcut = (page, chord) =>
    page.keyboard.press(
        `${process.platform === "darwin" ? "Meta" : "Control"}+${chord}`,
    );

await check(
    "manual table QA resolves a creature and shows tap and counters at both seats",
    linked(async (host, guest) => {
        await start([["Llanowar Elves", 20]], [["Mountain", 20]]);
        await showBoth(host, guest);
        const initial = await tableView("host");
        const spell = await clickTable(host, card(host, lastHand(initial)));
        const cast = spell.objects.find(({ zone }) => zone === "stack");
        assert.equal(cast.name, "Llanowar Elves");
        assert.equal(
            (await showBoth(host, guest, spell)).objects.find(
                ({ zone }) => zone === "stack",
            ).name,
            cast.name,
        );
        const resolved = await clickTable(host, card(host, cast.slot));
        const creature = object(resolved, resolved.battlefield[0]);
        const other = await showBoth(host, guest, resolved);
        assert.equal(object(other, other.battlefield[0]).name, creature.name);

        await card(host, creature.slot).click({ button: "right" });
        const menu = host.getByRole("menu");
        const rows = menu.getByRole("menuitem");
        assert.ok((await rows.count()) > 0);
        assert.equal(
            await rows
                .locator("button, input, select, textarea, [role=button]")
                .count(),
            0,
        );
        for (const name of ["Tap", "Add +1/+1 counter"]) {
            await menuItem(host, name).waitFor();
        }
        const marked = await clickTable(
            host,
            menuItem(host, "Add +1/+1 counter"),
        );
        const updated = await showBoth(host, guest, marked);
        for (const view of [marked, updated]) {
            assert.deepEqual(object(view, view.battlefield[0]).pt, {
                power: 2,
                toughness: 2,
            });
        }

        const guestHand = await tableView("guest");
        await act(
            {
                move: {
                    to: "battlefield",
                    ...ref(guestHand, lastHand(guestHand)),
                },
            },
            "guest",
        );
        let theirs = await showBoth(host, guest);
        const land = theirs.objects.find(
            ({ zone, name }) => zone === "battlefield" && name === "Mountain",
        );
        for (const tapped of [true, false]) {
            const after = await clickTable(guest, card(guest, land.slot));
            assert.equal(object(after, land.slot).tapped, tapped);
            theirs = await showBoth(host, guest);
            assert.equal(object(theirs, land.slot).tapped, tapped);
            const mine = await tableView("host");
            assert.equal(
                mine.objects.find(
                    ({ zone, name }) =>
                        zone === "battlefield" && name === "Mountain",
                ).tapped,
                tapped,
            );
        }
    }),
    live,
);

await check(
    "manual table QA shows a private top-card look and undoes a draw past a guest tap",
    linked(async (host, guest) => {
        await start([["Llanowar Elves", 20]], [["Mountain", 20]]);
        const setup = await tableView("guest");
        await act(
            { move: { to: "play", ...ref(setup, lastHand(setup)) } },
            "guest",
        );
        const theirs = await showBoth(host, guest);
        const land = theirs.battlefield[0];

        await listen(guest);
        await pile(host, "library").hover();
        await shortcut(host, "k");
        await host.keyboard.type("top");
        await host.getByRole("option", { name: /^Look at top cards…/ }).click();
        await host
            .getByRole("textbox", { name: "Look at top cards" })
            .fill("3");
        const [looked] = await Promise.all([
            clickTableResult(host),
            host.keyboard.press("Enter"),
        ]);
        assert.equal(looked.handling.count, 3);
        await showBoth(host, guest, looked);
        const lifts = await guest.evaluate(() =>
            window.qaUpdates
                .at(-1)
                .view.script.beats.flat()
                .filter(({ act }) => act.verb === "lift"),
        );
        assert.ok(
            lifts.some(
                ({ act }) => act.span === "begin" && act.handling.count === 3,
            ),
        );
        const [closed] = await Promise.all([
            clickTableResult(host),
            host.keyboard.press("Escape"),
        ]);
        assert.match(closed.table.log.at(-1).text, /Closed the pile viewer/);
        const drawn = await clickTable(host, pile(host, "library"));
        const handSize = drawn.table.orders.you.hand.length;
        assert.equal(handSize, closed.table.orders.you.hand.length + 1);
        await showBoth(host, guest, drawn);
        const tapped = await clickTable(guest, card(guest, land));
        assert.ok(tapped.table, JSON.stringify(tapped));
        assert.equal(object(tapped, land).tapped, true);
        await showBoth(host, guest);

        const [undone] = await Promise.all([
            clickTableResult(host),
            shortcut(host, "z"),
        ]);
        assert.equal(undone.table.orders.you.hand.length, handSize - 1);
        assert.match(
            undone.table.log.at(-1).text,
            /Undid: Drew 1 card \(seen\)/,
        );
        const after = await showBoth(host, guest, undone);
        assert.equal(object(after, land).tapped, true);
    }),
    live,
);

await check(
    "manual table QA shows attack arrows and a public discard on both seats",
    linked(async (host, guest) => {
        await start([["Grizzly Bears", 20]], [["Llanowar Elves", 20]]);
        for (const seat of ["host", "guest"]) {
            const view = await tableView(seat);
            await act(
                {
                    move: { to: "battlefield", ...ref(view, lastHand(view)) },
                },
                seat,
            );
            await showBoth(host, guest);
        }
        await act({ phase: { phase: "declare_attackers" } });
        await showBoth(host, guest);
        const mine = await tableView("host");
        const bear = mine.objects.find(
            ({ name, zone }) =>
                name === "Grizzly Bears" && zone === "battlefield",
        );
        const elf = mine.objects.find(
            ({ name, zone }) =>
                name === "Llanowar Elves" && zone === "battlefield",
        );
        const opponent = host.locator('[data-life="opponent"] .life-hit');
        const attacked = await drag(
            host,
            center(await card(host, bear.slot).boundingBox()),
            center(await opponent.boundingBox()),
            { button: "right" },
        );
        await showBoth(host, guest, attacked);
        for (const page of [host, guest]) {
            const line = page.locator('[data-line^="attack "]');
            assert.match(await line.getAttribute("d"), /^M.* L/);
        }

        const guestHand = await tableView("guest");
        await act(
            {
                move: {
                    to: { graveyard: 1 },
                    ...ref(guestHand, lastHand(guestHand)),
                },
            },
            "guest",
        );
        const beforeDiscard = await showBoth(host, guest);
        const graveyard = host.locator(
            'button[data-pile="graveyard"][data-side="opponent"]',
        );
        const moved = await drag(
            host,
            center(await card(host, elf.slot).boundingBox()),
            center(await graveyard.boundingBox()),
        );
        const theirs = await showBoth(host, guest, moved);
        assert.equal(
            object(moved, moved.players.opponent.graveyard.at(-1)).name,
            elf.name,
        );
        assert.equal(
            object(theirs, theirs.players.you.graveyard.at(-1)).name,
            elf.name,
        );
        assert.equal(
            theirs.players.you.graveyard.length,
            beforeDiscard.players.you.graveyard.length + 1,
        );
        assert.equal(moved.table.log.at(-1).by, "You");
        assert.equal(theirs.table.log.at(-1).by, "Host");
    }),
    live,
);
