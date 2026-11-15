import assert from "node:assert/strict";
import { crowdedControls } from "./capture.mjs";
import { check } from "./kept-harness.mjs";
import { pair, tableCommand, tableRoute, tableView } from "./table.mjs";

const lobby = (page) => page.getByRole("main", { name: "Table lobby" });
const button = (page, name) => page.getByRole("button", { name, exact: true });
const mod = process.platform === "darwin" ? "Meta" : "Control";
const gear = (page) => button(page, "Settings");
const leave = (seat) => tableCommand("table_leave", {}, seat);

async function importDeck(page, name, list) {
    await button(lobby(page), "Change").click();
    await button(lobby(page), "Paste decklist").click();
    await lobby(page).getByLabel("Deck name").fill(name);
    await lobby(page).getByLabel("Decklist").fill(list);
    await button(lobby(page), "Import").click();
}

await check(
    "manual table lobby shares counts and score with both players",
    async (page) => {
        const { host, guest } = await pair(page, "local", { fromStart: true });
        try {
            await button(host, "Host a table").waitFor();
            await host.keyboard.press(mod + "+n");
            await host.getByLabel("Name", { exact: true }).fill("Derek");
            await button(host, "Host").click();
            const invite = await lobby(host)
                .getByLabel("Invite code")
                .inputValue();
            assert.ok(invite);
            await button(lobby(host), "Copy").waitFor();

            await button(guest, "Join a table").click();
            await guest.evaluate(async () => {
                window.lobbyPayloads = [];
                await window.berylLive.listen("table-lobby", (payload) => {
                    window.lobbyPayloads.push(payload);
                });
            });
            await guest.getByLabel("Name", { exact: true }).fill("Lee");
            await guest.getByLabel("Invite code").fill(invite);
            await button(guest, "Join").click();
            for (const seat of [host, guest]) {
                await lobby(seat)
                    .getByText("Derek", { exact: true })
                    .first()
                    .waitFor();
                await lobby(seat)
                    .getByText("Lee", { exact: true })
                    .first()
                    .waitFor();
                await lobby(seat)
                    .getByText(/^Direct/)
                    .waitFor();
                await lobby(seat).getByText("Match score 0–0").waitFor();
            }
            const start = button(lobby(host), "Start");
            assert.equal(await start.isEnabled(), false);
            assert.equal(
                await lobby(guest).getByLabel("Invite code").count(),
                0,
            );
            assert.equal(
                await lobby(guest).getByLabel("Starting life").count(),
                0,
            );
            assert.equal(await button(lobby(guest), "Start").count(), 0);

            await importDeck(
                host,
                "Host private deck",
                "20 Island\n\nSideboard\n2 Plains",
            );
            await importDeck(guest, "Guest private deck", "21 Mountain");
            await lobby(guest).getByText("20 main · 2 sideboard").waitFor();
            const ready = button(lobby(guest), "Ready");
            await ready.click({ trial: true });
            assert.equal(await start.isEnabled(), false);
            await guest.keyboard.press(mod + "+Shift+s");
            await lobby(guest)
                .getByRole("button", { name: "Ready", pressed: true })
                .waitFor();
            const payloads = await guest.evaluate(() => window.lobbyPayloads);
            assert.ok(
                payloads.some(({ view }) => view?.seats[0]?.mainCount === 20),
            );
            for (const text of [
                await guest.locator("body").innerText(),
                JSON.stringify(payloads),
            ]) {
                assert.doesNotMatch(text, /Island|Plains|Host private deck/);
            }
            assert.deepEqual(await crowdedControls(host), []);
            assert.deepEqual(await crowdedControls(guest), []);

            await lobby(host).getByLabel("First").selectOption("0");
            await button(lobby(host), "Save settings").click();
            await start.click();
            for (const seat of [host, guest])
                await seat.locator("[data-manual-table]").waitFor();

            await gear(guest).click();
            await button(guest, "Lobby").click();
            const information = guest.getByRole("dialog", { name: "Lobby" });
            await information.getByText("Match score 0–0").waitFor();
            await button(information, "Close").click();
            await gear(host).click();
            for (const name of [
                "Lobby",
                "Next game",
                "Concede",
                "Assists",
                "Shortcuts",
                "Leave table",
            ])
                await button(host, name).waitFor();
            await button(host, "Next game").click();
            await button(
                host.getByRole("dialog", { name: "Next game" }),
                "Derek wins",
            ).click();
            for (const [key, seat] of [
                ["host", host],
                ["guest", guest],
            ]) {
                await lobby(seat).getByText("Match score 1–0").waitFor();
                await lobby(seat).getByText("Derek won game 1").waitFor();
                assert.ok(
                    (await tableView(key)).table.log.some(({ text }) =>
                        /^Ended the game\..+ won\.$/.test(text),
                    ),
                );
            }
            assert.equal(
                await lobby(host).getByLabel("First").inputValue(),
                "",
            );
            await lobby(guest).getByText("Loser chooses").waitFor();
            await button(lobby(host), "Sideboard").click();
            await button(lobby(host), "Move Plains to main deck").click();
            await lobby(guest).getByText("21 main · 1 sideboard").waitFor();

            await ready.click();
            await start.click();
            await guest.locator("[data-manual-table]").waitFor();
            assert.equal((await tableView("host")).turn.active, "opponent");
            await guest.keyboard.press("F2");
            const concede = guest.getByRole("dialog", { name: "Concede" });
            await button(concede, "Cancel").click();
            await guest.locator("[data-manual-table]").waitFor();
            await guest.keyboard.press("F2");
            await button(concede, "Confirm Concede").click();
            for (const seat of [host, guest])
                await lobby(seat).getByText("Match score 2–0").waitFor();
        } finally {
            await leave("guest");
            await leave("host");
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
