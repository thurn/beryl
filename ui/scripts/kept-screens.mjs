// The kept checks of the screens around the table (kept.mjs): the playground, keys and focus
// rings, the settings, the log, stops and tips, the start screen, the result and a bug report's
// freeze.
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { join } from "node:path";
import {
    ANSWERED,
    FIXTURE,
    base,
    card,
    check,
    committed,
    derive,
    fixtures,
    load,
    recordedCards,
    replied,
    writeFixture,
} from "./kept-harness.mjs";

// Why no audit: a draft and a count kept across a reload, and a page's scroll: no screen a shot
// settles on shows them.
await check(
    "playground reports and navigation survive refresh",
    async (page) => {
        await page.addInitScript(() => {
            window.EventSource = class {
                constructor() {
                    setTimeout(() => this.onopen?.(), 0);
                }
                addEventListener() {}
                close() {}
            };
        });
        const moment = "playground-payment";
        let reports = 2;
        await page.route("**/spec-api/**", async (route) => {
            const path = new URL(route.request().url()).pathname;
            let body;
            if (path.endsWith("/specs"))
                body = [
                    {
                        card: "Aang’s Iceberg",
                        slug: "aangs-iceberg",
                        reviewed: null,
                        reports,
                        momentReports: {
                            [`aangs-iceberg/${moment}/yours`]: reports,
                        },
                    },
                ];
            else if (path.includes("/playground/"))
                body = {
                    examples: [
                        {
                            title: "Payment",
                            variants: [
                                {
                                    name: moment,
                                    title: "Payment",
                                    label: "Default",
                                    plays: true,
                                },
                            ],
                        },
                    ],
                    missing: [],
                };
            else if (path.includes("/stream/"))
                return route.fulfill({
                    contentType: "text/event-stream",
                    body: "",
                });
            else if (path.endsWith("/held"))
                body = { ok: path.includes("/theirs/") };
            else if (path.endsWith("/invoke/card_records"))
                body = { ok: recordedCards };
            else if (path.endsWith("/report")) {
                reports++;
                body = { ok: { id: "test", path: "test" } };
            } else body = { ok: null };
            await route.fulfill({ json: body });
        });
        await page.route("**/?live=*", (route) =>
            route.fulfill({
                contentType: "text/html",
                body: `<script>parent.postMessage({kind:"beryl-shown"},location.origin)</script>`,
            }),
        );
        await page.goto(new URL("/playground/aangs-iceberg", base).href);
        const board = page.locator(".specs-moment").first();
        const bug = board.getByRole("button", {
            name: "Report a bug",
            exact: true,
        });
        await bug.waitFor();
        assert.equal(await bug.getAttribute("data-filed"), "2");
        assert.equal(await page.getByText("More cases →").count(), 0);
        await bug.click();
        await board.getByRole("textbox").fill("Keep my report draft");
        await board
            .getByRole("checkbox", { name: "Bug in the playground viewer" })
            .check();
        await page.reload();
        await bug.click();
        assert.equal(
            await board.getByRole("textbox").inputValue(),
            "Keep my report draft",
        );
        assert.ok(await board.getByRole("checkbox").isChecked());
        await board.getByRole("button", { name: "File", exact: true }).click();
        await page.waitForFunction(() => !document.querySelector("textarea"));
        await page.reload();
        await bug.waitFor();
        assert.equal(await bug.getAttribute("data-filed"), "3");
        await bug.click();
        assert.equal(await board.getByRole("textbox").inputValue(), "");
        await board
            .getByRole("button", { name: "Cancel", exact: true })
            .click();
        assert.equal(
            await board
                .getByRole("button", { name: "Play", exact: true })
                .count(),
            0,
        );
        const theirs = page.locator(".specs-moment").nth(1);
        await theirs.scrollIntoViewIfNeeded();
        await theirs
            .getByRole("button", { name: "Play", exact: true })
            .waitFor();
        const header = await page.locator(".testing-header").boundingBox();
        assert.ok(
            header.y >= 0 && header.y < 2,
            "navigation stays at the top after scrolling",
        );
        await page
            .getByRole("link", { name: "Playground", exact: true })
            .click();
        await page
            .getByRole("heading", { name: "Playground", exact: true })
            .waitFor();
    },
    // Off: it reloads and navigates the testing pages, waiting on their requests and their
    // stubbed EventSource's timer as a browser would; it measures no motion.
    {
        clock: "off",
    },
);

// Why no audit: which control a key reaches, and its ring while it has the focus.
await check(
    "the turn button is reached by Tab, with a visible ring",
    async (page) => {
        await page.clock.runFor(2000);
        const turn = page.locator(".turn-action button");
        // WebKit on macOS tabs only to text fields unless the system's Keyboard navigation
        // setting is on; Option+Tab reaches every control either way, as in Safari.
        const tab = process.platform === "darwin" ? "Alt+Tab" : "Tab";
        for (let i = 0; i < 40; i++) {
            if (await turn.evaluate((b) => b === document.activeElement)) break;
            await page.keyboard.press(tab);
        }
        assert.ok(
            await turn.evaluate((b) => b === document.activeElement),
            "Tab reaches the turn button",
        );
        const ring = await turn.evaluate(
            (b) => getComputedStyle(b).outlineStyle,
        );
        assert.notEqual(ring, "none", "the focused turn button shows a ring");
    },
);

// Why no audit: a key's effect on an open panel.
await check("Escape closes the settings", async (page) => {
    await page.clock.runFor(2000);
    await page.getByRole("button", { name: "Settings" }).click();
    const panel = page.locator(".settings");
    await panel.waitFor();
    await page.keyboard.press("Escape");
    assert.equal(await panel.count(), 0, "closed");
});

// Why no audit: what a drag leaves selected: a gesture's side effect.
await check("dragging across the table selects no text", async (page) => {
    await page.clock.runFor(2000);
    const selected = () =>
        page.evaluate(() => window.getSelection()?.toString() ?? "");
    // From the far corner, over the log and the table, to the near corner.
    await page.mouse.move(60, 90);
    await page.mouse.down();
    await page.mouse.move(1580, 880, { steps: 25 });
    await page.mouse.up();
    assert.equal(await selected(), "", "nothing selected by a drag");
    // A hand card picked up, carried over the log and put back.
    const box = await page.locator(card).boundingBox();
    await page.mouse.move(box.x + box.width / 2, box.y + 40);
    await page.mouse.down();
    await page.mouse.move(60, 300, { steps: 15 });
    await page.mouse.move(box.x + box.width / 2, 880, { steps: 15 });
    await page.mouse.up();
    assert.equal(await selected(), "", "nothing selected by a card's drag");
});

// A fixture's history reaches the log once, however often the screen starts listening (React's
// StrictMode mounts it twice), so no turn's heading comes twice.
// Why no audit: a history told twice is a heading twice in a list, no fault of any screen.
await check(
    "the log tells a fixture's history once",
    async (page) => {
        const turns = await page.locator(".game-log-turn").allTextContents();
        assert.ok(turns.length > 0, "the log heads its turns");
        assert.deepEqual(
            turns,
            [...new Set(turns)],
            `each turn is headed once: ${await page.locator(".game-log").textContent()}`,
        );
    },
    {
        fixture: "opponent-turn",
        at: 22,
        ready: ".game-log-toggle",
    },
);

// The log rests at its newest entry, even one laid out while hidden (by an ancestor), which has
// no size to scroll in until it shows: it would show its first turn.
// Why no audit: where a list is scrolled to once an ancestor that hid it shows.
await check(
    "the log shows its newest entry once it shows",
    async (page) => {
        await page.clock.runFor(2000);
        const list = page.locator(".game-log-list");
        assert.ok(await list.isHidden(), "the log is hidden");
        await page.evaluate(() => document.getElementById("hide-log").remove());
        // Folded by default: opened, it shows.
        await page.getByRole("button", { name: "Show log" }).click();
        // A resize is observed as the page renders, on the real clock.
        const scroll = () =>
            list.evaluate((el) => [
                el.scrollTop,
                el.clientHeight,
                el.scrollHeight,
            ]);
        let [top, height, full] = await scroll();
        // The tries only guard against a hang, with room for a loaded machine.
        for (let i = 0; i < 200 && top + height < full - 2; i++) {
            await page.clock.runFor(100);
            await page.waitForTimeout(100);
            [top, height, full] = await scroll();
        }
        assert.ok(full > height, "the log overflows its box");
        assert.ok(
            top + height >= full - 2,
            `scrolled to ${top} of ${full - height}`,
        );
    },
    {
        ready: "[data-life]",
        init: () => {
            const style = document.createElement("style");
            style.id = "hide-log";
            style.textContent = ".game-log { display: none !important }";
            document.addEventListener("DOMContentLoaded", () =>
                document.head.append(style),
            );
        },
    },
);

// Why no audit: what a setting sends: a command, which no screen shows.
await check(
    "the full-control toggle sends the session its new stops",
    async (page) => {
        const recorded = JSON.parse(
            await readFile(join(fixtures, `${FIXTURE}.json`), "utf8"),
        );
        const session = recorded.messages.find(
            (m) => m.kind === "response" && m.command === "start_game",
        ).value;
        await page.clock.runFor(2000);
        await page.click('[aria-label="Settings"]');
        const toggle = page.locator(".settings-toggle input");
        assert.equal(await toggle.isChecked(), false, "smart stops at first");
        await toggle.click();
        const sent = await page.evaluate(() =>
            window.berylFixture.calls
                .filter((c) => c.command === "set_stops")
                .map((c) => c.args.request),
        );
        const smart = {
            yours: [
                "precombatMain",
                "declareAttackers",
                "declareBlockers",
                "postcombatMain",
            ],
            theirs: ["declareAttackers", "declareBlockers", "end"],
        };
        assert.deepEqual(sent, [
            { session, stops: { fullControl: true, ...smart } },
        ]);
    },
);

// Why no audit: what a click on a stop sends, and the stops that take none.
await check(
    "a step icon sets and clears a stop in that player's turns",
    async (page) => {
        await page.clock.runFor(2000);
        const row = ".step-stops-opponent";
        const end = `${row} [data-step=end]`;
        const upkeep = `${row} [data-step=upkeep]`;
        assert.ok(await page.locator(`${end}.step-stop`).count());
        assert.ok(await page.locator(`${upkeep}.step-no-stop`).count());
        // Untap and cleanup, where no one gets priority, are never stops.
        assert.equal(await page.locator(`${row} .step-fixed`).count(), 2);
        assert.equal(
            await page.locator(`${row} .step-fixed .step-toggle`).count(),
            0,
        );
        await page.click(`${end} .step-toggle`);
        await page.click(`${upkeep} .step-toggle`);
        assert.ok(await page.locator(`${end}.step-no-stop`).count());
        assert.ok(await page.locator(`${upkeep}.step-stop`).count());
        const sent = await page.evaluate(() =>
            window.berylFixture.calls
                .filter((c) => c.command === "set_stops")
                .map((c) => c.args.request.stops.theirs),
        );
        assert.deepEqual(sent.at(-1), [
            "declareAttackers",
            "declareBlockers",
            "upkeep",
        ]);
    },
);

// Why no audit: the words of a tip, by seat.
await check(
    "the beginning of combat tooltip names the step for both seats",
    async (page) => {
        for (const [side, expected] of [
            ["you", "Your beginning of combat"],
            ["opponent", "Opponent's beginning of combat"],
        ]) {
            const stop = page.locator(
                `.step-stops-${side} [data-step="beginCombat"]`,
            );
            await stop.locator("button").hover();
            assert.equal(await stop.locator(".tip").innerText(), expected);
        }
    },
);

// Why no audit: a ring that depends on how the focus came.
await check(
    "a phase icon has a round keyboard focus ring that clears on pointer use",
    async (page) => {
        const icon = page.locator('.step-stops-you [data-step="main1"] button');
        await page.keyboard.press("Tab");
        await icon.focus();
        const ring = () =>
            icon.evaluate((e) => {
                const style = getComputedStyle(e);
                return [style.outlineStyle, style.borderRadius];
            });
        assert.deepEqual(await ring(), ["solid", "50%"]);
        await page
            .getByRole("button", { name: "Report bug", exact: true })
            .click();
        assert.equal((await ring())[0], "none");
    },
);

// The start screen: Enter starts the game, and the opponent picked is offered again after a
// reload.
const startCalls = (page) =>
    page.evaluate(() =>
        window.berylFixture.calls.filter((c) => c.command === "start_game"),
    );
// Why no audit: a key on the start screen, and the command it sends.
await check(
    "Enter on the start screen starts a game",
    async (page) => {
        await page.keyboard.press("Enter");
        await page.waitForURL(/#\/game$/);
        assert.equal((await startCalls(page)).length, 1);
    },
    {
        at: 0,
        hash: "#/",
        ready: ".start-agent input:checked",
    },
);
// Why no audit: a key on the start screen with a game under way, and that it sends none.
await check(
    "Enter with a game in progress returns to it",
    async (page) => {
        await page.getByRole("button", { name: "Return to Game" }).waitFor();
        await page.keyboard.press("Enter");
        await page.waitForURL(/#\/game$/);
        assert.equal((await startCalls(page)).length, 0, "no new game");
    },
    {
        at: 50,
        hash: "#/",
        ready: ".start-agent input:checked",
    },
);
// Why no audit: a choice kept across a reload.
await check(
    "the start screen remembers the last opponent across a reload",
    async (page) => {
        const checked = () =>
            page.locator(".start-agent input:checked").getAttribute("value");
        const other = await page
            .locator(".start-agent input:not(:checked)")
            .first()
            .getAttribute("value");
        await page.locator(`.start-agent input[value="${other}"]`).check();
        await page.getByRole("button", { name: "Start", exact: true }).click();
        await page.waitForURL(/#\/game$/);
        const [call] = await startCalls(page);
        assert.equal(call.args.request.agent, other);
        await page.evaluate(() => (window.location.hash = "#/"));
        await page.reload();
        await page.waitForSelector(".start-agent input:checked");
        assert.equal(await checked(), other, "the last opponent is picked");
    },
    {
        at: 0,
        hash: "#/",
        ready: ".start-agent input:checked",
    },
);

// The end of a game (`game-over`, under full control): Beza, the Bounding Spring attacks a
// human at 3 life with nothing to block, and its damage ends the game once they pass. The page
// starts at that last pass.
const overFixture = JSON.parse(
    await readFile(join(fixtures, "game-over.json"), "utf8"),
);
const overViews = await load("game-over");
const overEvents = overFixture.messages.filter((m) => m.kind === "event");
const lastPass = overViews.findLastIndex((v) => v.status.kind === "deciding");
const overAt = { fixture: "game-over", at: lastPass + 1, ready: "[data-life]" };
for (const [reason, title, description, by] of [
    ["life", "Victory", null, { name: "Trinket Mage", ability: null }],
    [
        "effect",
        "Victory",
        "Card effect",
        { name: "Trinket Mage", ability: null },
    ],
    ["library", "Victory", "Library", null],
    ["concede", "Defeat", "Conceded", null],
]) {
    const fixture = `result-${reason}`;
    await derive("game-over", fixture, overViews.length - 1, (view) => ({
        ...view,
        status: {
            kind: "over",
            outcome: {
                kind: "win",
                winner: title === "Victory" ? "you" : "opponent",
                reason,
                by,
            },
        },
    }));
    // Why no audit: the result's words and card for each way a game ends, over views written for
    // each.
    await check(
        `the ${reason} result shows ${title} and the applicable details`,
        async (page) => {
            const result = page.getByRole("dialog", { name: title });
            await result.waitFor();
            assert.deepEqual(
                await result.locator(".game-over-reason").allTextContents(),
                description ? [description] : [],
            );
            assert.deepEqual(
                await result
                    .locator(".game-over-card")
                    .evaluateAll((cards) =>
                        cards.map((card) => card.dataset.card),
                    ),
                reason === "effect" ? [by.name] : [],
            );
            await result
                .getByRole("button", { name: "New Game", exact: true })
                .waitFor();
            await result
                .getByRole("button", { name: "View Battlefield" })
                .click();
            assert.equal(await result.count(), 0);
            await page
                .getByRole("button", { name: `Back to ${title}` })
                .click();
            await result.waitFor();
        },
        {
            ...overAt,
            fixture,
            at: overViews.length,
        },
    );
}

// Why no audit: a question asked in place, and the command its answer sends.
await check(
    "the settings concede the game once confirmed in place",
    async (page) => {
        await page.clock.runFor(2000);
        const concessions = () =>
            page.evaluate(() =>
                window.berylFixture.calls.filter(
                    (c) => c.command === "concede",
                ),
            );
        const settings = page.getByRole("button", { name: "Settings" });
        const concede = page.getByRole("button", {
            name: "Concede",
            exact: true,
        });
        const confirm = page.getByRole("button", {
            name: "Confirm Concede",
            exact: true,
        });
        await settings.click();
        await concede.click();
        assert.deepEqual(await concessions(), [], "asks first");
        assert.equal(await page.locator(".settings").count(), 1, "still open");
        // Closing the settings puts the question back.
        await settings.click();
        await settings.click();
        assert.equal(await confirm.count(), 0, "asked again");
        await concede.click();
        await confirm.click();
        assert.deepEqual(
            (await concessions()).map((c) => c.args.request.session),
            [overEvents[0].payload.session],
        );
        assert.equal(await page.locator(".settings").count(), 0, "closed");
    },
    overAt,
);

// Why no audit: two commands in their order, behind a question.
await check(
    "a new game with one in progress asks first, then concedes it",
    async (page) => {
        await page.evaluate(() => (window.location.hash = "#/"));
        assert.equal(
            await page.locator(".start-title").textContent(),
            "New Game",
        );
        await page
            .getByRole("button", { name: "New Game", exact: true })
            .click();
        const commands = () =>
            page.evaluate(() =>
                window.berylFixture.calls
                    .map((c) => c.command)
                    .filter((c) => c === "concede" || c === "start_game"),
            );
        assert.deepEqual(await commands(), [], "nothing yet");
        await page
            .getByRole("button", { name: "Concede & Start", exact: true })
            .click();
        for (let i = 0; i < 20 && (await commands()).length < 2; i++) {
            await page.clock.runFor(100);
        }
        assert.deepEqual(await commands(), ["concede", "start_game"]);
    },
    overAt,
);

// A launched app with no game in progress resumes the newest unfinished one saved
// (`resume_game`): here the views of `game-over` up to its last pass.
await writeFixture("game-over-launched", {
    messages: [
        { kind: "response", command: "current_game", value: null },
        {
            kind: "response",
            command: "resume_game",
            value: overEvents[0].payload.session,
        },
        ...overFixture.messages,
    ],
});
// Why no audit: the commands one click sends when the game was resumed at launch.
await check(
    "a game resumed at launch gives way to a new game at one click",
    async (page) => {
        await page
            .getByRole("button", { name: "Return to Game", exact: true })
            .waitFor();
        const commands = () =>
            page.evaluate(() =>
                window.berylFixture.calls
                    .map((c) => c.command)
                    .filter((c) => c === "concede" || c === "start_game"),
            );
        await page
            .getByRole("button", { name: "New Game", exact: true })
            .click();
        for (let i = 0; i < 20 && (await commands()).length < 2; i++) {
            await page.clock.runFor(100);
        }
        assert.deepEqual(await commands(), ["concede", "start_game"]);
    },
    {
        fixture: "game-over-launched",
        at: lastPass + 1,
        hash: "#/",
        ready: ".start-agent input:checked",
    },
);

// Why no audit: a frame cancelled while a bug report holds the page: the page's own clock, which no
// screen shows.
await check(
    "cancelled frames stay cancelled across bug report freeze",
    async (page) => {
        await page.evaluate(async () => {
            const { freeze } = await import("/src/freeze.ts");
            window.cancelledFrameCalls = 0;
            window.cancelledFrameId = requestAnimationFrame(
                () => window.cancelledFrameCalls++,
            );
            freeze();
        });
        await page.clock.runFor(50);
        await page.evaluate(async () => {
            cancelAnimationFrame(window.cancelledFrameId);
            (await import("/src/freeze.ts")).thaw();
        });
        await page.clock.runFor(100);
        assert.equal(await page.evaluate(() => window.cancelledFrameCalls), 0);
    },
    {
        fixture: "arrow-player",
        at: (await load("arrow-player")).length,
        ready: "[data-life]",
    },
);

// Why no audit: a frame asked for between two holds of a bug report: the page's own clock, which no
// screen shows.
await check(
    "a second bug report keeps thawed frames frozen",
    async (page) => {
        await page.evaluate(async () => {
            const { freeze, thaw } = await import("/src/freeze.ts");
            window.refrozenCalls = 0;
            freeze();
            requestAnimationFrame(() => window.refrozenCalls++);
            thaw();
            freeze();
        });
        await page.clock.runFor(100);
        assert.equal(await page.evaluate(() => window.refrozenCalls), 0);
        await page.evaluate(async () =>
            (await import("/src/freeze.ts")).thaw(),
        );
        await page.clock.runFor(100);
        assert.equal(await page.evaluate(() => window.refrozenCalls), 1);
    },
    {
        fixture: "arrow-player",
        at: (await load("arrow-player")).length,
        ready: "[data-life]",
    },
);

/** Asserts that `button` takes a click anywhere along its middle, at 10, 30, 50, 70 and 90%
 * of its width, with both of the opponent's piles on the table: nothing lies over it. */
async function assertReachable(page, button) {
    assert.equal(
        await page
            .locator(
                "[data-pile=graveyard][data-side=opponent], [data-pile=exile][data-side=opponent]",
            )
            .count(),
        2,
        "the opponent's graveyard and exile piles lie on the table",
    );
    const missed = await button.evaluate((el) => {
        const r = el.getBoundingClientRect();
        return [10, 30, 50, 70, 90].flatMap((pct) => {
            const x = r.left + (r.width * pct) / 100;
            const hit = document.elementFromPoint(x, r.top + r.height / 2);
            return el.contains(hit) ? [] : [`${pct}%: ${hit?.className}`];
        });
    });
    assert.deepEqual(missed, [], "every point of the button reaches it");
}

const lifeShown = async (page) =>
    Number(
        (
            await page
                .locator('[data-life="you"] .life-orb-total')
                .textContent()
        ).replace("\u2212", "-"),
    );

/** How long the stage holds the table still as a game ends, before the result covers it
 * (UI Conventions, Verbs, dwell, "game's end"), in ms. */
const endDwell = JSON.parse(
    await readFile(
        new URL("../../data/ui-conventions.json", import.meta.url),
        "utf8",
    ),
).verbs.find((verb) => verb.name === "dwell").cases["game's end"].duration;

await replied("game-over-pass", "game-over", [ANSWERED]);

// Why no audit: when a result comes up against the blow that ended the game: an order in time, and
// a result put aside and asked back.
await check(
    "the life-loss result waits for the killing blow and can be put aside",
    async (page) => {
        await page.clock.runFor(2000);
        await page.keyboard.press(" ");
        const result = page.getByRole("dialog", { name: "Defeat" });
        // When the damage shows.
        let lost;
        let t = 0;
        await committed(page);
        for (; t < 100 && !(await result.count()); t++) {
            lost ??= (await lifeShown(page)) < 0 ? t : undefined;
            await page.clock.runFor(100);
            await committed(page);
        }
        assert.ok(await result.count(), "the result shows");
        assert.ok(lost !== undefined && lost < t, "the damage shows first");
        // The view of the end reaches the screen once its script has played out, the table
        // held still at its end: the result waits out that hold after the blow.
        assert.ok(
            (t - lost) * 100 >= endDwell,
            `the table holds after the blow: ${lost}, then ${t}`,
        );
        assert.equal(
            await result.locator(".game-over-reason, .game-over-card").count(),
            0,
            "the life-loss result only shows the outcome and controls",
        );
        const entries = await page.locator(".game-log-entry").allTextContents();
        assert.ok(
            entries.some((e) => /You lost the game at -1 life/.test(e)),
            "the log says how",
        );
        await result
            .getByRole("button", { name: "View Battlefield", exact: true })
            .click();
        assert.equal(await result.count(), 0, "the battlefield shows");
        const back = page.getByRole("button", { name: "Back to Defeat" });
        await assertReachable(page, back);
        await back.click();
        // One capitalisation for New Game, here as on the start screen.
        await result
            .getByRole("button", { name: "New Game", exact: true })
            .waitFor();
    },
    {
        ...overAt,
        fixture: "game-over-pass",
    },
);
