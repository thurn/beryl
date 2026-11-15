// These checks need two real sessions. The generic audits cannot break a
// transport, verify the original guest redials, or observe a refused join.
import assert from "node:assert/strict";
import { crowdedControls } from "./capture.mjs";
import { check } from "./kept-harness.mjs";
import {
    clickTableResult,
    pair,
    showTableView,
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
const ownLife = (page) => page.locator('[data-life="you"] .life-hit');
const notice = (page, text) =>
    page.getByRole("status").getByText(text, { exact: true });
const lobbyCommand = (seat, change) =>
    tableCommand("table_lobby", { request: { change } }, seat);
const action = (seat, action) =>
    tableCommand("table_act", { request: { action } }, seat);
const shownPlayers = (page) =>
    page.evaluate(() => window.berylStage.shown.view.players);
const leave = (seat) => tableCommand("table_leave", {}, seat);
const goTo = (page, hash) =>
    page.evaluate((next) => {
        location.hash = next;
    }, hash);

await check(
    "manual table link freezes both seats and resumes the saved game",
    async (page) => {
        const { host, guest } = await pair(page, "memory");
        try {
            for (const seat of ["host", "guest"]) {
                await lobbyCommand(seat, {
                    kind: "load_deck",
                    main: [["Island", 20]],
                    side: [],
                });
            }
            await lobbyCommand("guest", { kind: "ready", ready: true });
            await lobbyCommand("host", { kind: "start" });
            assert.deepEqual(
                await action("guest", { life: { seat: 1, set: 17 } }),
                { ok: true },
            );
            const before = await tableView("guest");
            await showTableView(guest, before);
            await showTableView(host, await tableView("host"));
            const points = await Promise.all([
                ownLife(host).boundingBox(),
                ownLife(guest).boundingBox(),
            ]);

            await tableCommand("test_cut_table_link", {}, "host");
            await Promise.all([
                notice(host, "Guest is reconnecting").waitFor(),
                notice(guest, "Host is reconnecting").waitFor(),
            ]);
            assert.deepEqual(await crowdedControls(host), []);
            const sent = [];
            for (const seat of [host, guest]) {
                seat.on("request", (request) => {
                    if (
                        new URL(request.url()).pathname === "/invoke/table_act"
                    ) {
                        sent.push(request.postData());
                    }
                });
            }
            const mod = process.platform === "darwin" ? "Meta" : "Control";
            for (const [index, seat] of [host, guest].entries()) {
                assert.equal(
                    await seat
                        .locator("[data-manual-table]")
                        .evaluate((node) => !!node.closest("[inert]")),
                    true,
                );
                const { x, y, width, height } = points[index];
                await seat.mouse.click(x + width / 2, y + height / 2);
                await seat.keyboard.press(mod + "+d");
                assert.equal(
                    await seat
                        .getByRole("textbox", { name: "Set life" })
                        .count(),
                    0,
                );
            }
            assert.deepEqual(
                await action("guest", { life: { seat: 1, set: 4 } }),
                { refused: "stale" },
            );
            assert.deepEqual(await shownPlayers(guest), before.players);
            assert.deepEqual(sent, []);

            await goTo(host, "#/");
            const saved = host
                .getByRole("list", { name: "Saved tables" })
                .getByRole("listitem")
                .filter({ hasText: "Host · Guest" });
            await saved.getByRole("button", { name: "Resume table" }).click();
            await host.locator("[data-manual-table]").waitFor();
            await notice(guest, "Host is reconnecting").waitFor({
                state: "hidden",
            });
            const guestView = await tableView("guest");
            const hostView = await tableView("host");
            assert.equal(guestView.table.link.kind, "connected");
            assert.deepEqual(guestView.script?.beats, []);
            assert.deepEqual(await shownPlayers(guest), before.players);
            assert.deepEqual(await shownPlayers(guest), guestView.players);
            assert.equal(
                guestView.players.you.library.length,
                hostView.players.opponent.library.length,
            );
            assert.equal(
                guestView.players.you.life,
                hostView.players.opponent.life,
            );
            assert.equal(guestView.players.you.life, 17);
            await ownLife(guest).click();
            const life = guest.getByRole("textbox", { name: "Set life" });
            await life.fill("19");
            const [played] = await Promise.all([
                clickTableResult(guest),
                life.press("Enter"),
            ]);
            assert.equal(played.players.you.life, 19);
            assert.equal((await tableView("host")).players.opponent.life, 19);

            await leave("host");
            await notice(guest, "Host left the table").waitFor();
            await guest
                .getByRole("button", { name: "Lobby", exact: true })
                .click();
            await guest.getByRole("button", { name: "Join a table" }).waitFor();
        } finally {
            await leave("guest");
            await leave("host");
            await guest.close();
        }
    },
    live,
);

await check(
    "manual table link reports bad invites and a version mismatch",
    async (host) => {
        const guest = await host.context().newPage();
        try {
            await guest.goto(
                new URL(await tableRoute(guest, false, "guest"), host.url())
                    .href,
            );
            await host.getByRole("button", { name: "Host a table" }).click();
            await host
                .getByLabel("Name", { exact: true })
                .fill("Compatibility Host");
            await host
                .getByRole("button", { name: "Host", exact: true })
                .click();
            const invite = await host.getByLabel("Invite code").inputValue();
            const secretAt = invite.lastIndexOf(":") + 1;
            const prefix = invite.slice(0, secretAt);
            const secret = invite.slice(secretAt);
            const wrong = `${prefix}${secret.startsWith("a") ? "b" : "a"}${secret.slice(1)}`;
            await guest.getByRole("button", { name: "Join a table" }).click();
            await guest.getByLabel("Name", { exact: true }).fill("Guest");
            const join = async (code, message) => {
                await guest.getByLabel("Invite code").fill(code);
                await guest
                    .getByRole("button", { name: "Join", exact: true })
                    .click();
                const alert = guest
                    .getByRole("alert")
                    .filter({ hasText: message });
                await alert.waitFor();
                assert.equal(await alert.textContent(), message);
                assert.equal(new URL(guest.url()).hash, "#/");
                assert.equal(
                    await tableCommand("table_current", {}, "guest"),
                    null,
                );
            };
            await join("bad invite", "Invalid invite code.");
            await join(wrong, "The host did not accept the invite.");
            await guest.route("**/invoke/table_join?*", (route) => {
                const url = new URL(route.request().url());
                url.pathname = "/invoke/test_join_table_mismatch";
                return route.continue({ url: url.href });
            });
            const mismatch = "Both players need the same version of Beryl.";
            await join(invite, mismatch);
            const alert = host.getByRole("alert").filter({ hasText: mismatch });
            await alert.waitFor();
            assert.equal(await alert.textContent(), mismatch);
        } finally {
            await leave("guest");
            await leave("host");
            await guest.close();
        }
    },
    live,
);
