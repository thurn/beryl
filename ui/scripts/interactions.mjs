// Drives the game screen through recorded fixtures with headless Playwright WebKit, as a
// player would: `by ui-test` (tools/) serves the UI and runs this. Usage:
//   node scripts/interactions.mjs <base-url> <fixtures-dir>
// The fixture `greedy-turn-9` ends on a target decision for a spell the human had just cast
// from their hand; the page starts one view earlier, at the priority decision, and the fixture
// transport plays the recorded views up to the next decision once the page calls `choose`
// (src/ipc.ts), which the page plays back paced (src/game/playback.ts) on Playwright's clock.
// `opponent-turn` plays out an opponent's turn to the session's halt in the human's. The combat fixtures
// `combat-declare` and `combat-double-block` end past the human's attack and block
// declarations, which the page makes whole and sends one pick at a time. Animation is
// measured on Playwright's clock, so the checks do not depend on the machine's speed, nor on
// how many run at once. The `choice-*` fixtures end on decisions the choice overlays ask. Exits
// non-zero if a check fails or a page logs an error. Filters after the fixtures folder run only
// the checks whose titles contain one of them, in any case (`just ui-test "drag"`).
import assert from "node:assert/strict";
import { readdir, readFile, writeFile } from "node:fs/promises";
import { basename, join } from "node:path";
import {
    slotOf as offeredSlot,
    triggerPlan,
    twins,
} from "../src/game/decisions.ts";
import { logLines } from "../src/game/log.ts";
import { strings } from "../src/cardRecords.ts";
import {
    coverage,
    cutText,
    launch,
    recordResult,
    workers,
} from "./capture.mjs";
import { audit, collect, record } from "./audit.mjs";
import { CHECK_SECONDS, HangError, hangGuard } from "./hang.mjs";

const [base, fixtures, ...filters] = process.argv.slice(2);
/** The checks the fast check chose (`by ui-test --select`, tools' selection.py), by exact title,
 * from the file `BERYL_CHECKS` names, one a line; null to run every check filters reach. */
const chosenTitles = process.env.BERYL_CHECKS
    ? new Set(
          (await readFile(process.env.BERYL_CHECKS, "utf8"))
              .split("\n")
              .filter(Boolean),
      )
    : null;
/** Whether `title` is reached: by any filter it contains or that contains it, and when checks
 * were chosen, by one that contains it. A filter longer than `title` that contains it opens a
 * block of checks named after it (`${title} (variant)`), so the K21 rerun, which passes the
 * failed checks' full titles, reaches each one. */
const reaches = (title, among) =>
    among.some((f) => {
        const [t, g] = [title.toLowerCase(), f.toLowerCase()];
        return t.includes(g) || g.includes(t);
    });
const wanted = (title) =>
    (filters.length === 0 || reaches(title, filters)) &&
    (!chosenTitles ||
        [...chosenTitles].some((c) =>
            c.toLowerCase().includes(title.toLowerCase()),
        ));
let ran = 0;
if (!base || !fixtures) {
    console.error(
        "usage: node scripts/interactions.mjs <base-url> <fixtures-dir> [title-filter...]",
    );
    process.exit(2);
}

/** A recorded fixture's views, oldest first. */
async function load(name) {
    const fixture = JSON.parse(
        await readFile(join(fixtures, `${name}.json`), "utf8"),
    );
    return fixture.messages
        .filter((m) => m.kind === "event")
        .map((m) => m.payload.view);
}

/** The card records a fixture recorded (its `card_records` response): what its page draws
 * its cards from. */
async function cardRecords(name) {
    const fixture = JSON.parse(
        await readFile(join(fixtures, `${name}.json`), "utf8"),
    );
    return fixture.messages.find(
        (m) => m.kind === "response" && m.command === "card_records",
    ).value;
}

// Every fixture recorded for this run, before this script writes one of its own.
const recorded = (await readdir(fixtures))
    .filter((f) => f.endsWith(".json"))
    .map((f) => f.slice(0, -".json".length));

/** The records of every card the run's recordings show, by the name asked. */
const recordedCards = { cards: {}, tokens: {} };
for (const name of recorded) {
    const { cards, tokens, ...art } = await cardRecords(name);
    Object.assign(recordedCards, art);
    Object.assign(recordedCards.cards, cards);
    Object.assign(recordedCards.tokens, tokens);
}

/** Writes the fixture `name` with the records of the cards it shows, as `beryl_cli
 * record-fixture` gives a recording its own: here from those the run's recordings hold,
 * whichever game showed the card, so a view cut, edited or renamed draws the cards it names. */
async function writeFixture(name, fixture) {
    const messages = fixture.messages.filter(
        (m) => m.command !== "card_records",
    );
    const shown = strings(messages);
    const among = (records) =>
        Object.fromEntries(
            Object.entries(records).filter(([asked]) => shown.has(asked)),
        );
    const cards = {
        kind: "response",
        command: "card_records",
        value: {
            ...recordedCards,
            cards: among(recordedCards.cards),
            tokens: among(recordedCards.tokens),
        },
    };
    await writeFile(
        join(fixtures, `${name}.json`),
        JSON.stringify({ ...fixture, messages: [cards, ...messages] }),
    );
}

const FIXTURE = "greedy-turn-9";
/** A card's width over its height (Card.tsx's CARD_ASPECT). */
const CARD_ASPECT = 63 / 88;
const views = await load(FIXTURE);
const [before, after] = views.slice(-2);

// The spell cast between the last two views, and the candidate that cast it.
const cast = after.events.find(
    (e) => e.kind === "zoneChange" && e.from === "hand" && e.to === "stack",
);
assert.ok(cast, "the fixture's last view follows a cast from the hand");
const slot = cast.slot;
const decision = before.status.decision;
const castIndex = decision.candidates.findIndex(
    (c) => c.option.kind === "cast" && c.option.slot === slot,
);
assert.ok(castIndex >= 0, "the view before offers the cast");
const castLabel = decision.candidates[castIndex].label;
const name = before.objects.find((o) => o.slot === slot).name;
const card = `[data-slot="${slot}"]`;

/** The `choose` commands the page has sent, as recorded by the fixture transport. */
const chosen = (page) =>
    page.evaluate(() =>
        window.berylFixture.calls
            .filter((c) => c.command === "choose")
            .map((c) => c.args.request.index),
    );

/** Where an element's box is, rounded to whole stage pixels. */
const where = (handle) =>
    handle.evaluate((e) => {
        const r = e.getBoundingClientRect();
        return [Math.round(r.left), Math.round(r.top)];
    });

/** Asserts that the points `[x, y]` `a` and `b` lie within 3 px of each other. */
function assertNear(a, b, message) {
    assert.ok(
        Math.hypot(a[0] - b[0], a[1] - b[1]) <= 3,
        `${message}: at ${a}, not ${b}`,
    );
}

/** Asserts that every point of `path` lies in the box `from` and `to` span, give or take a
 * spring's overshoot: a card travels between them rather than by way of anywhere else. */
function assertBetween(path, from, to, message) {
    const slack = 30;
    for (const [x, y] of path) {
        assert.ok(
            x >= Math.min(from[0], to[0]) - slack &&
                x <= Math.max(from[0], to[0]) + slack &&
                y >= Math.min(from[1], to[1]) - slack &&
                y <= Math.max(from[1], to[1]) + slack,
            `${message} passes ${x},${y}, off its way from ${from} to ${to}`,
        );
    }
}

/** The center of a bounding box. */
const center = (box) => ({
    x: box.x + box.width / 2,
    y: box.y + box.height / 2,
});

const duplicatesIn = (recording) =>
    audit({ ...recording, shown: [] }).filter((f) => f.kind === "duplicate");

const posesOf = (recording, slot) =>
    recording.frames.flatMap((f) =>
        f.instances.filter((c) => c.slot === String(slot) && c.o > 0.3),
    );

const motionSizes = [
    { width: 1600, height: 900 },
    { width: 1180, height: 820 },
];
const motionConfigs = motionSizes.flatMap((viewport) =>
    ["no-preference", "reduce"].map((reducedMotion) => ({
        viewport,
        reducedMotion,
    })),
);

const browser = await launch();
const errors = [];
let failed = false;

// Six pages at once, not four: ui-test is slow CI's longest lane, and on Windows it finished
// alone on 8 of 20 cores, 12 s over the suite's budget. Six cut a run from 148 s to 125 s here.
const queue = workers(6);

/** Pauses the page's clock where it stands: from then on the page's time moves only when the
 * check moves it (`clock.runFor` and friends). `pauseAt` only moves forward, and the page's time
 * runs on while the call travels, so it aims a frame ahead, and further if that was too close. */
async function pause(page) {
    for (let ahead = 16; ; ahead *= 2) {
        const now = await page.evaluate(() => Date.now());
        try {
            await page.clock.pauseAt(now + ahead);
            return;
        } catch (e) {
            if (!String(e).includes("to the past") || ahead > 60_000) throw e;
        }
    }
}

/** Runs `body` on a fresh page of a fixture's first `at` views, once `ready` matches and its
 * fonts have loaded, alongside other checks (capture.mjs' workers): awaiting it waits only for
 * a free worker. The page's fixtures come from `fixtures`, not from what Vite serves. The
 * page runs on Playwright's fake clock, paused once the page is ready so that a check cannot
 * race the wall clock; `clock: "off"` leaves the browser's own, and says why. */
async function check(
    title,
    body,
    {
        fixture = FIXTURE,
        at = views.length - 1,
        ready = `${card}[data-zone="hand"]`,
        viewport = { width: 1600, height: 900 },
        reducedMotion = "no-preference",
        hash = "#/game",
        init,
        phone = false,
        clock = "paused",
    } = {},
) {
    if (!wanted(title) || (chosenTitles && !chosenTitles.has(title))) return;
    ran += 1;
    await queue(async () => {
        // A page whose boot lost one of its modules to the network (Windows now and then
        // runs out of socket buffers, net::ERR_NO_BUFFER_SPACE, beside the slow suite's other
        // lanes) never shows the board: boot it once more, in a fresh context. Any other
        // failure, and any after the board shows, fails the check.
        const started = performance.now();
        for (let boots = 1; ; boots++) {
            const context = await browser.newContext({
                viewport,
                deviceScaleFactor: 1,
                reducedMotion,
                hasTouch: phone,
            });
            await context.route("**/fixtures/*.json", (route) =>
                route.fulfill({
                    path: join(
                        fixtures,
                        decodeURIComponent(
                            basename(new URL(route.request().url()).pathname),
                        ),
                    ),
                }),
            );
            const page = await context.newPage();
            const finish = await coverage(page);
            let covered = null;
            const seen = [];
            page.on("pageerror", (e) => seen.push(`${title}: ${e.message}`));
            page.on(
                "console",
                (m) =>
                    m.type() === "error" && seen.push(`${title}: ${m.text()}`),
            );
            let booted = false;
            let threw = false;
            let hung = false;
            const play = async () => {
                assert.ok(["paused", "off"].includes(clock), clock);
                if (clock === "paused") await page.clock.install();
                if (init) await page.addInitScript(init);
                const route = `/?fixture=${fixture}&at=${at}${phone ? "&phone" : ""}${hash}`;
                await page.goto(new URL(route, base).href, {
                    waitUntil: "load",
                });
                await page.waitForSelector(ready);
                booted = true;
                await page.evaluate(() => document.fonts.ready.then(() => {}));
                if (clock === "paused") await pause(page);
                await body(page);
                await assertSounded(page);
            };
            try {
                await hangGuard(play(), CHECK_SECONDS, title).catch((e) => {
                    hung = e instanceof HangError;
                    throw e;
                });
                console.log(`ok: ${title}`);
            } catch (e) {
                const lost = seen.some((s) => s.includes("net::ERR_"));
                if (!booted && lost && boots < 2) continue;
                failed = true;
                threw = true;
                console.error(`FAILED: ${title}\n${e.stack ?? e}`);
            } finally {
                // A page that hung would hold its coverage too: closing the context ends it.
                if (!hung) covered = await finish();
                await context.close();
            }
            errors.push(...seen);
            // A page error fails the run (below) and, in the run log, the check whose page it was.
            recordResult(
                `ui-test:${title}`,
                threw || seen.length ? "fail" : "pass",
                (performance.now() - started) / 1000,
                covered,
            );
            break;
        }
    });
}

/** Across every check: the views shown live whose sounds were checked, and the player's
 * commands heard. */
const sounded = { views: 0, heard: 0 };

/** The sounds the page has asked for so far (src/audio), oldest first. */
const cues = (page) =>
    page.evaluate(() => (window.berylSounds ?? []).map((s) => s.sound));

/** Anything that happens on the screen makes a sound (src/audio): every view the page showed
 * live asked, as it showed, for the sounds of its events (audio/events.ts holds one for every
 * kind), and a page that sent the player's answers played their sound. The page logs its cues
 * (`window.berylSounds`) rather than playing them. */
async function assertSounded(page) {
    const { silent, views, sent, heard } = await page.evaluate(() => {
        const sounds = window.berylSounds ?? [];
        const live = (window.berylShown ?? []).filter((s) => s.live);
        const silent = live.flatMap((s) =>
            window
                .berylBeatSounds(s.update.view)
                .filter(
                    (c) => !sounds.some((x) => x.sound === c && x.at === s.at),
                )
                .map((c) => `${c} at ${Math.round(s.at)} ms`),
        );
        const commands = ["choose", "cancel", "undo", "pass_on"];
        return {
            silent,
            views: live.length,
            sent: (window.berylFixture?.calls ?? []).filter((c) =>
                commands.includes(c.command),
            ).length,
            heard: sounds.filter((x) => ["select", "back"].includes(x.sound))
                .length,
        };
    });
    assert.deepEqual(silent, [], "every view shown live asked for its sounds");
    // Picks the page takes for the player (a lone target, a plan's next pick) go unheard.
    if (sent > 0) assert.ok(heard > 0, `${sent} answers sent, none heard`);
    sounded.views += views;
    sounded.heard += heard;
}

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
    { clock: "off" },
);

/** The `cancel` commands the page has sent. */
const cancels = (page) =>
    page.evaluate(
        () =>
            window.berylFixture.calls.filter((c) => c.command === "cancel")
                .length,
    );

assert.equal(
    decision.candidates[castIndex].payments.length,
    1,
    "the cast is paid one way: nothing for a cast step to ask",
);
await check(
    "a click casts a card with nothing to choose, and Escape takes it back",
    async (page) => {
        await page.clock.runFor(2000);
        const handle = await page.$(card);
        const inHand = await where(handle);
        await page.clock.pauseAt(Date.now() + 60_000);
        await page.click(card);
        assert.deepEqual(
            await chosen(page),
            [castIndex],
            `chose "${castLabel}"`,
        );
        await page.waitForSelector(`${card}[data-zone="stack"]`);
        // It goes to the stack from its place in the hand.
        assertNear(await where(handle), inHand, "the clicked card's start");
        const flight = [];
        for (let i = 0; i < 6; i++) {
            await page.clock.runFor(16);
            flight.push(await where(handle));
        }
        await page.clock.runFor(3000);
        assertBetween(flight, inHand, await where(handle), "its flight");
        // It asks for its target; Escape takes the cast back, and the card is in hand again.
        const targeting = page.getByRole("dialog", {
            name: `(${name}) Choose a target.`,
        });
        await targeting.waitFor();
        await page.keyboard.press("Escape");
        await page.waitForSelector(`${card}[data-zone="hand"]`);
        assert.equal(await cancels(page), 1, "cancelled");
        assert.equal(await targeting.count(), 0, "the target is not asked");
        // Cast again, its prompt's Cancel button takes it back too.
        await page.clock.runFor(2000);
        await page.click(card);
        await targeting
            .getByRole("button", { name: "Cancel", exact: true })
            .click();
        await page.waitForSelector(`${card}[data-zone="hand"]`);
        assert.equal(await cancels(page), 2, "cancelled again");
        assert.deepEqual(await chosen(page), [castIndex, castIndex]);
    },
);

await check(
    "the spell being cast, dragged back to the hand, is taken back",
    async (page) => {
        await page.clock.runFor(2000);
        await page.click(card);
        const onStack = `${card}[data-zone="stack"]`;
        await page.waitForSelector(onStack);
        await page
            .getByRole("dialog", { name: `(${name}) Choose a target.` })
            .waitFor();
        await page.clock.runFor(1000);
        const from = center(await page.locator(onStack).boundingBox());
        await drag(page, onStack, { by: { x: 700 - from.x, y: 870 - from.y } });
        await page.waitForSelector(`${card}[data-zone="hand"]`);
        assert.equal(await cancels(page), 1, "cancelled");
    },
);

await check("Space passes priority", async (page) => {
    await page.clock.runFor(2000);
    await page.keyboard.press("Space");
    const pass = decision.candidates.findIndex((c) => c.option.kind === "pass");
    assert.deepEqual(await chosen(page), [pass], "passed");
});

await check(
    "the turn button is reached by Tab, with a visible ring",
    async (page) => {
        await page.clock.runFor(2000);
        const turn = page.locator(".turn-action button");
        for (let i = 0; i < 40; i++) {
            if (await turn.evaluate((b) => b === document.activeElement)) break;
            await page.keyboard.press("Tab");
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

await check("Escape closes the settings", async (page) => {
    await page.clock.runFor(2000);
    await page.getByRole("button", { name: "Settings" }).click();
    const panel = page.locator(".settings");
    await panel.waitFor();
    await page.keyboard.press("Escape");
    assert.equal(await panel.count(), 0, "closed");
});

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

await check(
    "a click on the visible strip of each hand card acts on that card",
    async (page) => {
        await page.clock.runFor(2000);
        // Each hand card's strip in the fan, at rest: where it is the card the pointer hits,
        // as a row of points across the near edge.
        const strips = await page.evaluate(() => {
            const cards = [
                ...document.querySelectorAll('.card-el-hand[data-zone="hand"]'),
            ];
            const y = innerHeight - 40;
            const out = [];
            for (const el of cards) {
                const box = el.getBoundingClientRect();
                const xs = [];
                for (let x = Math.ceil(box.left); x < box.right; x += 2) {
                    const hit = document.elementFromPoint(x, y);
                    if (hit?.closest("[data-slot]") === el) xs.push(x);
                }
                out.push({ slot: Number(el.dataset.slot), xs, y });
            }
            return out.sort((a, b) => a.xs[0] - b.xs[0]);
        });
        assert.ok(strips.length >= 3, "a fan of cards");
        // Every click is caught before the board hears of it, and the card it lands on noted.
        await page.evaluate(() => {
            window.hits = [];
            for (const type of ["pointerdown", "pointerup", "click"]) {
                window.addEventListener(
                    type,
                    (e) => {
                        if (type === "pointerdown") {
                            const el = e.target.closest?.("[data-slot]");
                            window.hits.push(Number(el?.dataset.slot));
                        }
                        e.stopPropagation();
                    },
                    true,
                );
            }
        });
        const mid = ({ xs, y }) => ({ x: xs[xs.length >> 1], y });
        for (const [i, strip] of strips.entries()) {
            assert.ok(strip.xs.length > 4, `slot ${strip.slot} shows a strip`);
            // With each neighbor raised, the edge of the strip next to it.
            for (const n of [i - 1, i + 1]) {
                const neighbor = strips[n];
                if (!neighbor) continue;
                const raised = mid(neighbor);
                await page.mouse.move(raised.x, raised.y, { steps: 4 });
                await page.clock.runFor(500);
                const x =
                    n < i ? strip.xs[0] + 4 : strip.xs[strip.xs.length - 1] - 4;
                await page.mouse.move(x, strip.y, { steps: 4 });
                await page.clock.runFor(500);
                await page.mouse.down();
                await page.mouse.up();
                const hits = await page.evaluate(() => window.hits);
                assert.equal(
                    hits.at(-1),
                    strip.slot,
                    `a click on slot ${strip.slot}'s strip beside raised slot ${neighbor.slot}`,
                );
            }
        }
    },
);

await check("hovering a card shows it whole, with its artist", async (page) => {
    await page.clock.runFor(2000);
    await page.hover(card);
    const { artist } = (await cardRecords(FIXTURE)).cards[name];
    await page
        .locator(".inspect")
        .getByText(`Illustrated by ${artist}`)
        .waitFor();
});

/** Where every card is drawn, by slot: the box of what its element draws (a raised hand card
 * rises inside its element, which a copy over the hand shows), and whether a layout animation
 * is moving its frame. */
const drawn = (page) =>
    page.evaluate(() =>
        Object.fromEntries(
            [...document.querySelectorAll(".card-el[data-slot]")].flatMap(
                (e) => {
                    const look = e.firstElementChild;
                    if (!look) return [];
                    const r = look.getBoundingClientRect();
                    const frame =
                        e.querySelector(".card-slot")?.style.transform;
                    return [
                        [
                            e.dataset.slot,
                            {
                                box: [r.left, r.top, r.right, r.bottom],
                                animating: !!frame && frame !== "none",
                            },
                        ],
                    ];
                },
            ),
        ),
    );

const fieldCard = `[data-slot="${
    before.objects.find(
        (o) => o.zone === "battlefield" && !o.types.includes("land"),
    ).slot
}"]`;
for (const [title, target] of [
    ["a hand card", card],
    ["a battlefield card", fieldCard],
]) {
    await check(
        `hovering ${title} moves no other card and starts no layout animation`,
        async (page) => {
            await page.clock.runFor(2000);
            const still = await drawn(page);
            const slot = await page.$eval(target, (e) => e.dataset.slot);
            const box = await page.locator(target).boundingBox();
            const at = { x: box.x + box.width / 2, y: box.y + 40 };
            await page.mouse.move(at.x, at.y);
            for (let t = 0; t < 20; t++) {
                await page.clock.runFor(50);
                const now = await drawn(page);
                for (const [other, { box, animating }] of Object.entries(now)) {
                    assert.ok(!animating, `slot ${other} animates its layout`);
                    if (other === slot) continue;
                    const moved = box.map((v, i) =>
                        Math.abs(v - still[other].box[i]),
                    );
                    assert.ok(
                        Math.max(...moved) <= 1,
                        `slot ${other} moved by ${moved} at ${t * 50} ms`,
                    );
                }
            }
            const top = (await drawn(page))[slot].box[1];
            if (target === card) {
                assert.ok(
                    still[slot].box[1] - top > 40,
                    "the hovered hand card rises",
                );
            } else {
                assert.equal(top, still[slot].box[1], "a field card stays");
            }
            // The card is drawn raised inside its element, which stays put: wherever the pointer
            // found the card, it still finds it, so the hover holds rather than flickering.
            const held = await page.locator(target).boundingBox();
            for (const k of ["x", "y", "width", "height"]) {
                assert.ok(
                    Math.abs(held[k] - box[k]) <= 1,
                    `the hovered card's element moved: ${k} ${box[k]} to ${held[k]}`,
                );
            }
            assert.equal(
                await page.evaluate(
                    ({ x, y }) =>
                        document.elementFromPoint(x, y)?.closest("[data-slot]")
                            ?.dataset.slot,
                    at,
                ),
                slot,
                "the pointer is still over the hovered card",
            );
            assert.ok(await page.$(".inspect"), "the card is inspected");
        },
    );
}

// A quiet table with a row of creatures a side: nothing on the stack, no combat.
const quietViews = await load("combat-blockers");
const nonlands = (view, side) =>
    view.objects.filter(
        (o) =>
            o.zone === "battlefield" &&
            o.controller === side &&
            !o.types.includes("land"),
    );
// The creatures' row (layout.ts ROWS): the other permanents stand behind it, beside the lands.
const creatures = (view, side) =>
    nonlands(view, side).filter((o) => o.types.includes("creature"));
const quiet = quietViews.findIndex(
    (v) =>
        v.status.decision?.kind === "priority" &&
        v.stack.length === 0 &&
        !v.objects.some((o) => o.combat) &&
        creatures(v, "you").length >= 3 &&
        creatures(v, "opponent").length >= 3,
);
assert.ok(quiet >= 0, "the blockers fixture has a quiet, crowded table");
for (const viewport of [
    { width: 1280, height: 800 },
    { width: 1600, height: 900 },
]) {
    await check(
        `each battlefield row is centered as near as the stack allows at ${viewport.width} wide`,
        async (page) => {
            await page.clock.runFor(2000);
            const stage = await page.locator(".stage").boundingBox();
            const middle = stage.x + stage.width / 2;
            // A row too long to center left of the stack's column (layout.ts ROOM, which the
            // stack's coming and going never changes) reaches that column's edge instead,
            // short of it by no more than the lean of its last card.
            const scale = stage.width / 1600;
            const roomEnd = stage.x + 1160 * scale;
            const boxes = async (objects) =>
                Promise.all(
                    objects.map((o) =>
                        page.locator(`[data-slot="${o.slot}"]`).boundingBox(),
                    ),
                );
            for (const side of ["you", "opponent"]) {
                const whose = side === "you" ? "your" : "the opponent's";
                const row = await boxes(creatures(quietViews[quiet], side));
                assert.ok(row.length > 0, `${whose} row is empty`);
                const left = Math.min(...row.map((b) => b.x));
                const right = Math.max(...row.map((b) => b.x + b.width));
                const centered = Math.abs((left + right) / 2 - middle) <= 3;
                const flush =
                    (left + right) / 2 < middle &&
                    Math.abs(right - roomEnd) <= 12 * scale;
                assert.ok(
                    centered || flush,
                    `${whose} row spans ${left} to ${right}, centered on ${middle} ` +
                        `or reaching ${roomEnd}`,
                );
            }
            await assertClearOfLands(page);
        },
        {
            fixture: "combat-blockers",
            at: quiet + 1,
            ready: `[data-slot="${creatures(quietViews[quiet], "you")[0].slot}"]`,
            viewport,
        },
    );
}

// A fixture's history reaches the log once, however often the screen starts listening (React's
// StrictMode mounts it twice), so no turn's heading comes twice.
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
    { fixture: "opponent-turn", at: 22, ready: ".game-log-toggle" },
);

// The log names what an ability did and to what: an activation its source, its loyalty cost
// (the counters paid go unsaid) and its targets, and a trigger with targets its source and them,
// so a mill or a bounce that follows has its cause.
const abilityLog = "the log names an ability's source, cost and targets";
if (wanted(abilityLog)) {
    const started = performance.now();
    const ability = (source, name, targets, triggered = false) => ({
        kind: "ability",
        source,
        ability: 0,
        granted: false,
        name,
        triggered,
        nth: 0,
        controller: "opponent",
        targets,
        x: 0,
    });
    const view = {
        turn: { number: 19, active: "opponent", step: "precombatMain" },
        status: { kind: "playing" },
        objects: [{ slot: 30, owner: "you", controller: "you" }],
        stack: [],
        events: [
            { kind: "countersAdded", slot: 7, counter: "loyalty", amount: 1 },
            {
                kind: "ability",
                item: ability(7, "Jace, Wielder of Mysteries", [
                    { kind: "player", player: "you" },
                ]),
                loyalty: 1,
            },
            {
                kind: "zoneChange",
                slot: 30,
                from: "library",
                to: "graveyard",
                reason: "effect",
            },
            { kind: "countersRemoved", slot: 5, counter: "loyalty", amount: 3 },
            {
                kind: "ability",
                item: ability(5, "Teferi, Time Raveler", [
                    { kind: "object", slot: 9 },
                ]),
                loyalty: -3,
            },
            {
                kind: "ability",
                item: ability(
                    11,
                    "Subtlety",
                    [{ kind: "object", slot: 9 }],
                    true,
                ),
                loyalty: null,
            },
        ],
    };
    const names = new Map([
        [5, "Teferi, Time Raveler"],
        [7, "Jace, Wielder of Mysteries"],
        [9, "Snapcaster Mage"],
        [30, "Island"],
    ]);
    assert.deepEqual(
        logLines(view, names).map((l) => l.text),
        [
            "Opponent activated Jace, Wielder of Mysteries +1 targeting you",
            "Island was put into your graveyard",
            "Opponent activated Teferi, Time Raveler \u22123 targeting Snapcaster Mage",
            "Subtlety triggered targeting Snapcaster Mage",
        ],
    );
    const title = abilityLog;
    ran += 1;
    console.log(`ok: ${title}`);
    recordResult(
        `ui-test:${title}`,
        "pass",
        (performance.now() - started) / 1000,
    );
}

// The log rests at its newest entry, even one laid out while hidden (by an ancestor), which has
// no size to scroll in until it shows: it would show its first turn.
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
        for (let i = 0; i < 30 && top + height < full - 2; i++) {
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

/** Asserts that no permanent on the table covers a land: a long row of permanents may pass
 * over the land piles' side of the table, but in a row of its own. */
async function assertClearOfLands(page) {
    const boxes = await page.evaluate(() =>
        [...document.querySelectorAll('[data-zone="battlefield"]')].map((e) => {
            const r = e.getBoundingClientRect();
            return {
                name: e.dataset.card,
                land: e.classList.contains("card-el-land"),

                x: r.x,
                y: r.y,
                right: r.right,
                bottom: r.bottom,
            };
        }),
    );
    for (const land of boxes.filter((b) => b.land)) {
        for (const b of boxes.filter((b) => !b.land)) {
            const across =
                Math.min(land.right, b.right) - Math.max(land.x, b.x);
            const down =
                Math.min(land.bottom, b.bottom) - Math.max(land.y, b.y);
            assert.ok(
                across <= 1 || down <= 1,
                `${b.name} covers ${land.name} by ${across} by ${down} px`,
            );
        }
    }
}

/** Whether view `v` waits for the human: the views of the game playing on without them
 * follow one another unasked. */
const awaits = (v) =>
    v.status.kind !== "thinking" && v.status.kind !== "playing";

/** Runs the page's clock `step` milliseconds at a time until `selector` matches, for at most
 * a minute of page time; returns how long that took. */
async function until(page, selector, step = 100) {
    for (let ran = 0; ran <= 60_000; ran += step) {
        if (await page.$(selector)) return ran;
        await page.clock.runFor(step);
    }
    throw new Error(`${selector} never appeared`);
}

/** Steps the clock until nothing matches `selector`, failing if it outlasts `limit` ms. A
 * timer set in a React effect starts when the effect flushes, which is real time and not the
 * paused clock's, so a fixed `runFor` past its duration can land before it fires. */
async function untilGone(page, selector, limit, step = 16) {
    for (let ran = 0; ran <= limit; ran += step) {
        if (!(await page.$(selector))) return ran;
        await page.clock.runFor(step);
    }
    throw new Error(`${selector} outlasted ${limit} ms`);
}

/** Lets React commit what the clock's last step, or the pointer, set off. Its scheduler posts
 * each render as a MessageChannel message, which Playwright's clock does not drive, so under
 * load a poll made right after could see the page a step behind; a message posted now lands
 * after it. */
function committed(page) {
    return page.evaluate(
        () =>
            new Promise((resolve) => {
                const channel = new MessageChannel();
                channel.port1.onmessage = () => resolve();
                channel.port2.postMessage(null);
            }),
    );
}

/** Starts audit.mjs' frame recorder, stepping the paused clock a frame at a time until it has
 * the few frames it takes before anything is set going. */
async function startRecording(page) {
    let started = false;
    const starting = page.evaluate(record).finally(() => (started = true));
    while (!started) await page.clock.runFor(16);
    await starting;
}

/** Drags the element `from` matches by its center to `to`'s center, or by `by`, in steps. */
async function drag(page, from, { to, by }) {
    const start = center(await page.locator(from).boundingBox());
    const end = to
        ? center(await page.locator(to).boundingBox())
        : { x: start.x + by.x, y: start.y + by.y };
    await page.mouse.move(start.x, start.y);
    await page.mouse.down();
    for (let i = 1; i <= 10; i++) {
        await page.mouse.move(
            start.x + ((end.x - start.x) * i) / 10,
            start.y + ((end.y - start.y) * i) / 10,
        );
    }
    await page.mouse.up();
}

/** The `choose` commands sent once the page has had time to answer, at most `count`. */
async function settled(page, count) {
    for (let i = 0; i < 20 && (await chosen(page)).length < count; i++) {
        await page.clock.runFor(100);
    }
    return chosen(page);
}

// The opponent's turn played back: `opponent-turn` ends on the session's halt at the human's
// precombat main (End Turn), the first since a turn in which the opponent plays a land and casts
// spells, and which stops the human nowhere: they hold nothing to cast there. The page starts at
// the human's last decision before that turn, a look at the top of their library.
const turnViews = await load("opponent-turn");
const mainHalt = turnViews.length - 1;
const lookAt = turnViews.findLastIndex((v, i) => i < mainHalt && awaits(v));
assert.ok(
    turnViews[mainHalt].status.decision?.halt,
    "the fixture ends on a halt",
);
assert.equal(
    turnViews[lookAt].status.decision?.prompt.purpose,
    "look",
    "the human looks at their library before the opponent's turn",
);
assert.ok(
    turnViews
        .slice(lookAt + 1, mainHalt)
        .some((v) => v.turn.active === "opponent"),
    "the opponent's turn comes between the look and the halt",
);
const opponentLand = turnViews
    .slice(lookAt + 1, mainHalt + 1)
    .flatMap((v) =>
        v.events.filter(
            (e) =>
                e.kind === "zoneChange" &&
                e.reason === "play" &&
                v.objects.find((o) => o.slot === e.slot)?.owner === "opponent",
        ),
    )[0];
assert.ok(opponentLand, "the opponent plays a land that turn");
const landName = turnViews[mainHalt].objects.find(
    (o) => o.slot === opponentLand.slot,
).name;
// The spells that resolve into a graveyard that turn, which would otherwise vanish unseen.
const vanished = turnViews
    .slice(lookAt + 1, mainHalt + 1)
    .flatMap((v) =>
        v.events.flatMap((e) =>
            e.kind === "zoneChange" &&
            e.from === "stack" &&
            e.to === "graveyard"
                ? [v.objects.find((o) => o.slot === e.slot).name]
                : [],
        ),
    );
assert.ok(vanished.length > 0, "a spell resolves into a graveyard");

await check(
    "Space during the opponent's turn shows the rest of it at once",
    async (page) => {
        await page.clock.runFor(2000);
        // From here on time passes only when the test says so: the replay cannot advance.
        await page.clock.pauseAt(Date.now() + 60_000);
        await page.getByRole("button", { name: "Decline" }).click();
        await page.waitForSelector(".table:not(.table-dimmed)");
        // The turn plays out to the session's halt at the human's main: End Turn.
        const halt = page.getByRole("button", { name: "End Turn" });
        assert.ok(
            !((await halt.isVisible()) && (await halt.isEnabled())),
            "the turn is still playing out",
        );
        await page.keyboard.press("Space");
        // The turn's last view at once, with nothing it did lost.
        await page.waitForSelector(
            "button:has-text('End Turn'):not([disabled])",
        );
        await page.waitForSelector(
            `[data-slot="${opponentLand.slot}"][data-zone="battlefield"]`,
        );
        const log = await page.locator(".game-log").textContent();
        assert.ok(
            log.includes(`Opponent played ${landName}`),
            `the log says the land was played: ${log}`,
        );
        assert.equal((await chosen(page)).length, 1, "Space passed nothing");
        // The next press acts on it.
        await page.keyboard.press("Space");
        assert.deepEqual((await chosen(page)).slice(1), [0], "ended the turn");
    },
    { fixture: "opponent-turn", at: lookAt + 1, ready: ".choice" },
);

// A pass that the game answers with several views before the human's next priority, at which
// they may play a land that was in their hand all along.
const replayed = views.flatMap((v, i) => {
    const next = views.findIndex((w, j) => j > i && awaits(w));
    const between = views.slice(i + 1, next).filter((w) => w.events.length);
    const pass = v.status.decision?.candidates.findIndex(
        (c) => c.option.kind === "pass",
    );
    const land = views[next]?.status.decision?.candidates.findIndex(
        (c) =>
            c.option.kind === "playLand" &&
            v.objects.some(
                (o) => o.slot === c.option.slot && o.zone === "hand",
            ),
    );
    return v.status.decision?.kind === "priority" &&
        pass >= 0 &&
        between.length >= 3 &&
        land >= 0
        ? [{ at: i, pass, next, land }]
        : [];
})[0];
assert.ok(replayed, "a pass is followed by several views, then a land drop");
const replayedLand = `[data-slot="${
    views[replayed.next].status.decision.candidates[replayed.land].option.slot
}"]`;

await check(
    "a land clicked while the game plays on to it is played",
    async (page) => {
        await page.clock.runFor(2000);
        await page.clock.pauseAt(Date.now() + 60_000);
        await page.keyboard.press("Space");
        assert.deepEqual(await chosen(page), [replayed.pass], "passed");
        const box = await page.locator(replayedLand).boundingBox();
        assert.ok(
            !(await page.$(`${replayedLand}.card-el-playable`)),
            "the land is not yet playable",
        );
        // Pressed during the replay, which ends; let go at the land drop, which plays it.
        await page.mouse.click(box.x + box.width / 2, box.y + 40);
        assert.deepEqual(
            await chosen(page),
            [replayed.pass, replayed.land],
            "played the land",
        );
    },
    {
        at: replayed.at + 1,
        ready: `${replayedLand}[data-zone="hand"]`,
    },
);

/** A recorded combat declaration: the last run of the human's decisions of `kinds` in
 * `views`, the `at` that opens a page on its first, the candidate each was answered with, as
 * the view after it shows (the creature newly attacking or blocking, the attacker the blocker
 * waiting for one blocks, or done), and the view after the last, which shows the whole
 * declaration before combat moves on. */
function recordedDeclaration(views, kinds) {
    const asked = views.flatMap((v, i) =>
        v.status.kind === "deciding" ? [[v, i]] : [],
    );
    const of = (k) => kinds.includes(asked[k][0].status.decision.kind);
    let last = asked.length - 1;
    while (!of(last)) last -= 1;
    let first = last;
    while (first > 0 && of(first - 1)) first -= 1;
    const combat = (view, slot) =>
        view.objects.find((o) => o.slot === slot)?.combat;
    const picks = [];
    for (let k = first; k <= last; k++) {
        const [view] = asked[k];
        const next = views[asked[k][1] + 1] ?? views.at(-1);
        const { candidates } = view.status.decision;
        const waiting = view.objects.find(
            (o) => o.combat?.kind === "blocking" && o.combat.attacker === null,
        );
        const index = candidates.findIndex(({ option: o }) =>
            o.kind === "attack" || o.kind === "block"
                ? !combat(view, o.slot) &&
                  combat(next, o.slot)?.kind ===
                      (o.kind === "attack" ? "attacking" : "blocking")
                : o.kind === "object" &&
                  combat(next, waiting.slot)?.attacker === o.slot,
        );
        picks.push(
            index >= 0
                ? index
                : candidates.findIndex((c) => c.option.kind === "done"),
        );
    }
    return {
        at: asked[first][1] + 1,
        view: asked[first][0],
        picks,
        declared: views[asked[last][1] + 1] ?? views.at(-1),
    };
}

const slotOf = (slot) => `[data-slot="${slot}"]`;
/** Clicks the card in `slot` where it shows, nearest its middle and well inside its edges: a
 * copy beneath a pile's top card shows only its edge. */
async function clickShown(page, slot) {
    const box = await page.locator(slotOf(slot)).boundingBox();
    const at = await page.evaluate(
        ({ slot, box }) => {
            const points = [];
            for (let y = box.y + 12; y < box.y + box.height - 12; y += 4) {
                for (let x = box.x + 6; x < box.x + box.width - 6; x += 2) {
                    points.push({ x, y });
                }
            }
            const middle = {
                x: box.x + box.width / 2,
                y: box.y + box.height / 2,
            };
            points.sort(
                (a, b) =>
                    Math.hypot(a.x - middle.x, a.y - middle.y) -
                    Math.hypot(b.x - middle.x, b.y - middle.y),
            );
            return (
                points.find(
                    ({ x, y }) =>
                        document.elementFromPoint(x, y)?.closest("[data-slot]")
                            ?.dataset.slot === String(slot),
                ) ?? null
            );
        },
        { slot, box },
    );
    assert.ok(at, `slot ${slot} shows`);
    await page.mouse.click(at.x, at.y);
}
/** Whether the card in `slot` shows `role` ("attacker" or "blocker") on the table. */
const shows = async (page, slot, role) =>
    Boolean(await page.$(`${slotOf(slot)} .card-${role}`));
const named = (view, name) =>
    view.objects
        .filter((o) => o.zone === "battlefield" && o.name === name)
        .map((o) => o.slot);

// The attack declaration (`combat-declare`): Elite Spellbinder, Malcolm and two Insect tokens
// attack, of six creatures that may, the engine offering one of the twin tokens at a time.
const attackViews = await load("combat-declare");
const attackRecord = recordedDeclaration(attackViews, ["attacker"]);
const attackView = attackRecord.view;
const declaredAttackers = attackRecord.declared.objects
    .filter((o) => o.controller === "you" && o.combat?.kind === "attacking")
    .map((o) => o.slot);
const insects = named(attackView, "Insect Token");
assert.equal(insects.length, 2, "two Insect tokens");
// The twin the engine offers lies on top of their pile.
const [insectTop] = insects.sort((a, b) => a - b);
/** Clicks each creature of `slots` in turn to attack: a click on any card of a pile of twins
 * takes its top card, the lowest slot of those not yet attacking, which each click checks
 * attacks. Returns the attackers taken. */
async function declareByClicks(page, slots) {
    const taken = [];
    for (const slot of slots) {
        const self = attackView.objects.find((o) => o.slot === slot);
        const top = Math.min(
            ...attackView.objects
                .filter((o) => o.slot === slot || twins(self, o))
                .map((o) => o.slot)
                .filter((s) => !taken.includes(s)),
        );
        await clickShown(page, slot);
        await page.clock.runFor(500);
        assert.ok(await shows(page, top, "attacker"), `${top} attacks`);
        taken.push(top);
    }
    return taken;
}
assert.deepEqual(
    [...declaredAttackers].sort((a, b) => a - b),
    [
        ...named(attackView, "Elite Spellbinder"),
        ...named(attackView, "Malcolm, Alluring Scoundrel"),
        ...insects,
    ].sort((a, b) => a - b),
    "the fixture declares the four attackers",
);
// Every creature of yours that may attack: the candidates and their twins.
const mayAttack = attackView.objects.filter(
    (o) =>
        o.zone === "battlefield" &&
        o.controller === "you" &&
        o.types.includes("creature") &&
        !o.tapped &&
        !o.summoningSick,
);
assert.ok(
    mayAttack.length > declaredAttackers.length,
    "some creature that may attack stays home",
);
const offeredAttackers = attackView.status.decision.candidates.flatMap((c) =>
    c.option.kind === "attack" ? [c.option.slot] : [],
);
assert.ok(
    insects.some((slot) => !offeredAttackers.includes(slot)),
    "the engine offers one of the twin tokens",
);
const attackRun = {
    fixture: "combat-declare",
    at: attackRecord.at,
    ready: slotOf(insects[1]),
};

await check(
    "every creature that may attack glows, identical tokens included",
    async (page) => {
        await page.clock.runFor(2000);
        for (const o of mayAttack) {
            assert.ok(
                await page.$(`${slotOf(o.slot)} .card-target`),
                `${o.name} (${o.slot}) glows`,
            );
        }
    },
    attackRun,
);

await check(
    "dragging a creature forward joins the attack",
    async (page) => {
        await page.clock.runFor(2000);
        const spellbinder = named(attackView, "Elite Spellbinder")[0];
        // Not far enough: it goes back to its place.
        await drag(page, slotOf(spellbinder), { by: { x: 0, y: -20 } });
        await page.clock.runFor(1000);
        assert.ok(!(await shows(page, spellbinder, "attacker")), "a nudge");
        await drag(page, slotOf(spellbinder), { by: { x: 0, y: -150 } });
        await page.clock.runFor(1000);
        assert.ok(await shows(page, spellbinder, "attacker"), "attacks");
        assert.deepEqual(await chosen(page), []);
    },
    attackRun,
);

// The block declaration (`combat-double-block`): two blockers on each of two attackers.
const blockViews = await load("combat-double-block");
const blockRecord = recordedDeclaration(blockViews, ["blocker", "blockTarget"]);
const blockView = blockRecord.view;
const declaredBlocks = new Map(
    blockRecord.declared.objects
        .filter((o) => o.combat?.kind === "blocking")
        .map((o) => [o.slot, o.combat.attacker]),
);
const doubled = [...declaredBlocks.values()].find(
    (a) => [...declaredBlocks.values()].filter((b) => b === a).length === 2,
);
assert.ok(doubled !== undefined, "the fixture blocks one attacker twice");
assert.equal(
    new Set(declaredBlocks.values()).size,
    2,
    "and another attacker once",
);
const blockRun = {
    fixture: "combat-double-block",
    at: blockRecord.at,
    ready: slotOf(doubled),
};
const nameIn = (view, slot) => view.objects.find((o) => o.slot === slot).name;

await check(
    "blockers paired with attackers in any order are declared together",
    async (page) => {
        await page.clock.runFor(2000);
        assert.doesNotMatch(
            (await page.locator(".table").innerText()) ?? "",
            /click/i,
            "nothing says what to click",
        );
        // From the last blocker to the first: each, then its attacker.
        const pairs = [...declaredBlocks].sort(([a], [b]) => b - a);
        for (const [blocker, attacker] of pairs) {
            await page.click(slotOf(blocker));
            await page.clock.runFor(500);
            assert.ok(
                await page.$(`${slotOf(attacker)} .card-target`),
                "the attackers it may block glow",
            );
            await page.click(slotOf(attacker));
            await page.clock.runFor(500);
            assert.ok(
                await shows(page, blocker, "blocker"),
                `${blocker} blocks`,
            );
        }
        // Changing one's mind: a blocker clicked again waits for an attacker anew.
        const [again, its] = pairs[0];
        await page.click(slotOf(again));
        await page.clock.runFor(500);
        assert.ok(!(await shows(page, again, "blocker")), "taken back");
        await page.click(slotOf(its));
        await page.clock.runFor(500);
        assert.deepEqual(await chosen(page), [], "nothing is sent yet");
        await page.getByText(`${declaredBlocks.size} blocking`).waitFor();
        await page.getByRole("button", { name: "Block", exact: true }).click();
        assert.deepEqual(
            await settled(page, blockRecord.picks.length),
            blockRecord.picks,
            "the engine's picks, each blocker then its attacker, then done",
        );
    },
    blockRun,
);

// While blockers are declared, your orb shows the damage the unblocked attackers will deal,
// falling as each attacker is blocked, and red when it is lethal.
await check(
    "the unblocked attackers' damage shows by your orb as blocks are declared",
    async (page) => {
        await page.clock.runFor(2000);
        const badge = page.locator('[data-life="you"] .life-incoming');
        const power = (slot) =>
            blockView.objects.find((o) => o.slot === slot).pt.power;
        const atYou = blockView.objects.filter(
            (o) =>
                o.combat?.kind === "attacking" &&
                o.combat.planeswalker === null,
        );
        const life = blockView.players.you.life;
        let left = atYou.reduce((n, o) => n + Math.max(0, o.pt.power), 0);
        const says = async (when) => {
            assert.equal(
                await badge.getAttribute("data-incoming"),
                String(left),
                when,
            );
            assert.equal(
                await badge.getAttribute("data-lethal"),
                left > 0 && left >= life ? "true" : null,
                `${when}: lethal only at ${life}`,
            );
        };
        assert.ok(left > 0, "the attackers deal damage");
        await says("before any block");
        assert.equal(
            await page.locator('[data-life="opponent"] .life-incoming').count(),
            0,
            "the opponent is not attacked",
        );
        const blocked = new Set();
        for (const [blocker, attacker] of declaredBlocks) {
            await page.click(slotOf(blocker));
            await page.clock.runFor(500);
            await page.click(slotOf(attacker));
            await page.clock.runFor(500);
            if (
                !blocked.has(attacker) &&
                atYou.some((o) => o.slot === attacker)
            ) {
                left -= Math.max(0, power(attacker));
            }
            blocked.add(attacker);
            await says(`${blocker} blocking ${attacker}`);
        }
    },
    blockRun,
);

await check(
    "a blocker blocks only an attacker it may, and can be dragged onto it",
    async (page) => {
        await page.clock.runFor(2000);
        // A blocker that may block the doubled attacker but not another, as a creature without
        // flying or reach may not block a flyer (CR 702.9b): by the engine's preview, which
        // names one of identical attackers, so one named like none it may block.
        const nameOf = (slot) => nameIn(blockView, slot);
        const attacking = blockView.objects
            .filter((o) => o.combat?.kind === "attacking")
            .map((o) => o.slot);
        const [blocker, unblockable] = blockView.status.decision.candidates
            .filter((c) => c.option.kind === "block")
            .map((c) => {
                const names = new Set(c.blocks.map(nameOf));
                return [
                    c.option.slot,
                    c.blocks.includes(doubled)
                        ? attacking.find((a) => !names.has(nameOf(a)))
                        : undefined,
                ];
            })
            .find(([, a]) => a !== undefined);
        assert.ok(blocker !== undefined, "a blocker that may not block all");
        await page.click(slotOf(blocker));
        await page.clock.runFor(500);
        assert.ok(!(await page.$(`${slotOf(unblockable)} .card-target`)));
        await page.click(slotOf(unblockable));
        await page.clock.runFor(500);
        assert.ok(!(await shows(page, blocker, "blocker")), "no block");
        const before = (await cues(page)).length;
        await drag(page, slotOf(blocker), { to: slotOf(doubled) });
        await page.clock.runFor(1000);
        assert.ok(
            await shows(page, blocker, "blocker"),
            "dragged on, it blocks",
        );
        const dragged = (await cues(page)).slice(before);
        assert.ok(
            dragged.includes("select") && !dragged.includes("no"),
            `a block dragged on is heard, not refused: ${dragged}`,
        );
        assert.deepEqual(await chosen(page), []);
    },
    blockRun,
);

/** The combat lines in the arrow layer `layer` selects: each one's ends, as the ids its spec
 * names, and its path's first and last points, in page pixels. */
const combatLines = (page, layer) =>
    page.$$eval(`${layer} .arrow`, (gs) =>
        gs.map((g) => {
            const path = g.querySelector(".arrow-line path");
            const ctm = path.getScreenCTM();
            const at = (length) => {
                const p = path.getPointAtLength(length);
                const q = new DOMPoint(p.x, p.y).matrixTransform(ctm);
                return [q.x, q.y];
            };
            return {
                from: g.dataset.from,
                to: g.dataset.to,
                start: at(0),
                end: at(path.getTotalLength()),
            };
        }),
    );

/** Asserts that none of `attackers` (slots), each attacking a player, draws a line: the chevron
 * and the damage badge on the orb say an attack on a player, without a fan of arrows into it. */
async function assertNoLineAtPlayer(page, attackers) {
    const lines = await combatLines(page, ".board-arrows");
    for (const slot of attackers) {
        assert.ok(
            !lines.some((l) => l.from === `card-${slot}`),
            `attacker ${slot} draws no line`,
        );
    }
}

await check(
    "the human's attack on the opponent draws no line",
    async (page) => {
        await page.clock.runFor(2000);
        // The twins' pile, then the rest.
        const declaring = await declareByClicks(page, [
            insectTop,
            ...declaredAttackers.filter((s) => s !== insectTop),
        ]);
        await page.clock.runFor(2000);
        await assertNoLineAtPlayer(page, declaring);
    },
    attackRun,
);

// Blockers declared: each blocker stands across from its attacker, and the rest of its row
// makes room rather than lying under it.
const blockersViews = await load("combat-blockers");
const blockersView = blockersViews.at(-1);
const defender = blockersView.objects.find(
    (o) => o.combat?.kind === "blocking",
)?.controller;
assert.ok(defender, "the blockers fixture has a blocker");
// The defending side's nonland permanents.
const standing = blockersView.objects.filter(
    (o) =>
        o.zone === "battlefield" &&
        o.controller === defender &&
        !o.types.includes("land"),
);

await check(
    "a blocker stands across from its attacker, covering no other card",
    async (page) => {
        await page.clock.runFor(2000);
        const box = async (slot) =>
            page.locator(`[data-slot="${slot}"]`).boundingBox();
        for (const o of standing) {
            if (o.combat?.kind !== "blocking") continue;
            const mine = await box(o.slot);
            const theirs = center(await box(o.combat.attacker));
            const blockers = standing.filter(
                (b) => b.combat?.attacker === o.combat.attacker,
            ).length;
            assert.ok(
                Math.abs(center(mine).x - theirs.x) < 120 * blockers,
                `${o.name} stands across from what it blocks`,
            );
            for (const other of standing) {
                if (other.slot === o.slot) continue;
                const them = await box(other.slot);
                const overlap =
                    Math.min(mine.x + mine.width, them.x + them.width) -
                    Math.max(mine.x, them.x);
                assert.ok(
                    overlap < them.width / 2,
                    `${o.name} covers ${other.name} by ${Math.round(overlap)} px`,
                );
            }
        }
    },
    {
        fixture: "combat-blockers",
        at: blockersViews.length,
        ready: `[data-slot="${standing[0].slot}"]`,
    },
);

// A crowded row (five permanents a side, attackers raised over their neighbors) spreads into
// the table's clear right rather than laying its cards over one another's numbers.
await check(
    "every creature on a crowded table shows its power and toughness",
    async (page) => {
        await page.clock.runFor(2000);
        const hidden = await page.evaluate(() =>
            [...document.querySelectorAll('[data-zone="battlefield"]')]
                .filter((el) => !el.querySelector(".card-under"))
                .flatMap((el) => {
                    const pt = el.querySelector(".card-pt");
                    if (!pt) return [];
                    const r = pt.getBoundingClientRect();
                    const top = document
                        .elementFromPoint(r.x + r.width / 2, r.y + r.height / 2)
                        ?.closest("[data-slot]");
                    return top === el ? [] : [el.dataset.card];
                }),
        );
        assert.deepEqual(hidden, [], "power and toughness covered");
        await assertClearOfLands(page);
    },
    {
        fixture: "combat-blockers",
        at: blockersViews.length,
        ready: `[data-slot="${standing[0].slot}"]`,
    },
);

// A face-down permanent a side, the human's tapped: each in a permanent's frame (the field's
// short one, not a card back's full height), showing its 2/2, over no land.
const faceDown = blockersView.objects.filter(
    (o) => o.zone === "battlefield" && o.facing === "down",
);
for (const side of ["you", "opponent"]) {
    assert.ok(
        faceDown.some((o) => o.controller === side),
        `the blockers fixture has a face-down permanent for ${side}`,
    );
}
assert.ok(
    faceDown.some((o) => o.tapped),
    "the blockers fixture has a tapped face-down permanent",
);
// The face-up permanents: each row's cards share a size, and a tapped one lies on its side.
const faceUp = blockersView.objects.filter(
    (o) =>
        o.zone === "battlefield" &&
        o.facing === "up" &&
        !o.types.includes("land"),
);
await check(
    "a face-down permanent stands in a permanent's frame, a 2/2",
    async (page) => {
        await page.clock.runFor(2000);
        const el = (o) => page.locator(`.card-el[data-slot="${o.slot}"]`);
        // Untransformed sizes, which a tapped card's turn leaves alone.
        const size = (o) =>
            el(o).evaluate((e) => [e.offsetWidth, e.offsetHeight]);
        for (const o of faceDown) {
            const row = faceUp.filter((u) => u.controller === o.controller);
            const like = row.find((u) => u.tapped === o.tapped) ?? row[0];
            assert.ok(like, `a face-up permanent shares ${o.slot}'s row`);
            const [width, height] = await size(like);
            assert.match(
                await el(o).getAttribute("class"),
                /card-el-field/,
                `${o.slot} is drawn as a permanent`,
            );
            const [w, h] = await size(o);
            // Turned a quarter from `like`, its footprint is `like`'s turned.
            const [ww, hh] =
                like.tapped === o.tapped ? [width, height] : [height, width];
            assert.ok(
                Math.abs(w - ww) <= 1 && Math.abs(h - hh) <= 1,
                `${o.slot} is ${w} by ${h}, ${like.name} ${width} by ${height}`,
            );
            assert.equal(
                await el(o).locator(".card-pt").textContent(),
                "2/2",
                `${o.slot} shows its power and toughness`,
            );
        }
        await assertClearOfLands(page);
    },
    {
        fixture: "combat-blockers",
        at: blockersViews.length,
        ready: `[data-slot="${faceDown[0].slot}"]`,
    },
);

// A table with tapped and untapped lands and creatures, and a card Portable Hole exiled.
const beneathViews = await load("exiled-beneath");
const beneathView = beneathViews.at(-1);
const onField = beneathView.objects.filter((o) => o.zone === "battlefield");
const exiled = beneathView.objects.find(
    (o) => o.zone === "exile" && o.exiledBy !== null,
);
assert.ok(exiled, "the exiled-beneath fixture has a card exiled beneath");
for (const [kind, tapped] of [
    ["land", true],
    ["land", false],
    ["creature", true],
]) {
    assert.ok(
        onField.some((o) => o.types.includes(kind) && o.tapped === tapped),
        `the exiled-beneath fixture has ${tapped ? "a tapped" : "an untapped"} ${kind}`,
    );
}
const beneathRun = {
    fixture: "exiled-beneath",
    at: beneathViews.length,
    ready: `[data-slot="${exiled.slot}"]`,
};

await check(
    "a tapped card turns whole, keeping its frame's shape",
    async (page) => {
        await page.clock.runFor(2000);
        for (const o of onField) {
            const { frame, turn, slot } = await page
                .locator(`[data-slot="${o.slot}"] .card`)
                .evaluate((card) => {
                    // Unrounded, untransformed sizes.
                    const aspect = (e) => {
                        const style = getComputedStyle(e);
                        return (
                            parseFloat(style.width) / parseFloat(style.height)
                        );
                    };
                    const m = new DOMMatrix(getComputedStyle(card).transform);
                    return {
                        frame: aspect(card),
                        turn: (Math.atan2(m.b, m.a) * 180) / Math.PI,
                        slot: aspect(card.parentElement),
                    };
                });
            const what = `${o.name} (${o.tapped ? "tapped" : "untapped"})`;
            assert.ok(
                Math.abs(frame - CARD_ASPECT) < 0.01,
                `${what}'s frame is ${frame} wide for its height`,
            );
            assert.ok(
                Math.abs(turn - (o.tapped ? 90 : 0)) < 0.5,
                `${what} is turned ${turn}°`,
            );
            assert.ok(
                Math.abs(slot - (o.tapped ? 1 / CARD_ASPECT : CARD_ASPECT)) <
                    0.01,
                `${what} takes a footprint ${slot} wide for its height`,
            );
        }
        await assertClearOfLands(page);
    },
    beneathRun,
);

await check(
    "a card exiled by a permanent peeks out from beneath it, previewed on hover",
    async (page) => {
        await page.clock.runFor(2000);
        const under = page.locator(`[data-slot="${exiled.slot}"]`);
        const host = page.locator(`[data-slot="${exiled.exiledBy}"]`);
        const [mine, theirs] = [
            await under.boundingBox(),
            await host.boundingBox(),
        ];
        assert.ok(
            mine.x < theirs.x && mine.y > theirs.y,
            `${exiled.name} peeks out down and to the left of what exiled it`,
        );
        const z = (l) => l.evaluate((e) => Number(getComputedStyle(e).zIndex));
        assert.ok((await z(under)) < (await z(host)), "it lies beneath");
        // Its peeking strip, left of what exiled it.
        const at = { x: (mine.x + theirs.x) / 2, y: mine.y + mine.height / 2 };
        assert.equal(
            await page.evaluate(
                ({ x, y }) =>
                    document.elementFromPoint(x, y)?.closest("[data-slot]")
                        ?.dataset.slot,
                at,
            ),
            String(exiled.slot),
            "its strip shows",
        );
        await page.mouse.move(at.x, at.y);
        await page.waitForSelector(
            `.inspect[aria-label="${exiled.name}, inspected"]`,
        );
    },
    beneathRun,
);

// The first slot is a valid exile host, just like any other battlefield slot.
{
    const fixture = JSON.parse(
        await readFile(join(fixtures, "exiled-beneath.json"), "utf8"),
    );
    const last = fixture.messages.findLast((m) => m.kind === "event");
    const host = exiled.exiledBy;
    const swap = (slot) => (slot === host ? 0 : slot === 0 ? host : slot);
    last.payload.view = {
        ...beneathView,
        battlefield: beneathView.battlefield.map(swap),
        objects: beneathView.objects.map((o) => ({
            ...o,
            slot: swap(o.slot),
            exiledBy: swap(o.exiledBy),
        })),
    };
    await writeFixture("exiled-beneath-first", fixture);
}

await check(
    "a card exiled by the first slot stays under its host",
    async (page) => {
        await page.clock.runFor(2000);
        const host = await page.locator('[data-slot="0"]').boundingBox();
        const under = await page.locator(beneathRun.ready).boundingBox();
        assert.ok(under.x < host.x && under.x + under.width > host.x);
        const at = { x: (under.x + host.x) / 2, y: under.y + under.height / 2 };
        await page.mouse.click(at.x, at.y);
        await page
            .locator(`.zone-viewer [data-viewer-slot="${exiled.slot}"]`)
            .waitFor();
    },
    { ...beneathRun, fixture: "exiled-beneath-first" },
);

// The human's Swift Reconfiguration on the opponent's Sphinx of False Conclusions (or whatever the
// fixture attaches across the table): an Aura one player controls on a permanent the other does.
const attachedView = (await load("attached-across")).at(-1);
const attachedBySlot = new Map(attachedView.objects.map((o) => [o.slot, o]));
const aura = attachedView.objects.find(
    (o) =>
        o.zone === "battlefield" &&
        o.attachedTo !== null &&
        attachedBySlot.get(o.attachedTo)?.controller !== o.controller,
);
assert.ok(aura, "the attached-across fixture attaches across the table");
const auraHost = attachedBySlot.get(aura.attachedTo);

await check(
    "an attachment is drawn behind its host, linked on hover",
    async (page) => {
        await page.clock.runFor(2000);
        const mine = page.locator(`[data-slot="${aura.slot}"]`);
        const host = page.locator(`[data-slot="${auraHost.slot}"]`);
        const [a, h] = [await mine.boundingBox(), await host.boundingBox()];
        // Their boxes overlap or touch, in the host's row rather than its controller's.
        assert.ok(
            a.x <= h.x + h.width &&
                h.x <= a.x + a.width &&
                a.y <= h.y + h.height &&
                h.y <= a.y + a.height,
            `${aura.name} touches ${auraHost.name}`,
        );
        assert.ok(
            Math.abs(center(a).y - center(h).y) < h.height / 2,
            `${aura.name} lies in ${auraHost.name}'s row`,
        );
        const z = (l) => l.evaluate((e) => Number(getComputedStyle(e).zIndex));
        assert.ok((await z(mine)) < (await z(host)), "it lies behind");
        // Its name bar, above the host's top edge.
        const strip = { x: a.x + a.width * 0.4, y: (a.y + h.y) / 2 };
        assert.equal(
            await page.evaluate(
                ({ x, y }) =>
                    document.elementFromPoint(x, y)?.closest("[data-slot]")
                        ?.dataset.slot,
                strip,
            ),
            String(aura.slot),
            "its name bar shows above its host",
        );
        // Hovering either rings the other.
        await page.mouse.move(strip.x, strip.y);
        await host.locator(".card-linked").waitFor();
        await page.mouse.move(center(h).x, h.y + h.height * 0.75);
        await mine.locator(".card-linked").waitFor();
        // The host's inspector shows its types now and what is attached to it.
        const inspect = page.locator(
            `.inspect[aria-label="${auraHost.name}, inspected"]`,
        );
        await inspect.locator(`[data-attached="${aura.slot}"]`).waitFor();
        const types = (await inspect.locator(".inspect-types").innerText())
            .toLowerCase()
            .split(/\s+/);
        for (const t of auraHost.types) {
            assert.ok(
                types.includes(t),
                `the inspector shows ${t} in ${types}`,
            );
        }
    },
    {
        fixture: "attached-across",
        at: (await load("attached-across")).length,
        ready: `[data-slot="${aura.slot}"]`,
    },
);

// A permanent with more than one ability (a planeswalker's loyalty abilities) opens the ability
// chooser, each a button, beside those it cannot use now (Teferi's −8), disabled.
const uses = new Map();
attachedView.status.decision.candidates.forEach((c, index) => {
    if (c.option.kind !== "activate") return;
    uses.set(c.option.slot, [...(uses.get(c.option.slot) ?? []), index]);
});
const [walker, abilities] = [...uses].find(([, a]) => a.length > 1) ?? [];
assert.ok(
    walker !== undefined,
    "the attached-across fixture offers two abilities",
);
await check(
    "a permanent's abilities are chosen among as buttons",
    async (page) => {
        await page.clock.runFor(2000);
        await page.click(`[data-slot="${walker}"]`);
        const menu = page.locator(".ability-chooser");
        const buttons = menu.locator("button:enabled");
        assert.equal(await buttons.count(), abilities.length, "one per use");
        assert.equal(
            await menu.locator("button:disabled").count(),
            1,
            "the −8, disabled",
        );
        const label =
            attachedView.status.decision.candidates[abilities[1]].label;
        // The frames spring out of the card into their places.
        await page.clock.runFor(1000);
        await menu
            .getByRole("button", {
                name: label.slice(label.lastIndexOf(": ") + 2),
            })
            .click();
        assert.deepEqual(
            await chosen(page),
            [abilities[1]],
            `chose "${label}"`,
        );
    },
    {
        fixture: "attached-across",
        at: (await load("attached-across")).length,
        ready: `[data-slot="${walker}"]`,
    },
);

/** Drags a hand card by `by`: hovered first, so it rises clear of its neighbors in the fan,
 * then picked up there. */
async function dragFromHand(page, selector, by) {
    const box = await page.locator(selector).boundingBox();
    await page.mouse.move(box.x + box.width / 2, box.y + 40);
    await page.clock.runFor(1000);
    const raised = await page.locator(selector).boundingBox();
    const grab = { x: raised.x + raised.width / 2, y: raised.y + 40 };
    await page.mouse.move(grab.x, grab.y);
    await page.mouse.down();
    for (let i = 1; i <= 10; i++) {
        await page.mouse.move(
            grab.x + (by.x * i) / 10,
            grab.y + (by.y * i) / 10,
        );
    }
}

/** The slots of the permanents lit as the payment's, ascending. */
const lit = (page) =>
    page.$$eval(".card-paying", (els) =>
        els.map((e) => Number(e.closest("[data-slot]").dataset.slot)).sort(),
    );
const ascending = (slots) => [...slots].sort();

/** Your permanents tapped in `after` and not in `before`, ascending. */
function newlyTapped(before, after) {
    const was = new Set(
        before.objects.filter((o) => o.tapped).map((o) => o.slot),
    );
    return ascending(
        after.objects
            .filter(
                (o) =>
                    o.zone === "battlefield" &&
                    o.controller === "you" &&
                    o.tapped &&
                    !was.has(o.slot),
            )
            .map((o) => o.slot),
    );
}

// Two cards alike in hand are one candidate to the engine (architecture §7.2): both glow, and
// either plays.
const pairViews = await load("hand-pair");
const pairView = pairViews.at(-1);
const nameAt = (v, slot) => v.objects.find((o) => o.slot === slot)?.name;
const hand = pairView.objects.filter(
    (o) => o.zone === "hand" && o.owner === "you",
);
const playable = new Map(
    pairView.status.decision.candidates.flatMap((c, i) =>
        ["cast", "playLand", "activate"].includes(c.option.kind) &&
        hand.some((o) => o.slot === c.option.slot)
            ? [[nameAt(pairView, c.option.slot), { index: i, ...c }]]
            : [],
    ),
);
const twin = hand.find(
    (o) =>
        playable.get(o.name)?.option.kind === "playLand" &&
        playable.get(o.name).option.slot !== o.slot,
);
assert.ok(twin, "the pair fixture holds a land twice");

await check(
    "every hand card with something to play glows, twins included",
    async (page) => {
        await page.clock.runFor(2000);
        for (const o of hand) {
            const glows = await page.$(
                `[data-slot="${o.slot}"] .card-playable`,
            );
            assert.equal(
                glows !== null,
                playable.has(o.name),
                `${o.name} in slot ${o.slot} glows as playable`,
            );
        }
        // The twin the engine did not name plays the land too.
        await dragFromHand(page, `[data-slot="${twin.slot}"]`, {
            x: 0,
            y: -320,
        });
        await page.mouse.up();
        assert.deepEqual(await settled(page, 1), [
            playable.get(twin.name).index,
        ]);
    },
    {
        fixture: "hand-pair",
        at: pairViews.length,
        ready: `[data-slot="${twin.slot}"][data-zone="hand"]`,
    },
);

// A permanent that does something when clicked but doesn't drag (a land, which taps for mana)
// does nothing when pulled aside: a drag never acts as a click.
const marksViews = await load("marks");
const marksView = marksViews.at(-1);
const manaLand = marksView.status.decision.candidates.find(
    (c) =>
        c.option.kind === "mana" &&
        marksView.objects.find((o) => o.slot === c.option.slot)?.zone ===
            "battlefield",
);
assert.ok(manaLand, "the marks fixture offers a land's mana");
await check(
    "a land pulled aside does not tap; clicked, it does",
    async (page) => {
        await page.clock.runFor(2000);
        const land = page.locator(`[data-slot="${manaLand.option.slot}"]`);
        const box = await land.boundingBox();
        const at = { x: box.x + box.width / 2, y: box.y + box.height / 2 };
        await page.mouse.move(at.x, at.y);
        await page.mouse.down();
        for (let i = 1; i <= 10; i++) {
            await page.mouse.move(at.x + 4 * i, at.y - 12 * i);
        }
        await page.mouse.up();
        await page.clock.runFor(500);
        assert.deepEqual(await chosen(page), [], "the drag chose nothing");
        await page.mouse.click(at.x, at.y);
        assert.equal((await settled(page, 1)).length, 1, "the click chose");
    },
    {
        fixture: "marks",
        at: marksViews.length,
        ready: `[data-slot="${manaLand.option.slot}"]`,
    },
);

const landActs = pairView.status.decision.candidates.filter(
    (c) => c.option.slot === playable.get(twin.name).option.slot,
);
assert.equal(landActs.length, 1, "the land's one use is to be played");
await check(
    "one click plays a land with one use",
    async (page) => {
        await page.clock.runFor(2000);
        await page.click(`[data-slot="${twin.slot}"]`);
        assert.deepEqual(await settled(page, 1), [
            playable.get(twin.name).index,
        ]);
        assert.equal(await page.locator(".action-menu").count(), 0, "no menu");
    },
    {
        fixture: "hand-pair",
        at: pairViews.length,
        ready: `[data-slot="${twin.slot}"][data-zone="hand"]`,
    },
);

// The X decision of a kicked Thieving Skydiver (`cast-kicker`'s last), asked over the table:
// each value of X says the total cost it makes, in symbols.
const kickViews = await load("cast-kicker");
const kickX = kickViews.at(-1).status.decision;
assert.equal(kickX.kind, "x", "the fixture ends on X");
await check(
    "each X says the total cost it makes",
    async (page) => {
        await page.clock.runFor(2000);
        // Each label with its symbols as braces.
        const said = await page.$$eval(".prompt button", (bs) =>
            bs
                .filter((b) => b.textContent !== "Cancel")
                .map((b) => {
                    const copy = b.cloneNode(true);
                    for (const img of copy.querySelectorAll("[role=img]")) {
                        img.replaceWith(img.getAttribute("aria-label"));
                    }
                    return copy.textContent.trim();
                }),
        );
        assert.deepEqual(
            said,
            kickX.candidates.map((c) => `${c.label} · ${c.payments[0].cost}`),
        );
        assert.deepEqual(
            kickX.candidates.map((c) => c.payments[0].cost),
            ["{2}{U}", "{3}{U}", "{4}{U}"],
            "{1}{U} kicked with X",
        );
    },
    { fixture: "cast-kicker", at: kickViews.length, ready: ".prompt" },
);

// Subtlety, cast for its mana or with evoke, whose exile from the hand is asked as a cost while
// the cast can still be taken back.
const evokeViews = await load("cast-evoke");
const evokeCasts = evokeViews.at(-2).status.decision.candidates;
const evoked = evokeCasts.findIndex(
    (c) => c.option.kind === "cast" && c.option.costs.length > 0,
);
assert.ok(evoked >= 0, "the evoke fixture offers the evoked cast");
const plainCast = evokeCasts.findIndex(
    (c) => c.option.kind === "cast" && c.option.costs.length === 0,
);
const exiling = evokeViews.at(-1).status.decision;
assert.equal(exiling.prompt.purpose, "exile-cost");
assert.ok(exiling.cancellable, "the exile is asked while the cast is unpaid");
const subtlety = `[data-slot="${evokeCasts[evoked].option.slot}"]`;
const evokeFixture = {
    fixture: "cast-evoke",
    at: evokeViews.length - 1,
    ready: `${subtlety}[data-zone="hand"]`,
};

/** The enabled primary buttons on screen. */
const primaries = (page) =>
    page.$$eval(
        ".button-primary",
        (els) =>
            els.filter((b) => !b.disabled && b.getClientRects().length > 0)
                .length,
    );

/** The mode chooser's frames. */
const frames = (page) => page.locator(".ability-chooser .ability-choice");

// An opponent's library choice is public, even when both answers reach the same position.
for (const [libraryTop, alone] of [
    [true, false],
    [false, false],
    [false, true],
]) {
    const name = `spell-library-${libraryTop ? "top" : "bottom"}${alone ? "-empty" : ""}`;
    const fixture = JSON.parse(
        await readFile(join(fixtures, "cast-evoke.json"), "utf8"),
    );
    const event = fixture.messages.findLast((m) => m.kind === "event");
    const view = evokeViews.at(-2);
    const slot = evokeCasts[evoked].option.slot;
    const opponent = view.players.opponent;
    const library = alone
        ? [slot]
        : libraryTop
          ? [...opponent.library, slot]
          : [slot, ...opponent.library];
    event.payload.view = {
        ...view,
        status: {
            ...view.status,
            decision: {
                ...view.status.decision,
                candidates: view.status.decision.candidates.filter(
                    (c) => c.option.slot !== slot,
                ),
            },
        },
        players: { ...view.players, opponent: { ...opponent, library } },
        objects: view.objects.map((o) =>
            o.slot === slot
                ? {
                      ...o,
                      zone: "library",
                      owner: "opponent",
                      controller: "opponent",
                  }
                : o,
        ),
        events: [
            {
                kind: "zoneChange",
                slot,
                from: "stack",
                to: "library",
                reason: "effect",
                libraryTop,
            },
        ],
    };
    await writeFixture(name, fixture);
    await check(
        `the opponent picks the ${libraryTop ? "top" : "bottom"} of ${alone ? "an empty" : "their"} library`,
        async (page) => {
            const choice = page.getByRole("button", {
                name: libraryTop ? "Top of Library" : "Bottom of Library",
            });
            await choice.waitFor();
            await page.clock.runFor(2000);
            const moved = page.locator(`${subtlety}[data-zone="library"]`);
            assert.equal(
                await moved.locator(".card").count(),
                libraryTop || alone ? 1 : 0,
                "only the known top card shows its face",
            );
            assert.equal(
                await moved.getAttribute("data-card"),
                libraryTop || alone ? "Subtlety" : null,
                "a buried face does not label the card back",
            );
            await choice.click();
            await page
                .locator(".game-log")
                .getByText(/Opponent put Subtlety/)
                .waitFor();
        },
        { fixture: name, at: evokeViews.length, ready: ".step-stops-opponent" },
    );
}

/** A caption's text and every mana symbol share a visual center line. */
async function assertCaptionAligned(caption) {
    const offsets = await caption.evaluate((el) => {
        const range = document.createRange();
        range.selectNodeContents(el.firstChild);
        const text = range.getBoundingClientRect();
        return [...el.querySelectorAll(".mana-symbol")].map((symbol) => {
            const mana = symbol.getBoundingClientRect();
            return (mana.top + mana.bottom - text.top - text.bottom) / 2;
        });
    });
    assert.ok(
        offsets.length && offsets.every((dy) => Math.abs(dy) < 2),
        `caption's mana is centered on its text: ${offsets}`,
    );
}

await check(
    "Enter confirms the regular cast from the mode chooser",
    async (page) => {
        await page.clock.runFor(2000);
        await page.click(subtlety);
        await frames(page).first().waitFor();
        await page.keyboard.press("Enter");
        assert.deepEqual(await settled(page, 1), [plainCast]);
    },
    evokeFixture,
);

const timingViews = await load("teferi-timing");
const timingView = timingViews[0];
const timingCard = (name) =>
    `[data-slot="${timingView.objects.find((o) => o.name === name).slot}"]`;

await check(
    "Teferi's timing refusal appears briefly beside its source",
    async (page) => {
        await page.clock.runFor(2000);
        const source = page.locator(timingCard("Teferi, Time Raveler"));
        const notice = page.locator(".timing-refusal");
        await page.click(timingCard("Thundertrap Trainer"));
        assert.equal(await notice.count(), 0, "ordinary timing has no source");
        await page.click(timingCard("Thirst for Discovery"));
        await notice.waitFor();
        await page.clock.runFor(500);
        assert.equal(await notice.textContent(), "Sorcery speed only");
        const at = await notice.boundingBox();
        const teferi = await source.boundingBox();
        assert.ok(
            Math.abs(center(at).x - center(teferi).x) < 3,
            "the notice is centered on the opposing source",
        );
        const gap = at.y - (teferi.y + teferi.height);
        assert.ok(
            gap >= -1 && gap < 12,
            `the notice is just below the source: ${gap}`,
        );
        assert.ok(
            Number(await notice.evaluate((e) => getComputedStyle(e).opacity)) >
                0.9,
        );
        await page.mouse.move(800, 380);
        assert.equal(await notice.count(), 1, "feedback outlasts hover");
        await page.clock.runFor(2000);
        await notice.waitFor({ state: "detached" });
        assert.deepEqual(
            await chosen(page),
            [],
            "the rejected click sends no action",
        );
    },
    {
        fixture: "teferi-timing",
        at: 1,
        ready: timingCard("Thirst for Discovery"),
        reducedMotion: "reduce",
    },
);

const mainWithTeferi = timingViews.findIndex((view) =>
    view.status.decision?.candidates.some(
        (c) => c.label === "Cast Thirst for Discovery",
    ),
);
assert.ok(mainWithTeferi > 0, "the instant is legal in the main phase");
await check(
    "Teferi's timing allows the instant in the main phase",
    async (page) => {
        await page.clock.runFor(2000);
        await page.click(timingCard("Thirst for Discovery"));
        const index = timingViews[
            mainWithTeferi
        ].status.decision.candidates.findIndex(
            (c) => c.label === "Cast Thirst for Discovery",
        );
        assert.deepEqual(await settled(page, 1), [index]);
        assert.equal(await page.locator(".timing-refusal").count(), 0);
    },
    {
        fixture: "teferi-timing",
        at: mainWithTeferi + 1,
        ready: timingCard("Thirst for Discovery"),
    },
);

// Each cast's frame is the card, its caption the cast's cost in symbols or its keyword.
await check(
    "a cast's frame says its cost",
    async (page) => {
        await page.clock.runFor(2000);
        await page.click(subtlety);
        await frames(page).first().waitFor();
        await page.clock.runFor(800);
        const captions = frames(page).locator(".ability-choice-caption");
        const [evokeCaption, castCaption] = await captions.all();
        assert.ok(
            (await castCaption.boundingBox()).x >
                (await evokeCaption.boundingBox()).x,
            "the regular cast is on the right",
        );
        await assertCaptionAligned(castCaption);
        assert.deepEqual(
            await castCaption
                .locator("[role=img]")
                .evaluateAll((es) =>
                    es.map((e) => e.getAttribute("aria-label")),
                ),
            ["{2}", "{U}", "{U}"],
            "the card's mana cost",
        );
        assert.equal((await evokeCaption.textContent()).trim(), "Evoke");
        assert.equal(
            await frames(page).locator(".card-name").first().textContent(),
            "Subtlety",
            "the card itself",
        );
    },
    evokeFixture,
);

/** A fixture stopped at a payment plan and the view after: the cast before it, the plan the
 * stand-in chose and the permanents that tapped for it. */
async function payment(name) {
    const views = await load(name);
    const at = views.findIndex(
        (v) => v.status.decision?.kind === "paymentPlan",
    );
    const [plans, after] = [views[at], views[at + 1]];
    let castAt = at - 1;
    while (views[castAt].status.decision?.kind !== "priority") castAt--;
    const before = views[castAt];
    const spell = plans.stack.at(-1);
    const castIndex = before.status.decision.candidates.findIndex(
        (c) => c.option.kind === "cast" && c.option.slot === spell.slot,
    );
    const tapped = newlyTapped(plans, after);
    const chosenPlan = plans.status.decision.candidates.findIndex(
        (c) =>
            JSON.stringify(ascending(c.payments[0].taps)) ===
            JSON.stringify(tapped),
    );
    assert.ok(chosenPlan >= 0, `${name}: the tapped lands are a plan's`);
    return {
        views,
        castAt,
        spell: `[data-slot="${spell.slot}"]`,
        cast: before.status.decision.candidates[castIndex],
        castIndex,
        between: views
            .slice(castAt + 1, at)
            .filter((v) => v.status.kind === "deciding"),
        chosenPlan,
        tapped,
    };
}

const byDefault = await payment("pay-default");
assert.equal(byDefault.chosenPlan, 0, "the stand-in paid the default way");
assert.equal(byDefault.between.length, 0, "nothing asked before payment");
// A card the engine offers two payments for is cast in one gesture, a click as a drop, paying
// the way the session ranks first (Arena's auto-tapper), its lands lit while it is on the
// stack: here until the payment's view, which the page then waits past.
await derive("pay-default", "pay-default-paying", 1, (v) => v);
for (const gesture of ["drag", "click"]) {
    await check(
        `a ${gesture} casts a card with two payments, paying the first way`,
        async (page) => {
            await page.clock.runFor(2000);
            if (gesture === "drag") {
                await dragFromHand(page, byDefault.spell, { x: 0, y: -320 });
                assert.deepEqual(
                    await lit(page),
                    byDefault.tapped,
                    "lit while dragged: the lands that tap",
                );
                await page.mouse.up();
            } else {
                await page.click(byDefault.spell);
            }
            assert.deepEqual(
                await settled(page, 2),
                [byDefault.castIndex, 0],
                "cast, then paid the default way",
            );
            assert.equal(
                await page.getByRole("dialog", { name: /^Cast / }).count(),
                0,
                "no cast step",
            );
            await page.clock.runFor(2000);
            assert.deepEqual(
                await lit(page),
                byDefault.tapped,
                "the lands it tapped lit",
            );
            assert.equal(
                await page.getByText(/Payment \d/).count(),
                0,
                "no payment prompt",
            );
        },
        {
            fixture: "pay-default-paying",
            at: byDefault.castAt + 1,
            ready: byDefault.spell,
        },
    );
}

await check(
    "undo returns an automatically paid spell to hand in one step",
    async (page) => {
        await page.clock.runFor(2000);
        await page.click(byDefault.spell);
        await settled(page, 2);
        await until(page, 'role=button[name="Undo"]');
        await page.getByRole("button", { name: "Undo" }).click();
        await page.clock.runFor(1500);
        await page.locator(`${byDefault.spell}[data-zone="hand"]`).waitFor();
        assert.equal(
            await page.getByRole("dialog", { name: /^Pay / }).count(),
            0,
        );
    },
    {
        fixture: "pay-default",
        at: byDefault.castAt + 1,
        ready: byDefault.spell,
    },
);

// A payment plan asked outside the cast step reads as Arena's "Pay {cost}.": one Auto Pay,
// previewing the plan the session ranks first, and permanents picked on the table, which
// settle the plan that taps them. Its plans once were buttons naming their lands, cut at
// every size (by-w8gv.22); no audit can say which plan a pick on the table makes Auto Pay pay.
const otherViews = await load("pay-other");
const otherAt = otherViews.findIndex(
    (v) => v.status.decision?.kind === "paymentPlan",
);
const [firstPlan, otherPlan] = otherViews[otherAt].status.decision.candidates
    .map((c, index) => ({ c, index }))
    .filter(({ c }) => c.option.kind === "plan")
    .map(({ c, index }) => ({ index, taps: c.payments[0].taps }));
const otherOnly = otherPlan.taps.find((s) => !firstPlan.taps.includes(s));
await check(
    "a payment plan is picked on the table and paid by Auto Pay",
    async (page) => {
        await page.clock.runFor(2000);
        const autoPay = page.getByRole("button", {
            name: "Auto Pay",
            exact: true,
        });
        assert.equal(await page.getByText(/^Tap /).count(), 0, "no plan list");
        assert.deepEqual(await lit(page), [], "nothing lit before a preview");
        await autoPay.hover();
        await committed(page);
        assert.deepEqual(await lit(page), ascending(firstPlan.taps));
        await page.mouse.move(0, 0);
        await page.click(slotOf(otherOnly));
        await committed(page);
        assert.deepEqual(await lit(page), [otherOnly], "the pick lit");
        await autoPay.hover();
        await committed(page);
        assert.deepEqual(await lit(page), ascending(otherPlan.taps));
        await autoPay.click();
        assert.deepEqual(await settled(page, 1), [otherPlan.index]);
    },
    { fixture: "pay-other", at: otherAt + 1, ready: slotOf(otherOnly) },
);

const oculusViews = await load("cast-oculus");
const oculusSlot = oculusViews[0].objects.find(
    (o) => o.name === "Abhorrent Oculus" && o.zone === "hand",
).slot;
const oculusCard = `[data-slot="${oculusSlot}"]`;
await check(
    "a cast stays on the stack while its graveyard exile cost plays",
    async (page) => {
        await page.clock.runFor(2000);
        await dragFromHand(page, oculusCard, { x: 0, y: -320 });
        await page.mouse.up();
        await settled(page, 1);
        await page.clock.runFor(3500);
        const stack = await page.evaluate(
            () =>
                window.berylShown.find(
                    ({ live, update }) =>
                        live &&
                        update.view.events.some(
                            (e) =>
                                e.kind === "zoneChange" &&
                                e.from === "graveyard" &&
                                e.to === "exile",
                        ),
                )?.update.view.stack,
        );
        assert.ok(
            stack?.some(
                (item) => item.kind === "spell" && item.slot === oculusSlot,
            ),
            "the cost beat keeps Oculus on the stack",
        );
    },
    { fixture: "cast-oculus", at: 1, ready: oculusCard },
);

// The choice overlays: a scry, Jace's put-back and a pay-or-counter prompt, each the last view
// of its fixture. An overlay sends the first pick of its plan at once; the rest wait for the
// decisions they answer, which these fixtures don't record.
/** A fixture's last decision, and the candidate index of each card it offers, by slot. */
async function lastDecision(name) {
    const views = await load(name);
    const decision = views.at(-1).status.decision;
    const bySlot = new Map(
        decision.candidates.flatMap((c, i) =>
            c.option.kind === "object" ? [[c.option.slot, i]] : [],
        ),
    );
    return { views, decision, bySlot };
}
const tray = (slot) => `.choice-zone [data-slot="${slot}"]`;

const scry = await lastDecision("choice-scry");
const [scryTop, scryNext] = scry.views
    .at(-1)
    .players.you.library.slice(-2)
    .reverse();
await check(
    "clicking a scried card sends it to the bottom",
    async (page) => {
        await page.clock.runFor(2000);
        const before = await page.locator(tray(scryTop)).boundingBox();
        await page.click(tray(scryTop));
        await page.clock.runFor(2000);
        const after = await page.locator(tray(scryTop)).boundingBox();
        const library = await page.locator(".choice-library").boundingBox();
        assert.ok(
            after.x + after.width / 2 < library.x,
            "the card moved left of the library, to the bottom",
        );
        assert.ok(after.x < before.x, "the card moved left");
        await page.getByRole("button", { name: "Done" }).click();
        assert.deepEqual(
            await settled(page, 1),
            [scry.bySlot.get(scryTop)],
            "the first pick puts that card on the bottom",
        );
    },
    { fixture: "choice-scry", at: scry.views.length, ready: tray(scryNext) },
);

// The opponent's piles lie at the top right, where "View Battlefield" was: the way back stays
// clear of them.
await check(
    "a prompt's way back from the battlefield takes every click",
    async (page) => {
        await page.getByRole("button", { name: "View Battlefield" }).click();
        await assertReachable(page, page.locator(".overlay-return"));
    },
    { fixture: "choice-scry", at: scry.views.length, ready: ".choice" },
);

await check(
    "scry shows the table library stack",
    async (page) => {
        await page.clock.runFor(1000);
        assert.equal(await page.locator(".choice-library .pile").count(), 1);
        assert.equal(
            await page.locator(".choice-library .card-back").count(),
            1,
        );
        const library = page.locator('[data-table-library="you"]');
        const layers = page.locator('.card-layer > [data-library-pile="you"]');
        assert.ok(await layers.count(), "the table still owns its slots");
        assert.equal(
            await layers.first().isVisible(),
            false,
            "the packet is hidden while borrowed",
        );
        assert.equal(
            await library.isVisible(),
            false,
            "only the tray shows the library",
        );
        await page.getByRole("button", { name: "View Battlefield" }).click();
        assert.equal(
            await library.isVisible(),
            true,
            "the library returns to the table",
        );
        assert.equal(await layers.first().isVisible(), true);
    },
    { fixture: "choice-scry", at: scry.views.length, ready: tray(scryNext) },
);

// The scry's source, Aang's Iceberg, has four paragraphs of rules: they shrink to fit its box.
await check(
    "long rules text fits its card rather than being cut off",
    async (page) => {
        await page.evaluate(() => document.fonts.ready.then(() => {}));
        await page.clock.runFor(2000);
        const cut = await page.evaluate(() =>
            [...document.querySelectorAll(".card-rules p")]
                .filter((p) => {
                    const box = p.parentElement.getBoundingClientRect();
                    return p.getBoundingClientRect().bottom > box.bottom + 1;
                })
                .map((p) => p.textContent.slice(0, 30)),
        );
        assert.deepEqual(cut, [], "rules text overflowing its box");
    },
    { fixture: "choice-scry", at: scry.views.length, ready: tray(scryNext) },
);

// The last priority in the scry fixture where both players have graveyards and exiled cards,
// and the human holds a hand wide enough to fan out as far as it goes.
const exiledFrom = (v, side) =>
    v.objects.filter(
        (o) =>
            o.owner === side &&
            o.zone === "exile" &&
            !v.battlefield.includes(o.exiledBy),
    );
const pilesAt = scry.views.findLastIndex(
    (v) =>
        v.status.decision?.kind === "priority" &&
        v.objects.filter((o) => o.zone === "hand" && o.owner === "you")
            .length >= 6 &&
        ["you", "opponent"].every(
            (side) =>
                v.players[side].graveyard.length > 0 &&
                exiledFrom(v, side).length > 0,
        ),
);
assert.ok(pilesAt >= 0, "the scry fixture has a view with every pile");
const pilesView = scry.views[pilesAt];
const pile = (side, zone) => `[data-pile="${zone}"][data-side="${side}"]`;
const pileRun = {
    fixture: "choice-scry",
    at: pilesAt + 1,
    ready: pile("you", "graveyard"),
};
await derive("choice-scry", "known-library", pilesAt, (v) => ({
    ...v,
    objects: v.objects.map((o) =>
        o.zone === "library"
            ? {
                  ...o,
                  name:
                      o.slot === v.players[o.owner].library.at(-2)
                          ? "Jace, Vryn's Prodigy"
                          : null,
              }
            : o,
    ),
}));
await check(
    "known library browser never reveals unknown cards",
    async (page) => {
        await page.clock.runFor(2000);
        for (const side of ["you", "opponent"]) {
            await page
                .locator(`button[data-pile="library"][data-side="${side}"]`)
                .click();
            await page.clock.runFor(1000);
            const dialog = page.getByRole("dialog");
            assert.match(await dialog.innerText(), /Known cards/);
            assert.equal(await dialog.locator(".zone-viewer-card").count(), 1);
            assert.match(await dialog.innerText(), /Jace/);
            await dialog
                .getByRole("button", { name: "Close", exact: true })
                .click();
            await page.clock.runFor(1000);
        }
    },
    { ...pileRun, fixture: "known-library", at: 1 },
);

const overlaps = (a, b) =>
    a.x < b.x + b.width &&
    b.x < a.x + a.width &&
    a.y < b.y + b.height &&
    b.y < a.y + a.height;

for (const viewport of [
    { width: 1600, height: 900 },
    { width: 1280, height: 800 },
]) {
    await check(
        `your graveyard and exile lie clear of the hand at ${viewport.width}`,
        async (page) => {
            await page.clock.runFor(2000);
            const hand = await page.locator(".card-el-hand").all();
            assert.ok(hand.length >= 6, "the hand fans out as far as it goes");
            for (const zone of ["graveyard", "exile"]) {
                const box = await page.locator(pile("you", zone)).boundingBox();
                for (const card of hand) {
                    const name = await card.getAttribute("data-card");
                    assert.ok(
                        !overlaps(box, await card.boundingBox()),
                        `${name} covers your ${zone}`,
                    );
                }
            }
        },
        { ...pileRun, viewport },
    );
}

// Six other permanents a side, the opponent's back row reaching under their piles' corner, and
// thick piles on both sides (by-lomm): no layer of any library, graveyard or exile pile lies
// over a permanent, at the reported window and at narrower and wider ones. Cards lean by
// skewing sideways only, so their boxes' heights are their own.
const pilesCrowded = await load("piles-crowded");
for (const viewport of [
    { width: 1728, height: 963 },
    { width: 1600, height: 900 },
    { width: 1280, height: 800 },
    { width: 2134, height: 900 },
]) {
    await check(
        `the piles lie clear of every permanent at ${viewport.width}×${viewport.height}`,
        async (page) => {
            await page.clock.runFor(2000);
            const { piles, field } = await page.evaluate(() => {
                const boxes = (selector) =>
                    [...document.querySelectorAll(selector)]
                        .filter((e) => {
                            const s = getComputedStyle(e);
                            return (
                                s.visibility !== "hidden" &&
                                Number(s.opacity) > 0
                            );
                        })
                        .map((e) => {
                            const r = e.getBoundingClientRect();
                            return {
                                name: `${e.dataset.card ?? "a back"} (${e.dataset.zone})`,
                                x: r.x,
                                y: r.y,
                                right: r.right,
                                bottom: r.bottom,
                            };
                        });
                return {
                    piles: boxes(
                        ".card-layer > .card-el:is([data-zone=library], [data-zone=graveyard], [data-zone=exile])",
                    ),
                    field: boxes(
                        ".card-layer > .card-el[data-zone=battlefield]",
                    ),
                };
            });
            for (const zone of ["library", "graveyard", "exile"]) {
                assert.ok(
                    piles.filter((p) => p.name.endsWith(`(${zone})`)).length >=
                        6,
                    `both ${zone} piles are thick`,
                );
            }
            assert.ok(field.length >= 20, "both sides' permanents are drawn");
            for (const p of piles) {
                for (const b of field) {
                    const across =
                        Math.min(p.right, b.right) - Math.max(p.x, b.x);
                    const down =
                        Math.min(p.bottom, b.bottom) - Math.max(p.y, b.y);
                    assert.ok(
                        across <= 0.5 || down <= 0.5,
                        `${p.name} covers ${b.name} by ${across.toFixed(1)} × ${down.toFixed(1)} px`,
                    );
                }
            }
        },
        {
            fixture: "piles-crowded",
            at: pilesCrowded.length,
            ready: "[data-pile=exile][data-side=opponent]",
            viewport,
        },
    );
}

/** The names of the cards a pile in the piles' view holds. */
function pileNames(side, zone) {
    const slots =
        zone === "graveyard"
            ? pilesView.players[side].graveyard
            : exiledFrom(pilesView, side).map((o) => o.slot);
    return slots.map(
        (slot) => pilesView.objects.find((o) => o.slot === slot).name,
    );
}

await check(
    "a pile opens a viewer listing each of its cards, which closes",
    async (page) => {
        await page.clock.runFor(2000);
        // A pile is not pointed at card by card: hovering it shows no card whole.
        await page.hover(pile("you", "graveyard"));
        await page.clock.runFor(500);
        assert.equal(await page.$(".inspect"), null, "a pile card is hovered");
        const close = [
            (p) => p.keyboard.press("Escape"),
            (p) => p.getByRole("button", { name: "Close" }).click(),
            (p) => p.mouse.click(20, 450),
        ];
        const piles = [
            ["you", "graveyard"],
            ["opponent", "graveyard"],
            ["opponent", "exile"],
        ];
        for (const [i, [side, zone]] of piles.entries()) {
            const names = pileNames(side, zone);
            assert.ok(names.length > 0, `the ${side}'s ${zone} has cards`);
            await page.click(pile(side, zone));
            const viewer = `.zone-viewer${pile(side, zone)}`;
            await page.waitForSelector(viewer);
            const listed = await page.$$eval(
                `${viewer} [data-viewer-slot]`,
                (cards) => cards.map((c) => c.dataset.card),
            );
            assert.deepEqual(
                [...listed].sort(),
                [...names].sort(),
                `the ${side}'s ${zone} lists each of its cards`,
            );
            const slots = await page.$$eval(
                `${viewer} [data-viewer-slot]`,
                (cards) => cards.map((c) => c.dataset.viewerSlot),
            );
            for (const slot of slots) {
                const original = page.locator(
                    `.card-layer > [data-slot="${slot}"]`,
                );
                assert.equal(await original.count(), 1);
                assert.equal(
                    await original.isVisible(),
                    false,
                    "the browser owns the card",
                );
            }
            // Once the flight has arrived, each card shows whole on hover.
            await page.clock.runFor(500);
            await page.hover(`${viewer} [data-card="${listed[0]}"]`);
            await page
                .getByLabel(`${listed[0]}, inspected`)
                .waitFor({ state: "visible" });
            await close[i](page);
            await untilGone(page, viewer, 2000);
            for (const slot of slots) {
                assert.equal(
                    await page
                        .locator(`.card-layer > [data-slot="${slot}"]`)
                        .isVisible(),
                    true,
                    "the table takes the original back",
                );
            }
        }
        assert.deepEqual(await chosen(page), [], "browsing answers nothing");
    },
    pileRun,
);

await check(
    "an empty known library opens without revealing cards",
    async (page) => {
        await page.locator(`button${pile("you", "library")}`).click();
        const dialog = page.getByRole("dialog", { name: /Known cards/ });
        assert.match(await dialog.innerText(), /Nothing here/);
        assert.equal(await dialog.locator("[data-viewer-slot]").count(), 0);
        await dialog.getByRole("button", { name: "Close" }).click();
    },
    { ...pileRun, reducedMotion: "reduce" },
);

// The production recorder must catch deliberate duplicates in viewer and
// prompt markup, even when the borrowed face has no name.
await check(
    "the physical audit rejects copies in dialogs and prompts",
    async (page) => {
        await startRecording(page);
        await page.evaluate(() => {
            const panel = (klass, role) => {
                const e = document.createElement("div");
                e.className = klass;
                e.setAttribute("role", role);
                e.style.cssText = "all: initial; position: absolute; inset: 0";
                return e;
            };
            const board = panel("test-motion-overlay", "group");
            Object.assign(board.style, {
                position: "fixed",
                translate: "none",
                zIndex: 99999,
            });
            document.body.append(board);
            const browser = panel("zone-viewer", "dialog");
            const prompt = panel("prompt", "group");
            const hover = panel("inspect", "group");
            const mode = panel("ability-chooser", "group");
            board.append(browser, prompt, hover, mode);
            let n = 0;
            const card = (parent, attrs, klass = "") => {
                const e = document.createElement("div");
                e.className = klass;
                e.style.cssText = "all: initial";
                Object.assign(e.dataset, attrs);
                Object.assign(e.style, {
                    position: "absolute",
                    left: `${30 + (n % 8) * 125}px`,
                    top: `${60 + Math.floor(n++ / 8) * 150}px`,
                    width: "90px",
                    height: "125px",
                    background: "white",
                });
                parent.append(e);
                return e;
            };
            card(board, { slot: "9901" });
            card(browser, { viewerSlot: "9901" }, "zone-viewer-card");
            card(board, { slot: "9902", card: "Prompt card" });
            card(
                prompt,
                { promptSlot: "9902", card: "Different face" },
                "prompt-card",
            );
            card(board, { slot: "9903", card: "Twin" });
            card(board, { slot: "9904", card: "Twin" });
            card(hover, { slot: "9903", card: "Twin" });
            card(mode, { mode: "activated", stackKey: "1:Twin", card: "Twin" });
            card(board, { slot: "9905", tokenLife: "1" });
            card(board, { ceasingSlot: "9905", tokenLife: "0" });
        });
        await page.clock.runFor(100);
        const recording = await page.evaluate(collect, 0);
        const found = duplicatesIn(recording);
        assert.equal(found.length, 2);
        assert.ok(
            found.some(
                (f) =>
                    f.what.includes("slot 9901") &&
                    f.why.includes("zone-viewer"),
            ),
        );
        assert.ok(
            found.some(
                (f) => f.what.includes("slot 9902") && f.why.includes("prompt"),
            ),
        );
    },
    pileRun,
);

for (const { viewport, reducedMotion } of motionConfigs) {
    const config = `${viewport.width}x${viewport.height} ${reducedMotion}`;
    await check(
        `physical audit follows both graveyard browsers (${config})`,
        async (page) => {
            await page.clock.runFor(800);
            await startRecording(page);
            await page.clock.pauseAt(Date.now() + 60_000);
            const release = new Map();
            for (const side of ["you", "opponent"]) {
                await page.click(pile(side, "graveyard"));
                await page.waitForSelector(".zone-viewer-card");
                // Close one opening in mid-flight, then let the other settle.
                await page.clock.runFor(side === "you" ? 100 : 650);
                const top = pilesView.players[side].graveyard.at(-1);
                const point = await page
                    .locator(`[data-viewer-slot="${top}"]`)
                    .evaluate((e) => {
                        const r = e.getBoundingClientRect();
                        return [r.left + r.width / 2, r.top + r.height / 2];
                    });
                release.set(side, point);
                await page.keyboard.press("Escape");
                await page.clock.runFor(300);
                await page.waitForSelector(".zone-viewer", {
                    state: "detached",
                });
                await page.clock.runFor(650);
            }
            const run = await page.evaluate(collect, 0);
            assert.deepEqual(duplicatesIn(run), []);
            for (const side of ["you", "opponent"]) {
                const slots = pilesView.players[side].graveyard.map(String);
                const resting = (frame) =>
                    frame.instances.filter(
                        (c) => c.zone === "graveyard" && slots.includes(c.slot),
                    );
                const initial = resting(run.frames[0]);
                assert.equal(initial.length, slots.length);
                assert.equal(
                    new Set(initial.map(({ x, y }) => `${x},${y}`)).size,
                    slots.length,
                    "each layer has its own pose",
                );
                assert.deepEqual(
                    resting(run.frames.at(-1)).map(({ id, x, y }) => ({
                        id,
                        x,
                        y,
                    })),
                    initial.map(({ id, x, y }) => ({ id, x, y })),
                    "the same layers settle back into the pile",
                );
                const slot = slots.at(-1);
                const track = posesOf(run, slot);
                const from = track[0];
                const end = track.at(-1);
                const borrower = track.filter(
                    (c) => c.where === "zone-viewer-card",
                );
                assert.ok(
                    borrower.length > 0,
                    `${side}'s original was borrowed`,
                );
                assert.equal(
                    end.id,
                    from.id,
                    "the same table node takes the card back",
                );
                assertNear(
                    [end.x, end.y],
                    [from.x, from.y],
                    "back on its pile",
                );
                if (reducedMotion !== "reduce") {
                    assert.ok(
                        new Set(borrower.map((c) => `${c.x},${c.y}`)).size > 2,
                        "the browser drew intermediate poses",
                    );
                    const last = borrower.at(-1);
                    const returning = track
                        .slice(track.lastIndexOf(last) + 1)
                        .filter((c) => c.top);
                    assert.ok(
                        returning.some(
                            (c) => Math.hypot(c.x - from.x, c.y - from.y) > 4,
                        ),
                        `the ${side}'s card begins its visible return away from the pile`,
                    );
                    assertBetween(
                        returning.map(({ x, y }) => [x, y]),
                        release.get(side),
                        [end.x, end.y],
                        `the ${side}'s card flies back to its pile`,
                    );
                }
            }
        },
        { ...pileRun, viewport, reducedMotion },
    );
    await check(
        `physical audit follows a scry tray's same-zone return (${config})`,
        async (page) => {
            await page.clock.runFor(650);
            await startRecording(page);
            await page.clock.pauseAt(Date.now() + 60_000);
            await page
                .getByRole("button", { name: "View Battlefield" })
                .click();
            await page.clock.runFor(650);
            await page.locator(".overlay-return").click();
            await page.clock.runFor(650);
            const run = await page.evaluate(collect, 0);
            assert.deepEqual(duplicatesIn(run), []);
            const track = posesOf(run, scryTop);
            const table = track.filter((c) => c.zone === "library");
            assert.ok(table.length > 0, "the card returned to its library");
            assert.equal(new Set(table.map((c) => c.id)).size, 1);
            assert.equal(track[0].where, "choice-card");
            assert.equal(track.at(-1).where, "choice-card");
            if (reducedMotion !== "reduce") {
                const points = table.map(({ x, y }) => [x, y]);
                assert.ok(new Set(points.map(String)).size > 2);
                assertBetween(
                    points,
                    [track[0].x, track[0].y],
                    points.at(-1),
                    "the card returns from the tray",
                );
            }
        },
        {
            fixture: "choice-scry",
            at: scry.views.length,
            ready: tray(scryNext),
            viewport,
            reducedMotion,
        },
    );
}

// A full row and a scrolling row both borrow from the same live pile. Use distinct slots
// even where the fixture repeats the same printing.
for (const count of [1, 10, 11]) {
    const fixture = `graveyard-${count}`;
    await derive("choice-scry", fixture, pilesAt, (view) => {
        const you = view.players.you;
        const top = view.objects.find((o) => o.slot === you.graveyard.at(-1));
        const first = Math.max(...view.objects.map((o) => o.slot)) + 1;
        const cards = Array.from({ length: count }, (_, i) => ({
            ...top,
            slot: first + i,
        }));
        return {
            ...view,
            objects: [
                ...view.objects.filter((o) => !you.graveyard.includes(o.slot)),
                ...cards,
            ],
            players: {
                ...view.players,
                you: { ...you, graveyard: cards.map((o) => o.slot) },
            },
        };
    });
    await check(
        `a ${count}-card graveyard opens with the table cards borrowed`,
        async (page) => {
            const button = page.locator(`button${pile("you", "graveyard")}`);
            await page.clock.runFor(1000);
            const layers = await page
                .locator('.card-layer > [data-zone="graveyard"].card-el-you')
                .evaluateAll((cards) =>
                    cards.map((card) => ({
                        slot: Number(card.dataset.slot),
                        top: card.getBoundingClientRect().top,
                        z: Number(getComputedStyle(card).zIndex),
                    })),
                );
            assert.equal(layers.length, count);
            const ordered = layers.sort((a, b) => b.slot - a.slot);
            assert.ok(
                ordered.every(
                    (layer, i) =>
                        i === 0 ||
                        (layer.top < ordered[i - 1].top &&
                            layer.z < ordered[i - 1].z),
                ),
                "deeper cards fan behind the top card in distinct layers",
            );
            await button.click();
            const dialog = page.getByRole("dialog", { name: "Your graveyard" });
            const listed = dialog.locator("[data-viewer-slot]");
            assert.equal(await listed.count(), count);
            const slot = await listed.last().getAttribute("data-viewer-slot");
            const original = page.locator(
                `.card-layer > [data-slot="${slot}"]`,
            );
            assert.equal(await original.isVisible(), false);
            if (count === 11) {
                await page.clock.runFor(500);
                await listed.last().scrollIntoViewIfNeeded();
                const within = await listed.last().evaluate((card) => {
                    const list = card.parentElement.getBoundingClientRect();
                    const box = card.getBoundingClientRect();
                    return box.bottom <= list.bottom;
                });
                assert.ok(within, "the last card scrolls into view");
            }
            await dialog.getByRole("button", { name: "Close" }).click();
            assert.equal(await original.isVisible(), true);
        },
        { ...pileRun, fixture, viewport: { width: 1180, height: 820 } },
    );
}

const jace = await lastDecision("choice-jace");
const [first, second] = [...jace.bySlot.keys()];
const fatesealFixture = "choice-jace-fateseal";
await derive("choice-jace", fatesealFixture, jace.views.length - 1, (v) => ({
    ...v,
    status: {
        kind: "deciding",
        decision: {
            ...v.status.decision,
            kind: "yesNo",
            prompt: asking(v.status.decision.prompt, "yesNo", "fateseal", {
                title: "Top of Library",
                words: { yes: "BOTTOM", no: "TOP" },
                about: [v.players.you.library.at(-1)],
                from: { player: "you", zone: "library" },
            }),
            candidates: ["yes", "no"].map((kind) => ({ option: { kind } })),
        },
    },
}));
await check(
    "Jace's fateseal gives equal weight to both destinations",
    async (page) => {
        assert.equal(
            await page.locator(".overlay-actions .button-secondary").count(),
            2,
        );
    },
    {
        fixture: fatesealFixture,
        at: jace.views.length,
        ready: ".overlay-actions",
    },
);
await check(
    "Undo is accessible over Jace's put-back screen",
    async (page) => {
        await page.clock.runFor(1000);
        await page.getByRole("button", { name: "Undo", exact: true }).click();
        const sent = await page.evaluate(
            () =>
                window.berylFixture.calls.filter((c) => c.command === "undo")
                    .length,
        );
        assert.equal(sent, 1);
    },
    { fixture: "choice-jace", at: jace.views.length, ready: tray(first) },
);

await check(
    "dragging two cards onto the library puts them back in order",
    async (page) => {
        await page.clock.runFor(2000);
        const done = page.getByRole("button", { name: "Done" });
        assert.ok(await done.isDisabled(), "Done waits for two cards");
        // The library lies at the left, the hand fanned at the right.
        const deck = await page.locator(".choice-library").boundingBox();
        const end = {
            x: deck.x + deck.width / 2,
            y: deck.y + deck.height / 2,
        };
        for (const slot of [first, second]) {
            const box = await page.locator(tray(slot)).boundingBox();
            assert.ok(
                box.x > deck.x + deck.width,
                "the hand is right of the library",
            );
            // By its name bar, which the fanned cards after it leave showing.
            const grab = { x: box.x + 24, y: box.y + 30 };
            await page.mouse.move(grab.x, grab.y);
            await page.mouse.down();
            for (let i = 1; i <= 10; i++) {
                await page.mouse.move(
                    grab.x + ((end.x - grab.x) * i) / 10,
                    grab.y + ((end.y - grab.y) * i) / 10,
                );
            }
            await page.mouse.up();
            await page.clock.runFor(2000);
        }
        assert.ok(await done.isEnabled(), "two cards are on the library");
        const [under, over] = await Promise.all(
            [first, second].map((s) => page.locator(tray(s)).boundingBox()),
        );
        assert.ok(
            over.y > under.y && over.x < deck.x + deck.width,
            "the top card lies in front, lower down the library",
        );
        await done.click();
        assert.deepEqual(
            await settled(page, 1),
            [jace.bySlot.get(first)],
            "the card under the top one goes back first",
        );
    },
    { fixture: "choice-jace", at: jace.views.length, ready: tray(first) },
);

// Mana Tithe asks the human to pay {1} on the board, the table in view: the land the payment
// would tap glows on hover; the prompt answers from its buttons.
const pay = await lastDecision("choice-pay");
const decline = pay.decision.candidates.findIndex(
    (c) => c.option.kind === "no",
);
const payTaps = pay.decision.candidates.find((c) => c.option.kind === "yes")
    .payments[0].taps;
assert.equal(payTaps.length, 1, "one land pays {1}");
await check(
    "a pay-or-counter prompt lights the land that would pay",
    async (page) => {
        await page.clock.runFor(2000);
        assert.equal(await page.$(".choice"), null, "no overlay opens");
        assert.equal(await page.$(".table-dimmed"), null, "the table shows");
        assert.deepEqual(await lit(page), []);
        await page.getByRole("button", { name: /^Pay/ }).hover();
        assert.deepEqual(await lit(page), payTaps, "the paying land glows");
        await page.getByRole("button", { name: "Decline" }).click();
        assert.deepEqual(await settled(page, 1), [decline]);
    },
    { fixture: "choice-pay", at: pay.views.length, ready: ".prompt" },
);

await check(
    "View Battlefield puts the overlay aside until asked back",
    async (page) => {
        await page.getByRole("button", { name: "View Battlefield" }).click();
        assert.equal(await page.$(".choice"), null, "the overlay is aside");
        assert.equal(
            await page.$(".table-dimmed"),
            null,
            "the battlefield is clear",
        );
        await page.click(".overlay-return");
        await page.waitForSelector(".choice");
        assert.deepEqual(await chosen(page), [], "peeking answers nothing");
    },
    {
        fixture: "choice-look",
        at: (await load("choice-look")).length,
        ready: ".choice",
    },
);

// Menus and prompts say what each option does (by-isv.8). `menu-choices` stops at a priority
// decision with Jace, the Mind Sculptor's three loyalty abilities and Sink into Stupor, a
// spell whose back is a land: every card's menu draws its costs as symbols, never braces;
// pointing at a loyalty ability lights its paragraph of Jace's text; and the land is played
// by its land face's name.
const menus = await lastDecision("menu-choices");
const menuView = menus.views.at(-1);
const nameOfSlot = (view, slot) =>
    view.objects.find((o) => o.slot === slot)?.name;
const menuSlots = [
    ...Map.groupBy(
        menus.decision.candidates.filter(
            (c) => offeredSlot(c.option) !== undefined,
        ),
        (c) => offeredSlot(c.option),
    ),
]
    .filter(([, cs]) => cs.length > 1)
    .map(([slot]) => slot);
const sculptor = menuView.objects.find(
    (o) => o.name === "Jace, the Mind Sculptor" && o.zone === "battlefield",
).slot;
const stupor = menuView.objects.find(
    (o) => o.name === "Sink into Stupor" && o.zone === "hand",
).slot;
assert.ok(
    menuSlots.includes(sculptor) && menuSlots.includes(stupor),
    "Jace and Sink into Stupor each offer more than one thing",
);
// Teferi, Time Raveler at 1 loyalty can activate only its +1: a loyalty ability cannot be
// taken back, so the click still opens the chooser, the −3 shown disabled.
const lowViews = await load("walker-low");
const lowView = lowViews.at(-1);
const raveler = lowView.objects.find(
    (o) => o.name === "Teferi, Time Raveler" && o.zone === "battlefield",
).slot;
const ravelerActs = lowView.status.decision.candidates.flatMap((c, i) =>
    c.option.kind === "activate" && c.option.slot === raveler ? [i] : [],
);
assert.equal(ravelerActs.length, 1, "Teferi offers its +1 alone");
for (const phone of [false, true]) {
    await check(
        `the loyalty badge centers the number in the shield on ${phone ? "phone" : "desktop"}`,
        async (page) => {
            const badge = page.locator(
                `[data-slot="${raveler}"] .card-loyalty`,
            );
            const numberCenter = await badge.evaluate((e) => {
                const shield = e
                    .querySelector(".card-loyalty-glyph")
                    .getBoundingClientRect();
                const number = e.querySelector("b").getBoundingClientRect();
                return (
                    (number.top + number.height / 2 - shield.top) /
                    shield.height
                );
            });
            assert.ok(
                numberCenter >= 0.54 && numberCenter <= 0.64,
                `the number sits in the body below the shield's top notch: ${numberCenter}`,
            );
        },
        {
            fixture: "walker-low",
            at: lowViews.length,
            ready: ".card-loyalty",
            phone,
            viewport: phone ? { width: 390, height: 844 } : undefined,
        },
    );
}
await check(
    "a planeswalker's click opens its chooser, unaffordable abilities disabled",
    async (page) => {
        await page.clock.runFor(3000);
        await page.click(`[data-slot="${raveler}"]`);
        const items = page.locator(".ability-choice");
        assert.deepEqual(
            await items.evaluateAll((es) =>
                es.map((e) => [
                    e.getAttribute("aria-label").split(":")[0],
                    e.disabled,
                ]),
            ),
            [
                ["+1", false],
                ["\u22123", true],
            ],
            "+1, then −3 disabled",
        );
        assert.match(
            await items.nth(1).textContent(),
            /Return up to one target artifact/,
            "the −3 says what it does",
        );
        assert.deepEqual(await chosen(page), [], "nothing activated");
        await page.clock.runFor(1000);
        await items.nth(1).click({ force: true });
        assert.deepEqual(await chosen(page), [], "the −3 does nothing");
        await items.nth(0).click();
        assert.deepEqual(await settled(page, 1), ravelerActs, "the +1");
    },
    { fixture: "walker-low", at: lowViews.length, ready: ".turn-action" },
);

// A click on a hand card with nothing to offer (by-isv.23): the inspector says why, as the
// engine does. In `menu-choices` Counterspell has no spell to counter.
const refusedCard = menus.decision.refused.find(
    (r) => nameOfSlot(menuView, r.slot) === "Counterspell",
);
assert.deepEqual(
    refusedCard?.reason,
    { kind: "target" },
    "the view refuses Counterspell for want of a target",
);
await check(
    "clicking a refused card shows why in the inspector",
    async (page) => {
        await page.clock.runFor(3000);
        const counterspell = `[data-slot="${refusedCard.slot}"]`;
        const inspect = page.locator(".inspect");
        await page.hover(counterspell);
        await page.getByLabel("Counterspell, inspected").waitFor();
        assert.equal(
            await inspect.locator(".inspect-refusal").count(),
            0,
            "hovering alone says nothing",
        );
        await page.click(counterspell);
        assert.equal(
            await inspect.locator(".inspect-refusal").innerText(),
            "No target",
        );
        assert.equal((await cues(page)).at(-1), "no", "a refusal sounds no");
        await page.hover(".turn-action");
        await page.clock.runFor(500);
        await page.hover(counterspell);
        await page.getByLabel("Counterspell, inspected").waitFor();
        assert.equal(
            await inspect.locator(".inspect-refusal").count(),
            0,
            "the reason goes once the pointer leaves",
        );
    },
    { fixture: "menu-choices", at: menus.views.length, ready: ".turn-action" },
);

// A target prompt with two cards of one name (by-isv.8): `namesake-target` stops on a
// target for Teferi, Hero of Dominaria with a Snapcaster Mage on each side. The prompt lists
// neither; each glows on the table, on its side, and clicking one there picks it.
const namesake = await lastDecision("namesake-target");
const namesakeView = namesake.views.at(-1);
const mages = [...namesake.bySlot.keys()].filter(
    (slot) => nameOfSlot(namesakeView, slot) === "Snapcaster Mage",
);
const theirMage = mages.find(
    (slot) =>
        namesakeView.objects.find((o) => o.slot === slot).controller ===
        "opponent",
);
assert.equal(mages.length, 2, "the fixture offers two Snapcaster Mages");
await check(
    "a target prompt leaves same-named cards to the table",
    async (page) => {
        await page.clock.runFor(3000);
        const buttons = await page
            .locator(".prompt-choices .button")
            .allTextContents();
        assert.ok(
            !buttons.some((b) => b.includes("Snapcaster Mage")),
            `the prompt lists no Snapcaster Mage: ${buttons}`,
        );
        for (const slot of mages) {
            assert.ok(
                await page.$(
                    `[data-slot="${slot}"][data-zone="battlefield"] .card-target`,
                ),
                "each glows on the table",
            );
        }
        await page.click(`[data-slot="${theirMage}"]`);
        assert.deepEqual(await settled(page, 1), [
            namesake.bySlot.get(theirMage),
        ]);
    },
    {
        fixture: "namesake-target",
        at: namesake.views.length,
        ready: ".prompt",
    },
);

// Paying life for a land (by-isv.8): `pay-life` stops as Sink into Stupor is played as
// Soporific Springs with three untapped lands, Jace, the Mind Sculptor in hand the one card
// the fourth would make castable. The floating prompt names the land over its buttons and
// defaults to paying; with Jace renamed Counterspell, nothing wants the mana this turn and it
// defaults to entering tapped.
const payLife = await lastDecision("pay-life");
const answerOf = (kind) =>
    payLife.decision.candidates.findIndex((c) => c.option.kind === kind);
await writeFixture(
    "pay-life-unwanted",
    JSON.parse(
        (await readFile(join(fixtures, "pay-life.json"), "utf8")).replaceAll(
            "Jace, the Mind Sculptor",
            "Counterspell",
        ),
    ),
);
for (const [fixture, primary, kind] of [
    ["pay-life", "Pay 3 Life", "yes"],
    ["pay-life-unwanted", "Enters Tapped", "no"],
]) {
    await check(
        `paying life for a land defaults by whether its mana is wanted (${fixture})`,
        async (page) => {
            await page.clock.runFor(2000);
            assert.equal(
                await page.locator(".prompt-title").textContent(),
                "Pay 3 life?",
                "Arena's question",
            );
            assert.equal(await page.$(".overlay"), null, "the table stays lit");
            const confirm = page.locator(".prompt .button-primary");
            assert.equal(await confirm.textContent(), primary);
            await confirm.click();
            assert.deepEqual(await settled(page, 1), [answerOf(kind)]);
        },
        { fixture, at: payLife.views.length, ready: ".prompt" },
    );
}

// The trigger-order prompt (still asked under exact stops) shows each trigger by its source,
// unnumbered until picked, and says what it does only of two from one source; picking the one
// that resolves first puts the other on the stack, the first going on after it unasked.
const order = await lastDecision("choice-trigger-order");
await check(
    "the trigger-order prompt names each trigger and picks what resolves first",
    async (page) => {
        await page.clock.runFor(3000);
        assert.equal(
            await page.locator(".prompt-trigger-text").count(),
            0,
            "two sources need no words",
        );
        assert.equal(await page.locator(".prompt-trigger").count(), 2);
        assert.equal(
            await page.locator(".prompt-title").textContent(),
            "Resolve first",
        );
        assert.deepEqual(
            await page.locator(".prompt-trigger-order").allTextContents(),
            ["", ""],
            "nothing is numbered yet",
        );
        await page.locator(".prompt-trigger").first().click();
        assert.deepEqual(await settled(page, 1), [1], "the other goes first");
    },
    {
        fixture: "choice-trigger-order",
        at: order.views.length,
        ready: ".prompt-trigger",
    },
);

// Three triggers, two of one source: picked to resolve C, then A, then B, they go on the
// stack B, then A, C last unasked. Later picks name their trigger by source and ability, as
// the candidates' indexes shift once one is on the stack.
{
    const t = (index, source, ability) => ({
        kind: "trigger",
        index,
        source,
        ability,
    });
    const [a, b, c] = [t(0, 10, 0), t(1, 10, 2), t(2, 20, 0)];
    const plan = triggerPlan(
        { candidates: [a, b, c].map((option) => ({ option })) },
        [2, 0],
    );
    assert.equal(plan.length, 2, "the first to resolve goes on unasked");
    assert.ok(plan[0].wants(b) && !plan[0].wants(a) && !plan[0].wants(c));
    // Then, with B on the stack, A is first of those left.
    const [a2, c2] = [t(0, 10, 0), t(1, 20, 0)];
    assert.ok(plan[1].wants(a2) && !plan[1].wants(c2) && plan[1].waits);
    assert.ok(!plan[1].wants(t(0, 10, 2)), "not the other of its source");
}

// The trigger-order prompt with a third trigger (`choice-trigger-order` with its first
// trigger's source given a second one): each pick but the last is numbered as it resolves,
// the title asking for the next; the last pick sends the order.
await derive(
    "choice-trigger-order",
    "choice-trigger-order-three",
    order.views.length - 1,
    (v) => {
        const d = v.status.decision;
        const [first] = d.candidates;
        d.candidates.push({
            ...first,
            option: { ...first.option, index: 2, ability: 9 },
        });
        return v;
    },
);
await check(
    "three triggers are numbered in resolve order as they are picked",
    async (page) => {
        await page.clock.runFor(3000);
        const triggers = page.locator(".prompt-trigger");
        await triggers.nth(2).click();
        assert.deepEqual(
            await page.locator(".prompt-trigger-order").allTextContents(),
            ["", "", "1"],
        );
        assert.equal(
            await page.locator(".prompt-title").textContent(),
            "Resolve next",
        );
        assert.deepEqual(await chosen(page), [], "nothing is sent yet");
        await triggers.nth(0).click();
        // The one left resolves last: it goes on the stack first.
        assert.deepEqual(await settled(page, 1), [1]);
    },
    {
        fixture: "choice-trigger-order-three",
        at: order.views.length,
        ready: ".prompt-trigger",
    },
);

// The trigger-order prompt names each trigger's source in full,
// even the longest card names Beryl knows: `choice-trigger-order` stops on one between two
// permanents, renamed here to the two longest names in `beryl_cards`' generated name list, and
// no text on the page is cut.
const known = await readFile(
    new URL("../../rust/crates/beryl_cards/src/names.rs", import.meta.url),
    "utf8",
);
const longest = [...known.matchAll(/^pub const \w+: &str = "(.*)";$/gm)]
    .map((m) => JSON.parse(`"${m[1]}"`))
    .sort((a, b) => b.length - a.length)
    .slice(0, 2);
const sources = order.decision.candidates.map(
    (c) =>
        order.views.at(-1).objects.find((o) => o.slot === c.option.source).name,
);
assert.equal(sources.length, 2, "the fixture orders two triggers");
let renamed = await readFile(
    join(fixtures, "choice-trigger-order.json"),
    "utf8",
);
for (const [i, name] of sources.entries()) {
    renamed = renamed.replaceAll(JSON.stringify(name).slice(1, -1), longest[i]);
}
await writeFixture("choice-trigger-order-long", JSON.parse(renamed));
await check(
    "the trigger-order prompt shows the longest names in full",
    async (page) => {
        await page.clock.runFor(3000);
        assert.deepEqual(await cutText(page), [], "no text is cut");
        const choices = page.locator(".prompt-trigger");
        assert.deepEqual(
            await page.locator(".prompt-trigger-name").allTextContents(),
            longest,
            "each trigger is named by its source",
        );
        // The second resolves first: the first goes on the stack.
        await choices.nth(1).click();
        assert.deepEqual(await settled(page, 1), [0]);
    },
    {
        fixture: "choice-trigger-order-long",
        at: order.views.length,
        ready: ".prompt",
    },
);

// The stack's arrows (by-l1w.8): a spell aimed at a card beside the stack, an ability aimed at
// its source's neighbor, one aimed at a player, and a spell aimed at a creature under one aimed
// at it (by-97z). A spell aimed at the only other spell needs no arrow; an ability's source only
// while the pointer is on the ability (by-wkr). Each arrow ties the elements its item names,
// starts and ends on their edges apart from every other arrow's ends, is long enough to read,
// and runs mostly clear of the other cards on the table.
for (const fixture of [
    "arrow-spell",
    "arrow-ability",
    "arrow-player",
    "arrow-stacked",
]) {
    const views = await load(fixture);
    const view = views.at(-1);
    const top = view.stack.length - 1;
    const item = view.stack[top];
    // The top item's element; beneath it, the strip of the item that peeks out.
    const idOf = (item, i) =>
        i < top
            ? `stack-peek-${i}`
            : item.kind === "spell"
              ? `card-${item.slot}`
              : `stack-${i}`;
    const targets = view.stack
        .map((item, i) =>
            item.targets
                .filter(
                    (t) =>
                        !(
                            top === 1 &&
                            t.kind === "object" &&
                            view.stack.some(
                                (s) => s.kind === "spell" && s.slot === t.slot,
                            )
                        ),
                )
                .map((t) => [
                    idOf(item, i),
                    t.kind === "player"
                        ? `${t.player}-life-orb`
                        : `card-${t.slot}`,
                ]),
        )
        .reverse()
        .flat();
    const sourced = item.kind === "ability" && item.source !== null;
    const expected = [
        ...(sourced ? [[`card-${item.source}`, idOf(item, top)]] : []),
        ...targets,
    ];
    if (sourced) {
        await check(
            `${fixture}: an ability's source arrow shows while it is pointed at`,
            async (page) => {
                await page.clock.runFor(3000);
                const ends = () =>
                    page.$$eval(".board-arrows .arrow", (gs) =>
                        gs.map((g) => [g.dataset.from, g.dataset.to]),
                    );
                assert.deepEqual(await ends(), targets, "targets alone");
                await page.hover(`#card-${item.source}`);
                await page.clock.runFor(500);
                assert.deepEqual(await ends(), targets, "source card alone");
                await page.hover(`#stack-${top}`);
                await page.clock.runFor(500);
                assert.deepEqual(await ends(), expected, "and the source");
                await page.mouse.move(5, 450);
                await page.clock.runFor(500);
                assert.deepEqual(await ends(), targets, "targets again");
            },
            { fixture, at: views.length, ready: ".arrow" },
        );
    }
    if (fixture === "arrow-ability") {
        await check(
            "target arrows disappear while a card moves",
            async (page) => {
                const targetArrow =
                    ".board-arrows .arrow:is(.arrow-red, .arrow-green)";
                const arrow = page.locator(targetArrow);
                // The target's arrow shows once both its ends have rested.
                await until(page, targetArrow);
                const target = await arrow.getAttribute("data-to");
                const card = page.locator(`#${target}`);
                // Time between browser calls must not count toward the arrow's rest.
                await page.clock.pauseAt(Date.now() + 60_000);
                // Even movement below a pixel breaks the arrow's rest period.
                for (const x of [0.1, 0.2, 0.3]) {
                    await card.evaluate((el, x) => {
                        el.style.translate = `${x}px`;
                    }, x);
                    await page.clock.runFor(50);
                    assert.equal(await arrow.count(), 0);
                }
                await until(page, targetArrow);
            },
            { fixture, at: views.length, ready: ".arrow" },
        );
    }
}

// Play the target announcement live. Static fixture views keep arrows for inspection.
if (wanted("a live card target keeps its arrow while stationary")) {
    const current = (await load("arrow-ability")).at(-1);
    const item = current.stack.at(-1);
    const target = item.targets[0].slot;
    const fixture = "arrow-live";
    const untargeted = { ...item, targets: [] };
    await writeFixture(fixture, {
        messages: [
            { kind: "response", command: "choose", value: null },
            ...[{ ...current, stack: [untargeted], events: [] }, current].map(
                (view) => ({
                    kind: "event",
                    event: "game-update",
                    payload: { session: 1, view },
                }),
            ),
        ],
    });
    await check(
        "a live card target keeps its arrow while stationary",
        async (page) => {
            const arrow = ".board-arrows .arrow:is(.arrow-red, .arrow-green)";
            assert.equal(await page.locator(arrow).count(), 0);
            await page
                .getByRole("button", { name: "Resolve", exact: true })
                .click();
            await until(page, arrow, 25);
            assert.equal(
                await page.locator(arrow).getAttribute("data-to"),
                `card-${target}`,
            );
            await page.clock.runFor(5000);
            assert.equal(await page.locator(arrow).count(), 1);
        },
        { fixture, at: 1, ready: "#stack-0" },
    );
}

/** A cut from a fixture, played from its first view into the rest on the next answer. */
async function motionFixture(name, views) {
    await writeFixture(name, {
        messages: [
            { kind: "response", command: "choose", value: null },
            ...views.map((view) => ({
                kind: "event",
                event: "game-update",
                payload: { session: 1, view },
            })),
        ],
    });
    return { fixture: name, at: 1 };
}

const localScryReturn =
    "confirming scry lands on top and tucks beneath the deck";
if (wanted(localScryReturn)) {
    const before = { ...scry.views.at(-1), events: [] };
    const library = before.players.you.library;
    const [bottom, top] = library.slice(-2);
    const priority = scry.views.findLast(
        (view) => view.status.decision?.kind === "priority",
    ).status.decision;
    const priorId = before.status.decision.id;
    const result = {
        ...before,
        status: {
            kind: "deciding",
            decision: { ...priority, id: { ...priorId, seq: priorId.seq + 2 } },
        },
        players: {
            ...before.players,
            you: {
                ...before.players.you,
                library: [bottom, ...library.slice(0, -2), top],
            },
        },
        events: [
            { kind: "scry", player: "you", top: 1, bottom: 1 },
            { kind: "abilityFinished" },
        ],
    };
    const afterPick = {
        ...before,
        status: {
            kind: "deciding",
            decision: {
                ...before.status.decision,
                id: { ...priorId, seq: priorId.seq + 1 },
                candidates: before.status.decision.candidates.filter(
                    (candidate) => candidate.option.kind === "done",
                ),
            },
        },
    };
    const run = await motionFixture("local-scry-return", [
        before,
        afterPick,
        result,
    ]);
    await check(
        localScryReturn,
        async (page) => {
            const deck = (slot) => page.locator(`.card-layer ${slotOf(slot)}`);
            await page.clock.runFor(650);
            assert.equal(await deck(bottom).locator(".card-back").count(), 0);
            assert.equal(
                await page.locator(tray(bottom)).locator(".card-back").count(),
                0,
            );
            await page.locator(tray(bottom)).click({ force: true });
            await page.clock.runFor(500);
            await startRecording(page);
            await page.clock.pauseAt(Date.now() + 60_000);
            await page
                .getByRole("button", { name: "Done", exact: true })
                .click({ force: true });
            await page.clock.runFor(120);
            assert.equal(await page.locator(".card-el-scry-bottom").count(), 1);
            await page.clock.runFor(250);
            assert.equal(await deck(bottom).locator(".card-back").count(), 1);
            await page.clock.runFor(350);
            assert.ok(
                await deck(bottom).evaluate((e) =>
                    e.classList.contains("card-el-pile"),
                ),
            );
            const layer = (card) =>
                card.evaluate((e) => Number(e.style.zIndex));
            assert.ok(
                (await layer(deck(bottom))) < (await layer(deck(library[0]))),
                "the bottom card enters below the entire deck",
            );
            await page.clock.runFor(900);
            const motion = await page.evaluate(collect, 0);
            assert.deepEqual(duplicatesIn(motion), []);
            const track = posesOf(motion, top);
            const home = track.at(-1);
            assert.equal(home.librarySide, "you");
            assert.ok(
                track.every((pose) => pose.back !== true),
                "the retained top card keeps the face shown in the chooser",
            );
            assert.ok(
                track
                    .filter((c) => c.zone === "library")
                    .every((c) => c.y <= home.y + 3),
                "the top return never dips below its landing",
            );
        },
        { ...run, ready: tray(top) },
    );
}

const parkedHand = "opponent hand stays parked during a private choice";
if (wanted(parkedHand)) {
    const opening = (await load("opening-bottom")).find(
        (v) => v.status.decision?.kind === "keepOrMulligan",
    );
    for (const [choice, view] of [
        ["mulligan", opening],
        ["discard", turnViews[mainHalt]],
    ]) {
        const before = { ...view, events: [] };
        const thinking = {
            ...before,
            status: { kind: "thinking" },
            handling: { zone: "hand", side: "opponent" },
        };
        const hand = before.objects.filter(
            (o) => o.zone === "hand" && o.owner === "opponent",
        );
        assert.ok(hand.length > 1, "there is a hand to keep parked");
        const watched = [
            ...hand.map((o) => o.slot),
            before.players.opponent.library.at(-1),
        ];
        const run = await motionFixture(`parked-opponent-${choice}`, [
            before,
            thinking,
        ]);
        await check(
            `${parkedHand} (${choice})`,
            async (page) => {
                await page.clock.runFor(800);
                await startRecording(page);
                await page.clock.pauseAt(Date.now() + 60_000);
                const answer =
                    choice === "mulligan"
                        ? page.getByRole("button", {
                              name: /^Keep \d$/,
                          })
                        : page.locator(".turn-action button").first();
                await answer.evaluate((button) => button.click());
                await page.clock.runFor(950);
                assert.equal(
                    await page.evaluate(
                        () => window.berylFixture.view().handling?.zone,
                    ),
                    "hand",
                );
                const motion = await page.evaluate(collect, 0);
                for (const slot of watched) {
                    const poses = posesOf(motion, slot);
                    assert.ok(
                        poses.length > 1,
                        `slot ${slot} stays visible in the recording`,
                    );
                    const home = [poses[0].x, poses[0].y];
                    for (const pose of poses) {
                        assertNear(
                            [pose.x, pose.y],
                            home,
                            `slot ${slot} stays seated`,
                        );
                    }
                }
                assert.equal(
                    await page.locator(".card-el-handling").count(),
                    0,
                );
            },
            {
                ...run,
                ready:
                    choice === "mulligan"
                        ? ".opening-hand"
                        : ".turn-action button",
            },
        );
    }
}

const opponentScry =
    "opponent scry fans out, returns to both ends and clears its result";
// Opens for every check below: "opponent scry plays consecutive results..." too.
if (wanted("opponent scry")) {
    const before = { ...turnViews[mainHalt], events: [] };
    const library = before.players.opponent.library;
    const held = library.slice(-2);
    const looking = {
        ...before,
        status: { kind: "thinking" },
        handling: { zone: "library", side: "opponent", count: 2, scry: true },
    };
    for (const bottom of [0, 1, 2]) {
        const top = held.length - bottom;
        const reordered = [
            ...held.slice(top),
            ...library.slice(0, -held.length),
            ...held.slice(0, top).reverse(),
        ];
        const finished = {
            ...before,
            players: {
                ...before.players,
                opponent: { ...before.players.opponent, library: reordered },
            },
            events: [{ kind: "scry", player: "opponent", top, bottom }],
        };
        const run = await motionFixture(`opponent-scry-${bottom}`, [
            before,
            looking,
            { ...looking, players: finished.players },
            finished,
        ]);
        await check(
            `${opponentScry} (${top} top, ${bottom} bottom)`,
            async (page) => {
                await page.clock.runFor(800);
                const backs = page.locator(".card-el-handling");
                const cards = held.map((slot) =>
                    page.locator(`.card-layer ${slotOf(slot)}`),
                );
                const home = await cards[1].boundingBox();
                const parked = (slot) =>
                    page.locator(
                        `${slotOf(slot)}[data-library-pile="opponent"]`,
                    );
                const staying = await where(parked(library[0]));
                for (const card of cards)
                    await card.evaluate((e) => (e.dataset.original = "true"));

                await page.clock.pauseAt(Date.now() + 60_000);
                await page.locator(".turn-action button").first().click();
                await until(page, ".card-el-handling", 16);
                await page.clock.runFor(720);
                assert.equal(await backs.count(), 2);
                assert.equal(await backs.locator(".card-back").count(), 2);
                assert.equal(
                    await page.locator(".card-el-handling[data-card]").count(),
                    0,
                );
                for (const card of cards) {
                    const box = await card.boundingBox();
                    assert.ok(
                        box.width > home.width * 1.5,
                        "the lift enlarges the backs",
                    );
                    assert.ok(
                        box.x > home.x - home.width &&
                            box.y > home.y + home.height,
                        "the enlarged fan clears the corner piles",
                    );
                }
                const surface = parked(library.at(-3));
                assert.equal(await surface.locator(".card-back").count(), 1);
                assert.equal(
                    await surface
                        .locator(".card-shade")
                        .evaluate((e) => Number(e.style.opacity)),
                    0,
                    "the cards below the look keep an unshaded back on top",
                );
                const resting = await where(parked(library[0]));
                assertNear(
                    resting,
                    staying,
                    "the rest of the library stays put",
                );

                await until(page, ".library-choice-hud", 16);
                assert.equal(
                    await backs.count(),
                    0,
                    "the backs return without a separate packed hold",
                );
                assert.equal(
                    await page.locator(".library-choice-hud").innerText(),
                    `Scry · ${top} top · ${bottom} bottom`,
                );
                if (top > 0) {
                    assert.equal(
                        await surface.locator(".card-back").count(),
                        1,
                        "the remaining deck keeps its card back while the top packet is in flight",
                    );
                }
                await page.clock.runFor(550);
                const tucked = page.locator(".card-el-scry-bottom");
                assert.deepEqual(
                    await tucked.evaluateAll((cards) =>
                        cards.map((c) => Number(c.dataset.slot)).sort(),
                    ),
                    held.slice(top).sort(),
                    "only bottom-bound cards take the lower route",
                );
                assert.equal(
                    await tucked.locator(".card-back").count(),
                    bottom,
                );
                for (const card of await tucked.all()) {
                    const box = await card.boundingBox();
                    assert.ok(
                        box.x > home.x + home.width * 0.35 &&
                            box.y > home.y + home.height * 0.8,
                        "the bottom route rounds the pile's lower right edge",
                    );
                }
                for (const card of cards.slice(0, top)) {
                    const [x, y] = await where(card);
                    assert.ok(
                        Math.hypot(x - home.x, y - home.y) < 24,
                        "top-bound backs have already returned directly",
                    );
                }
                // The result expires without another view.
                await untilGone(page, ".library-choice-hud", 1800);
                for (const card of cards) {
                    assert.equal(await card.count(), 1);
                    assert.equal(
                        await card.getAttribute("data-original"),
                        "true",
                    );
                    const [x] = await where(card);
                    assert.ok(
                        Math.abs(x - home.x) < 28,
                        "each back returns to the library",
                    );
                }
            },
            { ...run, ready: ".turn-action button" },
        );
    }
    const [moved, kept] = held;
    const reordered = [moved, ...library.slice(0, -2), kept];
    const next = {
        ...looking,
        players: {
            ...before.players,
            opponent: { ...before.players.opponent, library: reordered },
        },
        events: [{ kind: "scry", player: "opponent", top: 1, bottom: 1 }],
    };
    const run = await motionFixture("opponent-scry-consecutive", [
        before,
        looking,
        next,
        {
            ...next,
            status: before.status,
            handling: undefined,
            events: [{ kind: "scry", player: "opponent", top: 2, bottom: 0 }],
        },
    ]);
    await check(
        "opponent scry plays consecutive results and lifts the new top cards",
        async (page) => {
            await page.clock.pauseAt(Date.now() + 60_000);
            await page.locator(".turn-action button").first().click();
            const caption = page.locator(".library-choice-hud");
            const backs = page.locator(".card-el-handling");
            const slots = () =>
                backs.evaluateAll((cards) =>
                    cards.map((c) => Number(c.dataset.slot)).sort(),
                );
            await until(page, ".card-el-handling", 16);
            assert.deepEqual(await slots(), [...held].sort());
            await until(page, ".card-el-scry-bottom", 16);
            assert.equal(await caption.innerText(), "Scry · 1 top · 1 bottom");
            await until(page, ".card-el-handling", 16);
            assert.deepEqual(await slots(), reordered.slice(-2).sort());
            await until(page, '.library-choice-hud:has-text("2 top")', 16);
            await page.clock.runFor(1100);
            assert.equal(await caption.innerText(), "Scry · 2 top · 0 bottom");
            await untilGone(page, ".library-choice-hud", 1800);
        },
        { ...run, ready: ".turn-action button" },
    );
}

const parkedSearch = "an opponent searching keeps their library parked";
if (wanted(parkedSearch)) {
    const before = { ...turnViews[mainHalt], events: [] };
    const searching = {
        ...before,
        status: { kind: "thinking" },
        handling: { zone: "library", side: "opponent", count: null },
    };
    const run = await motionFixture("opponent-parked-search", [
        before,
        searching,
    ]);
    await check(
        parkedSearch,
        async (page) => {
            await page.clock.runFor(1100);
            const library = page.locator(
                '.card-layer [data-library-pile="opponent"]',
            );
            assert.equal(
                await library.count(),
                before.players.opponent.library.length,
            );
            assert.equal(
                await page
                    .locator(
                        '.card-layer [data-library-pile="opponent"][data-card]',
                    )
                    .count(),
                0,
            );
            assert.equal(await page.locator(".card-el-handling").count(), 0);
            assert.equal(await page.locator(".choice-zone").count(), 0);
        },
        { ...run, at: 2, ready: ".card-layer" },
    );
}

const targetDeparture =
    "a resolving spell's arrow stays until its target leaves";
if (wanted(targetDeparture)) {
    const view = (await load("arrow-stacked")).at(-1);
    const item = view.stack[0];
    const target = item.targets[0].slot;
    for (const to of ["graveyard", "exile"]) {
        const before = {
            ...view,
            events: [],
            stack: [{ ...item, targets: [] }],
        };
        const object = view.objects.find((o) => o.slot === target);
        const departure = {
            ...view,
            stack: [item],
            status: { kind: "playing" },
            battlefield: view.battlefield.filter((slot) => slot !== target),
            objects: view.objects.map((o) =>
                o.slot === target ? { ...o, zone: to } : o,
            ),
            players: {
                ...view.players,
                [object.owner]: {
                    ...view.players[object.owner],
                    graveyard:
                        to === "graveyard"
                            ? [...view.players[object.owner].graveyard, target]
                            : view.players[object.owner].graveyard,
                },
            },
            events: [
                { kind: "cast", slot: item.slot, controller: "opponent", item },
                {
                    kind: "zoneChange",
                    slot: target,
                    from: "battlefield",
                    to,
                    reason: "effect",
                },
            ],
        };
        const run = await motionFixture(`arrow-leaves-${to}`, [
            before,
            departure,
            { ...departure, status: view.status, events: [] },
        ]);
        await check(
            `${targetDeparture} for ${to}`,
            async (page) => {
                const caster = view.objects.find(
                    (o) => o.slot === item.slot,
                ).controller;
                const color = object.controller === caster ? "green" : "red";
                const arrow = `.board-arrows .arrow-${color}[data-to="card-${target}"]`;
                const targetCard = page.locator(slotOf(target));
                await page.clock.runFor(1000);
                await page
                    .getByRole("button", { name: "Resolve", exact: true })
                    .click();
                await until(page, arrow, 25);
                assert.equal(
                    await targetCard.getAttribute("data-zone"),
                    "battlefield",
                );
                await page.clock.runFor(800);
                assert.equal(
                    await page.locator(arrow).count(),
                    1,
                    "the target cue fills the pause",
                );
                await until(page, `${slotOf(target)}[data-zone="${to}"]`, 25);
                assert.equal(
                    await page.locator(arrow).count(),
                    0,
                    "the arrow is gone when the move begins",
                );
                await page.clock.runFor(1500);
                assert.equal(
                    await page.locator(arrow).count(),
                    0,
                    "no arrow reappears on the pile",
                );
                assert.equal(
                    await page
                        .locator(slotOf(item.slot))
                        .getAttribute("data-zone"),
                    "stack",
                );
            },
            { ...run, ready: slotOf(target) },
        );
    }
}

/** Verify early progress, arrival, and a straight path on the simulated frame clock. */
async function assertDirectCardFlight(page, card, start) {
    await page.clock.runFor(120);
    const early = center(await card.boundingBox());
    await page.clock.runFor(220);
    const at340 = center(await card.boundingBox());
    await page.clock.runFor(180);
    const end = center(await card.boundingBox());
    const dx = end.x - start.x;
    const dy = end.y - start.y;
    const distance = Math.hypot(dx, dy);
    assert.ok(distance > 80, "the card travels to a different slot");
    const x = early.x - start.x;
    const y = early.y - start.y;
    assert.ok(
        (x * dx + y * dy) / distance > distance * 0.55,
        "most travel happens in the first 120 ms",
    );
    const sideways = Math.abs(x * dy - y * dx) / distance;
    assert.ok(
        sideways < 4,
        `the card travels in a straight line (deviation ${sideways.toFixed(1)} px)`,
    );
    assert.ok(
        Math.hypot(end.x - at340.x, end.y - at340.y) < 3,
        "the card has landed by 340 ms",
    );
}

async function assertTokenFlight(page, token, sourceSelector) {
    await page.clock.runFor(1000);
    const source = await page.locator(sourceSelector).boundingBox();
    const selector = slotOf(token);
    // Sample the mounted token before the next animation frame can carry it away.
    await page.evaluate((selector) => {
        const observe = new MutationObserver(() => {
            const token = document.querySelector(selector);
            if (!token) return;
            window.firstTokenFrame = token.getBoundingClientRect().toJSON();
            observe.disconnect();
        });
        observe.observe(document.body, { childList: true, subtree: true });
    }, selector);
    await page.getByRole("button", { name: "Resolve", exact: true }).click();
    await until(page, selector, 10);
    const card = page.locator(selector);
    const start = center(await page.evaluate(() => window.firstTokenFrame));
    assert.ok(
        start.x >= source.x && start.x <= source.x + source.width,
        "token starts inside its source",
    );
    assert.ok(
        start.y >= source.y && start.y <= source.y + source.height,
        "token starts inside its source",
    );
    await assertDirectCardFlight(page, card, start);
    assert.equal(await card.getAttribute("data-zone"), "battlefield");
    assert.equal(
        await card.evaluate((el) => Number(getComputedStyle(el).opacity)),
        1,
    );
}

const fieldToStack = "a battlefield card flies directly onto the stack";
if (wanted(fieldToStack)) {
    const view = (await load("arrow-stacked")).at(-1);
    const slot = view.stack[0].targets[0].slot;
    const before = {
        ...view,
        stack: [],
        events: [],
        objects: view.objects.filter((o) => o.zone !== "stack"),
    };
    const spell = { kind: "spell", slot, targets: [], paid: [], x: 0 };
    const after = {
        ...before,
        stack: [spell],
        battlefield: before.battlefield.filter((s) => s !== slot),
        objects: before.objects.map((o) =>
            o.slot === slot ? { ...o, zone: "stack" } : o,
        ),
        events: [
            {
                kind: "zoneChange",
                slot,
                from: "battlefield",
                to: "stack",
                reason: "cast",
            },
        ],
    };
    const run = await motionFixture("card-flies-from-battlefield-to-stack", [
        before,
        after,
    ]);
    await check(
        fieldToStack,
        async (page) => {
            const card = page.locator(slotOf(slot));
            const start = center(await card.boundingBox());
            await card.evaluate((el) =>
                el.setAttribute("data-tested-flight", ""),
            );
            await page.keyboard.press("Space");
            await until(page, `${slotOf(slot)}[data-zone="stack"]`, 10);
            await assertDirectCardFlight(page, card, start);
            assert.equal(await card.count(), 1);
            assert.equal(await card.getAttribute("data-tested-flight"), "");
        },
        { ...run, ready: slotOf(slot) },
    );
}

const tokenFlight =
    "a spell's token starts over the spell and flies to the battlefield";
if (wanted(tokenFlight)) {
    const view = (await load("arrow-spell")).at(-1);
    const item = view.stack.at(-1);
    const token = (await load("tokens"))
        .at(-1)
        .objects.find((o) => o.name === "Map Token");
    const before = { ...view, events: [], stack: [{ ...item, targets: [] }] };
    const after = {
        ...view,
        stack: [],
        battlefield: [...view.battlefield, token.slot],
        objects: [
            ...view.objects.map((o) =>
                o.slot === item.slot ? { ...o, zone: "graveyard" } : o,
            ),
            token,
        ],
        events: [
            {
                kind: "zoneChange",
                slot: token.slot,
                from: "nowhere",
                to: "battlefield",
                reason: "created",
            },
            {
                kind: "tokensCreated",
                tokens: [token.slot],
                controller: token.controller,
            },
            {
                kind: "zoneChange",
                slot: item.slot,
                from: "stack",
                to: "graveyard",
                reason: "resolve",
            },
        ],
    };
    const run = await motionFixture("token-flies-from-spell", [before, after]);
    await check(
        tokenFlight,
        async (page) => {
            await assertTokenFlight(page, token.slot, slotOf(item.slot));
        },
        { ...run, ready: slotOf(item.slot) },
    );
}

const abilityTokenFlight =
    "an ability token starts over the resolving indicator";
if (wanted(abilityTokenFlight)) {
    const view = (await load("arrow-spell")).at(-1);
    const sourceObject = view.objects.find((o) => o.zone === "battlefield");
    const item = {
        kind: "ability",
        source: sourceObject.slot,
        name: sourceObject.name,
        ability: 0,
        nth: 0,
        granted: false,
        controller: "you",
        triggered: true,
        targets: [],
        x: 0,
    };
    const token = (await load("tokens"))
        .at(-1)
        .objects.find((o) => o.name === "Map Token");
    const before = { ...view, events: [], stack: [{ ...item, targets: [] }] };
    const after = {
        ...view,
        stack: [],
        battlefield: [...view.battlefield, token.slot],
        objects: [
            ...view.objects.map((o) =>
                o.slot === item.slot ? { ...o, zone: "graveyard" } : o,
            ),
            token,
        ],
        events: [
            {
                kind: "zoneChange",
                slot: token.slot,
                from: "nowhere",
                to: "battlefield",
                reason: "created",
            },
            {
                kind: "tokensCreated",
                tokens: [token.slot],
                controller: token.controller,
            },
        ],
    };
    const run = await motionFixture("token-flies-from-ability", [
        before,
        after,
    ]);
    await check(
        abilityTokenFlight,
        async (page) => {
            await assertTokenFlight(page, token.slot, "#stack-0");
        },
        { ...run, ready: "#stack-0" },
    );
}

const modeHandoff = "a picked loyalty option carries its frame to the stack";
if (wanted(modeHandoff)) {
    const base = (await load("walker-low")).at(-1);
    const walker = base.objects.find((o) => o.name === "Teferi, Time Raveler");
    const { option } = base.status.decision.candidates.find(
        (c) => c.option.kind === "activate" && c.option.slot === walker.slot,
    );
    const ability = {
        kind: "ability",
        source: walker.slot,
        ability: option.ability,
        granted: option.granted,
        name: walker.name,
        triggered: false,
        nth: 0,
        controller: "you",
        targets: [],
        x: 0,
    };
    const run = await motionFixture("loyalty-mode-flight", [
        { ...base, events: [] },
        { ...base, stack: [...base.stack, ability], events: [] },
    ]);
    await check(
        modeHandoff,
        async (page) => {
            await page.click(slotOf(walker.slot));
            const frame = page.locator(".ability-choice:enabled");
            await page.clock.runFor(600);
            await frame.evaluate((button) => {
                const stage = button.closest(".stage");
                button.addEventListener(
                    "click",
                    () => {
                        const from = button
                            .querySelector(".ability-choice-lift")
                            .getBoundingClientRect();
                        const observer = new MutationObserver(() => {
                            const stack = stage.querySelector(".stack-ability");
                            if (!stack) return;
                            observer.disconnect();
                            const to = stack.getBoundingClientRect();
                            stage.dataset.modeFlight = JSON.stringify({
                                distance: Math.hypot(
                                    from.left - to.left,
                                    from.top - to.top,
                                ),
                                scale: to.width / from.width,
                                oldVisible:
                                    button.isConnected &&
                                    getComputedStyle(button).visibility !==
                                        "hidden",
                            });
                        });
                        observer.observe(stage, {
                            subtree: true,
                            childList: true,
                        });
                    },
                    { capture: true, once: true },
                );
            });
            await frame.click();
            await until(page, "[data-mode-flight]", 10);
            const result = JSON.parse(
                await page.locator(".stage").getAttribute("data-mode-flight"),
            );
            assert.ok(result.distance < 12, JSON.stringify(result));
            assert.ok(
                Math.abs(result.scale - 1) < 0.08,
                JSON.stringify(result),
            );
            assert.equal(result.oldVisible, false);
            await page.clock.runFor(550);
            assert.equal(await page.locator(".stack-ability").count(), 1);
        },
        { ...run, ready: slotOf(walker.slot) },
    );
}

const opponentWaterbend =
    "opponent waterbend shows benders, mana, then activation";
if (wanted(opponentWaterbend)) {
    const base = turnViews[mainHalt];
    const payment = (await load("waterbend-tap")).at(-1);
    const nextSlot = Math.max(...base.objects.map((o) => o.slot)) + 1;
    const [iceberg, jace, plains, otherPlains] = [
        "Aang's Iceberg",
        "Jace, Vryn's Prodigy",
        "Plains",
        "Plains",
    ].map((name, i) => ({
        ...payment.objects.find(
            (o) => o.zone === "battlefield" && o.name === name,
        ),
        slot: nextSlot + i,
        owner: "opponent",
        controller: "opponent",
        tapped: false,
    }));
    const sources = [jace, plains, otherPlains];
    const table = {
        ...base,
        objects: [...base.objects, iceberg, ...sources],
        battlefield: [
            ...base.battlefield,
            iceberg.slot,
            ...sources.map((o) => o.slot),
        ],
        events: [],
    };
    const ability = {
        ...payment.stack.at(-1),
        source: iceberg.slot,
        controller: "opponent",
        tapsToPay: true,
    };
    const tap = (o) => ({ kind: "tap", slot: o.slot });
    const run = await motionFixture("opponent-waterbend-beats", [
        table,
        {
            ...table,
            objects: [
                ...base.objects,
                iceberg,
                ...sources.map((o) => ({ ...o, tapped: true })),
            ],
            events: [
                tap(jace),
                ...[plains, otherPlains].flatMap((o) => [
                    tap(o),
                    {
                        kind: "mana",
                        player: "opponent",
                        source: o.slot,
                        color: "white",
                        amount: 1,
                    },
                ]),
                { kind: "ability", item: ability, loyalty: null },
                { kind: "abilityFinished" },
            ],
        },
    ]);
    await check(
        opponentWaterbend,
        async (page) => {
            await startRecording(page);
            await page.locator(".turn-action button").first().click();
            await page.clock.runFor(3500);
            const recording = await page.evaluate(collect, 0);
            const findings = audit(recording);
            const bend = findings.find(
                (f) => f.kind === "tap" && f.what === jace.name,
            );
            const lands = findings.filter(
                (f) => f.kind === "tap" && f.what === plains.name,
            );
            const activate = findings.find(
                (f) =>
                    f.kind === "ability" &&
                    f.what === `ability of ${iceberg.name}`,
            );
            assert.equal(lands.length, 2);
            for (const f of [bend, ...lands])
                assert.equal(f?.verdict, "ok", JSON.stringify(f));
            const firstStackFrame = recording.frames.find((frame) =>
                Object.entries(frame.stack).some(
                    ([key, pose]) =>
                        key.endsWith(`:${iceberg.name}`) &&
                        pose.o > 0.3 &&
                        pose.on,
                ),
            );
            assert.ok(
                firstStackFrame,
                "the ability is visible before resolving",
            );
            assert.ok(
                lands.every((f) => f.at - bend.at >= 250),
                "Jace turns before the Plains",
            );
            assert.ok(
                Math.abs(lands[0].at - lands[1].at) < 80,
                "both Plains turn together",
            );
            assert.ok(
                activate && lands.every((f) => firstStackFrame.t - f.at >= 250),
                "the activation follows payment",
            );
            assert.equal(
                await page.locator(".stack-ability").count(),
                0,
                "the resolved ability leaves",
            );
        },
        { ...run, ready: ".turn-action button" },
    );
}

const tapBeforeSacrifice = "a mana token turns before leaving the battlefield";
if (wanted(tapBeforeSacrifice)) {
    const view = (await load("arrow-spell")).at(-1);
    const map = (await load("tokens")).at(-1).objects.find((o) => o.token);
    for (const side of ["you", "opponent"]) {
        const token = { ...map, owner: side, controller: side, tapped: false };
        const before = {
            ...view,
            objects: [...view.objects, token],
            battlefield: [...view.battlefield, token.slot],
            events: [],
        };
        const retained = side === "opponent";
        const after = {
            ...view,
            objects: retained
                ? [...view.objects, { ...token, zone: "graveyard" }]
                : view.objects,
            players: retained
                ? {
                      ...view.players,
                      [side]: {
                          ...view.players[side],
                          graveyard: [
                              ...view.players[side].graveyard,
                              token.slot,
                          ],
                      },
                  }
                : view.players,
            events: [
                { kind: "tap", slot: token.slot },
                {
                    kind: "zoneChange",
                    slot: token.slot,
                    from: "battlefield",
                    to: "graveyard",
                    reason: "sacrifice",
                },
                {
                    kind: "mana",
                    source: token.slot,
                    player: side,
                    amount: 1,
                    color: "blue",
                },
            ],
        };
        const run = await motionFixture(`tap-before-sacrifice-${side}`, [
            before,
            after,
        ]);
        await check(
            `${tapBeforeSacrifice} (${side})`,
            async (page) => {
                const field = `${slotOf(token.slot)}[data-zone="battlefield"]`;
                await page.evaluate((field) => {
                    const angle = () => {
                        const card = document.querySelector(`${field} .card`);
                        return (
                            card &&
                            Math.abs(
                                new DOMMatrix(getComputedStyle(card).transform)
                                    .b,
                            )
                        );
                    };
                    window.tokenTap = { initial: angle(), rotated: false };
                    const observe = () => {
                        const turn = angle();
                        if (turn === null) return;
                        if (turn > 0.9) window.tokenTap.rotated = true;
                        requestAnimationFrame(observe);
                    };
                    requestAnimationFrame(observe);
                }, field);
                await page
                    .getByRole("button", { name: "Resolve", exact: true })
                    .click();
                await until(
                    page,
                    `${slotOf(token.slot)}[data-zone="graveyard"]`,
                    16,
                );
                assert.deepEqual(
                    await page.evaluate(() => window.tokenTap),
                    { initial: 0, rotated: true },
                    "visible rotation completes on the battlefield before departure",
                );
                assert.equal(await page.locator(field).count(), 0);
            },
            { ...run, ready: slotOf(token.slot) },
        );
    }
}

const tokenDissolve =
    "a ceasing token breaks apart while its spell stays whole";
if (wanted(tokenDissolve)) {
    const base = (await load("arrow-spell")).at(-1);
    const token = (await load("tokens")).at(-1).objects.find((o) => o.token);
    const before = {
        ...base,
        objects: [...base.objects, token],
        battlefield: [...base.battlefield, token.slot],
        events: [],
    };
    const after = {
        ...base,
        events: [
            {
                kind: "zoneChange",
                slot: token.slot,
                from: "battlefield",
                to: "graveyard",
                reason: "destroy",
            },
        ],
    };
    const run = await motionFixture("token-dissolve", [before, after]);
    for (const { viewport, reducedMotion } of motionConfigs) {
        await check(
            `${tokenDissolve} (${viewport.width}x${viewport.height} ${reducedMotion})`,
            async (page) => {
                await startRecording(page);
                await page.clock.pauseAt(Date.now() + 60_000);
                const dying = `[data-ceasing-slot="${token.slot}"]`;
                await page
                    .getByRole("button", { name: "Resolve", exact: true })
                    .click();
                await until(page, dying, 10);
                const card = page.locator(dying);
                assert.equal(
                    (await card.getAttribute("data-dissolving")) !== null,
                    reducedMotion === "no-preference",
                );
                if (reducedMotion === "no-preference") {
                    const mask = () =>
                        card.evaluate(
                            (el) => getComputedStyle(el).maskPosition,
                        );
                    const initial = await mask();
                    await page.clock.runFor(120);
                    assert.notEqual(
                        await mask(),
                        initial,
                        "the mask advances over the token",
                    );
                    assert.equal(
                        await card.evaluate(
                            (el) => getComputedStyle(el).opacity,
                        ),
                        "1",
                    );
                }
                assert.equal(
                    await page
                        .locator(".card-el-stack[data-dissolving]")
                        .count(),
                    0,
                );
                await page.clock.runFor(400);
                assert.equal(await card.count(), 0, "the token has ceased");
                const recording = await page.evaluate(collect, 0);
                assert.deepEqual(duplicatesIn(recording), []);
                const track = posesOf(recording, token.slot);
                assert.equal(new Set(track.map((c) => c.id)).size, 1);
                const masks = new Set(
                    track.flatMap((c) => (c.mask ? [c.mask] : [])),
                );
                if (reducedMotion === "reduce") assert.equal(masks.size, 0);
                else
                    assert.ok(
                        masks.size > 1,
                        "the original token's mask progresses",
                    );
                assert.ok(
                    recording.frames
                        .at(-1)
                        .instances.every((c) => c.id !== track[0].id),
                );
            },
            { ...run, ready: slotOf(token.slot), viewport, reducedMotion },
        );
    }
}

const revealFlip = "a hidden opponent card turns before showing its cast face";
if (wanted(revealFlip)) {
    const base = (await load("arrow-spell")).at(-1);
    const spell = base.stack.at(-1);
    const slot = Math.max(...base.objects.map((o) => o.slot)) + 1;
    const card = {
        ...base.objects.find((o) => o.slot === spell.slot),
        slot,
        owner: "opponent",
        controller: "opponent",
    };
    const before = {
        ...base,
        objects: [...base.objects, { ...card, name: null, zone: "hand" }],
        events: [],
    };
    const after = {
        ...base,
        objects: [...base.objects, { ...card, zone: "stack" }],
        stack: [...base.stack, { ...spell, slot }],
        events: [],
    };
    const run = await motionFixture("cast-reveal", [before, after]);
    for (const { viewport, reducedMotion } of motionConfigs) {
        await check(
            `${revealFlip} (${viewport.width}x${viewport.height} ${reducedMotion})`,
            async (page) => {
                const card = page.locator(slotOf(slot));
                assert.equal(await card.locator(".card-back").count(), 1);
                await startRecording(page);
                await page
                    .getByRole("button", { name: "Resolve", exact: true })
                    .click();
                await until(page, `${slotOf(slot)}[data-zone="stack"]`, 10);
                await page.clock.runFor(600);
                assert.equal(await card.locator(".card-back").count(), 0);
                assert.equal(await card.locator(".card").count(), 1);
                const recording = await page.evaluate(collect, 0);
                assert.deepEqual(duplicatesIn(recording), []);
                const track = posesOf(recording, slot);
                assert.equal(new Set(track.map((c) => c.id)).size, 1);
                assert.equal(track[0].back, true);
                assert.equal(track.at(-1).back, false);
                assert.equal(track.at(-1).zone, "stack");
                const edge = track.findIndex((c) => Math.abs(c.turn) < 0.25);
                if (reducedMotion === "reduce") {
                    assert.equal(edge, -1, "the face changes without a turn");
                } else {
                    const reveal = track.findIndex((c) => c.back === false);
                    assert.ok(
                        edge > 0 && reveal > 0,
                        "the card turns through edge-on",
                    );
                    assert.ok(
                        [track[reveal - 1], track[reveal]].some(
                            (c) => Math.abs(c.turn) < 0.35,
                        ),
                        "the face changes at the turn",
                    );
                }
            },
            { ...run, ready: slotOf(slot), viewport, reducedMotion },
        );
    }
}

const faceFlip = "a face-down permanent turns over before revealing its face";
if (wanted(faceFlip)) {
    const base = (await load("arrow-spell")).at(-1);
    const card = (await load("combat-blockers"))
        .at(-1)
        .objects.find((o) => o.zone === "battlefield" && o.facing === "down");
    assert.ok(card);
    const slot = 800;
    const before = {
        ...base,
        battlefield: [...base.battlefield, slot],
        objects: [...base.objects, { ...card, slot }],
        events: [],
    };
    const after = {
        ...before,
        objects: before.objects.map((o) =>
            o.slot === slot
                ? { ...o, facing: "up", name: "Snapcaster Mage" }
                : o,
        ),
    };
    const run = await motionFixture("face-up-flip", [before, after]);
    await check(
        faceFlip,
        async (page) => {
            await page.clock.runFor(1000);
            // Hold the gesture at a fixed instant while sampling the lift and flip.
            await page.clock.pauseAt(Date.now() + 60_000);
            const card = page.locator(slotOf(slot));
            const rest = await card.boundingBox();
            assert.equal(await card.locator(".card-face-down").count(), 1);
            await page
                .getByRole("button", { name: "Resolve", exact: true })
                .click();
            await page.clock.runFor(650);
            assert.equal(await card.locator(".card-face-down").count(), 1);
            assert.ok(
                (await card.boundingBox()).width > rest.width * 1.5,
                "the back is enlarged before revealing the face",
            );
            await page.clock.runFor(150);
            assert.equal(
                await card.locator(".card-face-down").count(),
                1,
                "old face remains until edge-on",
            );
            const matrix = await card
                .locator(".card-flip")
                .evaluate(
                    (el) => new DOMMatrix(getComputedStyle(el).transform).m11,
                );
            assert.ok(Math.abs(matrix) < 0.95, "card is visibly turning");
            await page.clock.runFor(250);
            assert.equal(await card.locator(".card-face-down").count(), 0);
            const revealed = await card.boundingBox();
            assert.ok(
                revealed.width > rest.width * 1.3,
                `the face stays enlarged (${revealed.width} vs ${rest.width})`,
            );
            await until(page, `${slotOf(slot)}.card-el-field`, 50);
            await page.clock.runFor(800);
            const settled = await card.boundingBox();
            assertNear([settled.x, settled.y], [rest.x, rest.y], "same slot");
            assert.ok(Math.abs(settled.width - rest.width) < 3);
            assert.equal(await card.count(), 1);
        },
        { ...run, ready: slotOf(slot) },
    );
}

const departureFlip = "a face-down permanent reveals its face before landing";
if (wanted(departureFlip)) {
    const base = (await load("arrow-spell")).at(-1);
    const template = (await load("combat-blockers"))
        .at(-1)
        .objects.find((o) => o.zone === "battlefield" && o.facing === "down");
    assert.ok(template);
    const slot = 800;
    for (const owner of ["you", "opponent"]) {
        for (const to of ["library", "graveyard", "exile", "hand"]) {
            const card = { ...template, slot, owner, controller: owner };
            const before = {
                ...base,
                battlefield: [...base.battlefield, slot],
                objects: [
                    ...base.objects,
                    {
                        ...card,
                        name: owner === "you" ? "Snapcaster Mage" : null,
                    },
                ],
                events: [],
            };
            const player = base.players[owner];
            const players = {
                ...base.players,
                [owner]: {
                    ...player,
                    ...(to === "library" || to === "graveyard"
                        ? { [to]: [...player[to], slot] }
                        : {}),
                },
            };
            const after = {
                ...base,
                players,
                objects: [
                    ...base.objects,
                    {
                        ...card,
                        name: "Snapcaster Mage",
                        zone: to,
                        facing: "up",
                    },
                ],
                events: [
                    {
                        kind: "zoneChange",
                        slot,
                        from: "battlefield",
                        to,
                        reason: "effect",
                    },
                ],
            };
            const run = await motionFixture(`departure-flip-${owner}-${to}`, [
                before,
                after,
            ]);
            await check(
                `${departureFlip} (${owner} to ${to})`,
                async (page) => {
                    await page.clock.runFor(1000);
                    const card = page.locator(slotOf(slot));
                    if (owner === "opponent")
                        assert.equal(
                            await card.getAttribute("data-card"),
                            null,
                        );
                    await startRecording(page);
                    await page
                        .getByRole("button", { name: "Resolve", exact: true })
                        .click();
                    await page.clock.runFor(3200);
                    const recording = await page.evaluate(collect, 0);
                    const track = posesOf(recording, slot);
                    assert.equal(track[0].back, true);
                    const front = track.findIndex((c) => c.back === false);
                    assert.ok(front > 0, "the departing card shows its face");
                    assert.ok(
                        [track[front - 1], track[front]].some(
                            (c) => Math.abs(c.turn) < 0.35,
                        ),
                        "the card changes faces edge-on",
                    );
                    assert.ok(
                        track.some(
                            (c) =>
                                c.zone === "battlefield" &&
                                !c.back &&
                                c.top &&
                                c.w > track[0].w * 1.3,
                        ),
                        "the enlarged front is visible before the card leaves",
                    );
                    assert.equal(await card.getAttribute("data-zone"), to);
                    if (to === "library")
                        assert.equal(
                            await card.locator(".card-scan").count(),
                            0,
                        );
                    assert.equal(new Set(track.map((c) => c.id)).size, 1);
                    assert.deepEqual(duplicatesIn(recording), []);
                },
                { ...run, ready: slotOf(slot) },
            );
        }
    }
}

const directPrivateMill =
    "cards in a private library look skip the graveyard showcase";
if (wanted(directPrivateMill)) {
    const base = (await load("arrow-spell")).at(-1);
    await check(directPrivateMill, async (page) => {
        const shown = await page.evaluate(async (base) => {
            const { beats } = await import("/src/game/beats.ts");
            const side = "opponent";
            const library = base.players[side].library;
            const [other, manifest] = library.slice(-2);
            const before = {
                ...base,
                handling: { zone: "library", side, count: 2 },
            };
            const mill = {
                kind: "zoneChange",
                slot: other,
                from: "library",
                to: "graveyard",
                reason: "effect",
            };
            const up = (events) => {
                const moved = new Map(events.map((e) => [e.slot, e.to]));
                return {
                    ...base,
                    events,
                    objects: base.objects.map((o) =>
                        moved.has(o.slot)
                            ? {
                                  ...o,
                                  zone: moved.get(o.slot),
                                  name: "Island",
                                  facing: o.slot === manifest ? "down" : "up",
                              }
                            : o,
                    ),
                };
            };
            const showcased = (from, to) =>
                beats(
                    { session: 1, view: from },
                    { session: 1, view: to },
                ).some((beat) => beat.spotlight?.shown?.has(other));
            const manifestDread = up([
                {
                    kind: "zoneChange",
                    slot: manifest,
                    from: "library",
                    to: "battlefield",
                    reason: "effect",
                },
                mill,
            ]);
            const ownTop = base.players.you.library.at(-1);
            const explore = {
                ...base,
                status: {
                    kind: "deciding",
                    decision: {
                        ...base.status.decision,
                        // A yes or no about the card on top of the library, as an
                        // explore asks.
                        prompt: {
                            ...base.status.decision.prompt,
                            shape: "yesNo",
                            purpose: "explore",
                            part: "pick",
                            count: 1,
                            about: [ownTop],
                            from: { player: "you", zone: "library" },
                        },
                    },
                },
            };
            const explored = up([{ ...mill, slot: ownTop }]);
            const showsExplored = beats(
                { session: 1, view: explore },
                { session: 1, view: explored },
            ).some((beat) => beat.spotlight?.shown?.has(ownTop));
            return [
                showcased(before, manifestDread),
                showcased(base, manifestDread),
                showcased(before, up([mill])),
                showsExplored,
                showcased(base, up([mill])),
            ];
        }, base);
        assert.deepEqual(
            shown,
            [false, false, false, false, true],
            "Manifest, surveil and explore skip the extra reveal; a blind mill still shows its face",
        );
    });
}

const counterFlight =
    "counterspell and its target fly to opposite graveyards together";
if (wanted(counterFlight)) {
    const base = (await load("arrow-spell")).at(-1);
    const spell = base.stack.at(-1);
    const spellCard = base.objects.find((o) => o.slot === spell.slot);
    const otherSide = spellCard.owner === "you" ? "opponent" : "you";
    const slot = 800;
    const counter = {
        ...spell,
        slot,
        targets: [{ kind: "object", slot: spell.slot }],
    };
    const before = {
        ...base,
        events: [],
        stack: [spell, counter],
        objects: [
            ...base.objects,
            {
                ...spellCard,
                slot,
                owner: otherSide,
                controller: otherSide,
                name: "Counterspell",
            },
        ],
    };
    const after = {
        ...before,
        stack: [],
        objects: before.objects.map((o) =>
            o.slot === slot || o.slot === spell.slot
                ? { ...o, zone: "graveyard" }
                : o,
        ),
        players: {
            ...before.players,
            [spellCard.owner]: {
                ...before.players[spellCard.owner],
                graveyard: [
                    ...before.players[spellCard.owner].graveyard,
                    spell.slot,
                ],
            },
            [otherSide]: {
                ...before.players[otherSide],
                graveyard: [...before.players[otherSide].graveyard, slot],
            },
        },
        events: [
            {
                kind: "zoneChange",
                slot: spell.slot,
                from: "stack",
                to: "graveyard",
                reason: "counter",
            },
            {
                kind: "zoneChange",
                slot,
                from: "stack",
                to: "graveyard",
                reason: "resolve",
            },
        ],
    };
    const run = await motionFixture("paired-counter-flight", [before, after]);
    await check(
        counterFlight,
        async (page) => {
            await page.clock.runFor(800);
            await startRecording(page);
            await page
                .getByRole("button", { name: "Resolve", exact: true })
                .click();
            await page.clock.runFor(4200);
            const recording = await page.evaluate(collect, 0);
            assert.deepEqual(duplicatesIn(recording), []);
            const poses = [spell.slot, slot].map((s) => posesOf(recording, s));
            const box = (p) => [p.x, p.y];
            for (const track of poses) {
                assert.equal(track[0].zone, "stack");
                assert.equal(track.at(-1).zone, "graveyard");
                assert.equal(new Set(track.map((p) => p.id)).size, 1);
                assertBetween(
                    track.map(box),
                    box(track[0]),
                    box(track.at(-1)),
                    "flight is direct",
                );
            }
            const midFlight = recording.frames.filter(({ instances }) =>
                poses.every((track) => {
                    const p = instances.find((c) => c.id === track[0].id);
                    return (
                        p?.zone === "graveyard" &&
                        Math.hypot(p.x - track[0].x, p.y - track[0].y) > 40 &&
                        Math.hypot(p.x - track.at(-1).x, p.y - track.at(-1).y) >
                            40
                    );
                }),
            );
            assert.ok(
                midFlight.length >= 4,
                "both cards visibly move in the same frames",
            );
            assert.ok(
                Math.abs(poses[0].at(-1).y - poses[1].at(-1).y) > 200,
                "each lands on its owner's side",
            );
        },
        { ...run, ready: slotOf(slot) },
    );
}

const copyArt = "a nameless two-two token shows the Double Masters Copy art";
if (wanted(copyArt)) {
    const base = (await load("arrow-spell")).at(-1);
    const creature = (await load("combat-blockers"))
        .at(-1)
        .objects.find((o) => o.zone === "battlefield" && o.facing === "down");
    assert.ok(creature);
    const slot = 800;
    const token = { ...creature, slot, name: "", facing: "up", token: true };
    const run = await motionFixture("nameless-copy-art", [
        {
            ...base,
            objects: [...base.objects, token],
            battlefield: [...base.battlefield, slot],
        },
    ]);
    await check(
        copyArt,
        async (page) => {
            await page.route("https://*.scryfall.io/**", (route) =>
                route.fulfill({
                    contentType: "image/svg+xml",
                    body: '<svg xmlns="http://www.w3.org/2000/svg" width="1" height="1"/>',
                }),
            );
            const url = new URL(page.url());
            url.searchParams.set("art", "");
            await page.goto(url.href);
            const copy = page.locator(slotOf(slot));
            const art = copy.locator("img.card-art");
            await art.waitFor();
            assert.match(
                await art.getAttribute("src"),
                /art_crop.*6870a2af-80e1-4813-a332-2b1bd789f1c1/,
            );
            assert.match(
                await copy.locator(".card-pt").textContent(),
                /2\s*\/\s*2/,
            );
            assert.equal(await copy.getAttribute("data-card"), "");
            assert.equal(
                await copy.locator(".card-scan, .card-face-down").count(),
                0,
            );
        },
        { ...run, ready: slotOf(slot) },
    );
}

const taxFlow = "resolution payment mana reaches its spell before it leaves";
if (wanted(taxFlow)) {
    const base = (await load("arrow-spell")).at(-1);
    const item = base.stack.at(-1);
    const land = base.objects.find((o) => o.zone === "battlefield");
    const before = { ...base, events: [] };
    const owner = base.objects.find((o) => o.slot === item.slot).owner;
    const after = {
        ...base,
        stack: [],
        players: {
            ...base.players,
            [owner]: {
                ...base.players[owner],
                graveyard: [...base.players[owner].graveyard, item.slot],
            },
        },
        objects: base.objects.map((o) =>
            o.slot === item.slot ? { ...o, zone: "graveyard" } : o,
        ),
        events: [
            {
                kind: "mana",
                player: "you",
                source: land.slot,
                color: "white",
                amount: 1,
            },
            { kind: "resolutionPayment", player: "you", item },
            {
                kind: "zoneChange",
                slot: item.slot,
                from: "stack",
                to: "graveyard",
                reason: "resolve",
            },
        ],
    };
    const run = await motionFixture("tax-flow", [before, after]);
    await check(
        taxFlow,
        async (page) => {
            await page.clock.runFor(1000);
            const source = await page.locator(slotOf(item.slot)).boundingBox();
            await page
                .getByRole("button", { name: "Resolve", exact: true })
                .click();
            await until(page, ".mana-mote", 20);
            const end = await page
                .locator(".mana-mote")
                .first()
                .evaluate((el) => {
                    const a = el.getAnimations()[0];
                    const frame = a.effect.getKeyframes().at(-1);
                    const m = new DOMMatrix(frame.transform);
                    return { x: m.m41, y: m.m42 };
                });
            const scale =
                (await page.locator(".table").boundingBox()).width /
                (await page.locator(".table").evaluate((el) => el.offsetWidth));
            assert.ok(
                Math.abs(end.x * scale - (source.x + source.width / 2)) <
                    source.width / 2,
            );
            assert.equal(
                await page.locator(slotOf(item.slot)).getAttribute("data-zone"),
                "stack",
            );
            await page.clock.runFor(500);
            assert.equal(
                await page.locator(slotOf(item.slot)).getAttribute("data-zone"),
                "stack",
            );
            await page.clock.runFor(3000);
            await until(
                page,
                `${slotOf(item.slot)}[data-zone="graveyard"]`,
                50,
            );
            assert.equal(
                await page.locator(slotOf(item.slot)).getAttribute("data-zone"),
                "graveyard",
            );
        },
        { ...run, ready: slotOf(item.slot) },
    );
}

const beforeChooser = "mana finishes before the additional-cost chooser opens";
if (wanted(beforeChooser)) {
    const views = await load("cast-exile-choice");
    const at = views.findIndex(
        (v) => v.status.decision?.prompt.purpose === "exile-cost",
    );
    const spell = views[at].stack.at(-1).slot;
    await check(
        beforeChooser,
        async (page) => {
            await page.clock.runFor(1000);
            await dragFromHand(page, slotOf(spell), { x: 0, y: -320 });
            await page.mouse.up();
            await until(page, ".mana-mote", 20);
            assert.equal(
                await page.locator(".choice").count(),
                0,
                "payment is not covered by the chooser",
            );
            await until(page, ".choice", 20);
            const shown = await page.evaluate(() =>
                window.berylShown.map((s) => ({
                    at: s.at,
                    events: s.update.view.events.map((e) => e.kind),
                    purpose: s.update.view.status.decision?.prompt.purpose,
                })),
            );
            const mana = shown.find((s) => s.events.includes("mana"));
            const chooser = shown.find((s) => s.purpose === "exile-cost");
            assert.ok(
                chooser.at - mana.at >= 800,
                "payment finishes before selection",
            );
            const paid = await page.evaluate(
                () =>
                    window.berylShown
                        .flatMap((s) => s.update.view.events)
                        .filter((e) => e.kind === "mana").length,
            );
            for (const slot of views[at].players.you.graveyard.slice(1)) {
                await page.locator(`.choice-card[data-slot="${slot}"]`).click();
            }
            await page.getByRole("button", { name: /^Submit \d+$/ }).click();
            await page.clock.runFor(4000);
            assert.equal(
                await page.evaluate(
                    () =>
                        window.berylShown
                            .flatMap((s) => s.update.view.events)
                            .filter((e) => e.kind === "mana").length,
                ),
                paid,
                "confirm does not animate mana twice",
            );
        },
        { fixture: "cast-exile-choice", at, ready: slotOf(spell) },
    );
}

const pendingMana =
    "pending costs route mana without catching later standalone mana";
if (wanted(pendingMana)) {
    const views = await load("cast-exile-choice");
    const view = views.find(
        (v) => v.status.decision?.prompt.purpose === "exile-cost",
    );
    await check(
        pendingMana,
        async (page) => {
            const result = await page.evaluate(async (v) => {
                const { manaMotes } = await import("/src/game/ManaFlow.tsx");
                const { layout } = await import("/src/game/layout.ts");
                const { zonePiles } = await import("/src/game/zoneViews.ts");
                const { MANA_POOLS } = await import("/src/game/tableRows.ts");
                const { choiceOf } = await import("/src/game/choices.ts");
                const { beats } = await import("/src/game/beats.ts");
                const source = v.objects.find(
                    (o) => o.zone === "battlefield" && o.controller === "you",
                );
                const order = new Map();
                const places = layout(
                    v,
                    "held",
                    false,
                    {},
                    order,
                    new Set(),
                    zonePiles(v, order),
                    [],
                );
                v.events = [
                    {
                        kind: "mana",
                        player: "you",
                        source: source.slot,
                        color: "blue",
                        amount: 1,
                    },
                ];
                const pool = {
                    x: MANA_POOLS.you.left + 24,
                    y: MANA_POOLS.you.top + 17,
                };
                const paid = manaMotes(undefined, v, places, places)[0].to;
                const later = structuredClone(v);
                later.status = { kind: "playing" };
                later.stack = [{ ...v.stack[0], slot: 999 }];
                const unrelated = manaMotes(v, later, places, places)[0].to;
                const standalone = structuredClone(v);
                standalone.status.decision.kind = "priority";
                standalone.status.decision.cancellable = false;
                const floating = manaMotes(
                    undefined,
                    standalone,
                    places,
                    places,
                )[0].to;
                const ability = structuredClone(v);
                ability.stack = [
                    {
                        kind: "ability",
                        source: source.slot,
                        controller: "you",
                        ability: 0,
                        nth: 0,
                        granted: false,
                        triggered: false,
                        name: source.name,
                        targets: [],
                        x: 0,
                    },
                ];
                ability.status.decision.kind = "manaPayment";
                ability.status.decision.cancellable = false;
                const manual = manaMotes(undefined, ability, places, places)[0]
                    .to;
                const pooledBefore = structuredClone(v);
                pooledBefore.status.decision.kind = "manaPayment";
                pooledBefore.players.you.mana.blue = 3;
                const pooledAfter = structuredClone(v);
                pooledAfter.events = [];
                const pooled = beats(
                    { view: pooledBefore },
                    { view: pooledAfter },
                );
                const pooledMotes = manaMotes(
                    pooledBefore,
                    pooled[0].update.view,
                    places,
                    places,
                ).length;
                const token = structuredClone(
                    v.objects.find((o) => o.zone === "graveyard"),
                );
                token.slot = 999;
                token.token = true;
                v.objects.push(token);
                v.players.you.graveyard.push(999);
                return {
                    pool,
                    paid,
                    unrelated,
                    floating,
                    manual,
                    cards: choiceOf(v).cards,
                    pooled: pooled.map((b) => ({
                        kind: b.update.view.status.kind,
                        hold: b.hold,
                    })),
                    pooledMotes,
                };
            }, view);
            assert.notDeepEqual(
                result.paid,
                result.pool,
                "pending spell receives payment",
            );
            assert.deepEqual(
                result.unrelated,
                result.pool,
                "later spell cannot catch standalone mana",
            );
            assert.deepEqual(
                result.floating,
                result.pool,
                "priority mana floats",
            );
            assert.notDeepEqual(
                result.manual,
                result.pool,
                "manual ability receives payment",
            );
            assert.equal(
                result.pooled[0].kind,
                "playing",
                "pooled payment precedes chooser",
            );
            assert.ok(result.pooled[0].hold >= 800);
            assert.equal(
                result.pooledMotes,
                3,
                "floating mana flies before choice",
            );
            assert.ok(
                !result.cards.includes(999),
                "sacrificed token is not an exile-cost card",
            );
        },
        { fixture: "cast-exile-choice", at: views.length, ready: ".table" },
    );
}

const costOrder = "mana payment precedes graveyard exile departures";
if (wanted(costOrder)) {
    for (const seat of ["you", "opponent"]) {
        const source = structuredClone(oculusViews.slice(0, 2));
        if (seat === "opponent") {
            for (const v of source) {
                [v.players.you, v.players.opponent] = [
                    v.players.opponent,
                    v.players.you,
                ];
                for (const o of v.objects) {
                    o.controller = o.controller === "you" ? "opponent" : "you";
                    o.owner = o.owner === "you" ? "opponent" : "you";
                }
                for (const e of v.events) {
                    if (e.player)
                        e.player = e.player === "you" ? "opponent" : "you";
                    if (e.controller)
                        e.controller =
                            e.controller === "you" ? "opponent" : "you";
                }
            }
        }
        source[0].status.decision.passTo = { kind: "resolve" };
        const exiles = source[1].events.filter(
            (e) => e.kind === "zoneChange" && e.from === "graveyard",
        );
        const run = await motionFixture(`cost-order-${seat}`, source);
        await check(
            `${costOrder}: ${seat}`,
            async (page) => {
                await page.clock.runFor(1000);
                await page
                    .getByRole("button", { name: "Resolve", exact: true })
                    .click();
                await until(page, ".mana-mote", 20);
                for (const e of exiles)
                    assert.equal(
                        await page
                            .locator(slotOf(e.slot))
                            .getAttribute("data-zone"),
                        "graveyard",
                        "cards wait while payment flies",
                    );
                await page.clock.runFor(4000);
                for (const e of exiles)
                    assert.equal(
                        await page
                            .locator(slotOf(e.slot))
                            .getAttribute("data-zone"),
                        "exile",
                    );
            },
            { ...run, ready: slotOf(oculusSlot) },
        );
    }
}

const linkedReturn = "a linked return flies in sync with its departing source";
if (wanted(linkedReturn)) {
    const base = (await load("arrow-spell")).at(-1);
    const model = base.objects.find((o) => o.zone === "battlefield");
    for (const owner of ["you", "opponent"]) {
        const other = owner === "you" ? "opponent" : "you";
        const source = {
            ...model,
            slot: 800,
            name: "Aang's Iceberg",
            types: ["enchantment"],
            controller: owner,
            owner,
        };
        const captive = {
            ...model,
            slot: 801,
            name: "Staff of the Storyteller",
            types: ["artifact"],
            zone: "exile",
            exiledBy: source.slot,
            controller: other,
            owner: other,
        };
        const before = {
            ...base,
            objects: [...base.objects, source, captive],
            battlefield: [...base.battlefield, source.slot],
            events: [],
        };
        const after = {
            ...before,
            battlefield: [...base.battlefield, captive.slot],
            objects: [
                ...base.objects,
                { ...source, zone: "graveyard" },
                { ...captive, zone: "battlefield", exiledBy: null },
            ],
            players: {
                ...base.players,
                [owner]: {
                    ...base.players[owner],
                    graveyard: [...base.players[owner].graveyard, source.slot],
                },
            },
            events: [
                {
                    kind: "zoneChange",
                    slot: source.slot,
                    from: "battlefield",
                    to: "graveyard",
                    reason: "sacrifice",
                },
                {
                    kind: "zoneChange",
                    slot: captive.slot,
                    from: "exile",
                    to: "battlefield",
                    reason: "return",
                },
            ],
        };
        const run = await motionFixture(`linked-return-${owner}`, [
            before,
            after,
        ]);
        await check(
            `${linkedReturn}: ${owner}`,
            async (page) => {
                await page.clock.runFor(1000);
                const pair = [source.slot, captive.slot].map(slotOf);
                const positions = () =>
                    page.evaluate(
                        (selectors) =>
                            selectors.map((selector) => {
                                const el = document.querySelector(selector);
                                const transform = new DOMMatrix(
                                    getComputedStyle(el).transform,
                                );
                                return [transform.e, transform.f];
                            }),
                        pair,
                    );
                const start = await positions();
                await page.clock.pauseAt(Date.now() + 60_000);
                await page
                    .getByRole("button", { name: "Resolve", exact: true })
                    .evaluate((button) => button.click());
                await until(page, `${pair[0]}[data-zone="graveyard"]`, 20);
                assert.equal(
                    await page.locator(pair[1]).getAttribute("data-zone"),
                    "battlefield",
                );
                await page.clock.runFor(200);
                const mid = await positions();
                await page.clock.runFor(1000);
                const end = await positions();
                const progress = start.map(
                    ([x, y], i) =>
                        Math.hypot(mid[i][0] - x, mid[i][1] - y) /
                        Math.hypot(end[i][0] - x, end[i][1] - y),
                );
                assert.ok(
                    progress.every((p) => p > 0.05 && p < 0.95) &&
                        Math.abs(progress[0] - progress[1]) < 0.08,
                    `both cards should be equally far through their flights at 200ms: ${progress}`,
                );
            },
            { ...run, ready: `.card-el-beneath${slotOf(captive.slot)}` },
        );
    }
}

const exileStagger =
    "simultaneous exile departures span three hundred milliseconds";
if (wanted(exileStagger)) {
    const base = (await load("arrow-spell")).at(-1);
    const sample = base.objects.find((o) => o.zone === "battlefield");
    const cards = Array.from({ length: 6 }, (_, i) => ({
        ...sample,
        slot: 800 + i,
        zone: "graveyard",
        owner: "you",
        controller: "you",
        tapped: false,
    }));
    const before = {
        ...base,
        players: {
            ...base.players,
            you: { ...base.players.you, graveyard: cards.map((o) => o.slot) },
        },
        objects: [...base.objects, ...cards],
        events: [],
    };
    const after = {
        ...before,
        players: {
            ...before.players,
            you: { ...before.players.you, graveyard: [] },
        },
        objects: before.objects.map((o) =>
            o.slot >= 800 ? { ...o, zone: "exile" } : o,
        ),
        events: cards.map((o) => ({
            kind: "zoneChange",
            slot: o.slot,
            from: "graveyard",
            to: "exile",
            reason: "exile",
        })),
    };
    const run = await motionFixture("exile-stagger", [before, after]);
    await check(
        exileStagger,
        async (page) => {
            await page.clock.runFor(1000);
            const first = page.locator(slotOf(800));
            const last = page.locator(slotOf(805));
            const a = center(await first.boundingBox());
            const b = center(await last.boundingBox());
            await page.clock.pauseAt(Date.now() + 60_000);
            await page
                .getByRole("button", { name: "Resolve", exact: true })
                .evaluate((button) => button.click());
            await until(page, `${slotOf(800)}[data-zone="exile"]`, 20);
            await page.clock.runFor(150);
            const c = center(await first.boundingBox());
            const d = center(await last.boundingBox());
            assert.ok(
                Math.hypot(c.x - a.x, c.y - a.y) > 2,
                "first card has left",
            );
            assert.ok(
                Math.hypot(d.x - b.x, d.y - b.y) < 2,
                "last waits for its stagger",
            );
            await page.clock.runFor(300);
            const e = center(await last.boundingBox());
            assert.ok(
                Math.hypot(e.x - b.x, e.y - b.y) > 2,
                "last leaves within 300ms",
            );
            await page.clock.runFor(1000);
            assert.equal(await page.locator('[data-zone="exile"]').count(), 6);
        },
        { ...run, ready: slotOf(805) },
    );
}

const sweepTogether =
    "a sweeper's creatures, token and face-down card leave together";
if (wanted(sweepTogether)) {
    // An overloaded Damn: one resolution destroys a creature from each seat, a token
    // (which ceases to exist in the graveyard) and the opponent's face-down card (which
    // turns face up first). They leave in one move, not one wave per kind (by-f0ax).
    const base = (await load("arrow-spell")).at(-1);
    const [trinket, counter] = base.stack;
    const damn = { ...counter, targets: [] };
    const sample = base.objects.find((o) => o.slot === trinket.slot);
    const creature = (slot, side, extra) => ({
        ...sample,
        slot,
        zone: "battlefield",
        owner: side,
        controller: side,
        types: ["creature"],
        pt: { power: 2, toughness: 2 },
        ...extra,
    });
    const swept = [
        creature(800, "you", { name: "Snapcaster Mage" }),
        creature(801, "opponent", { name: "Abhorrent Oculus" }),
        creature(802, "you", { name: "Spirit Token", token: true }),
        creature(803, "opponent", { name: null, facing: "down" }),
    ];
    const slots = swept.map((o) => o.slot);
    const before = {
        ...base,
        stack: [trinket, damn],
        battlefield: [...base.battlefield, ...slots],
        objects: [
            ...base.objects.map((o) =>
                o.slot === damn.slot ? { ...o, name: "Damn" } : o,
            ),
            ...swept,
        ],
        events: [],
    };
    const grave = (side, ...more) => [
        ...before.players[side].graveyard,
        ...more,
    ];
    const after = {
        ...before,
        stack: [trinket],
        battlefield: base.battlefield,
        players: {
            you: { ...before.players.you, graveyard: grave("you", 800) },
            opponent: {
                ...before.players.opponent,
                graveyard: grave("opponent", 801, 803, damn.slot),
            },
        },
        objects: before.objects.flatMap((o) =>
            o.slot === 802
                ? []
                : o.slot === 803
                  ? [
                        {
                            ...o,
                            zone: "graveyard",
                            facing: "up",
                            name: "Brazen Borrower",
                        },
                    ]
                  : o.slot === damn.slot || slots.includes(o.slot)
                    ? [{ ...o, zone: "graveyard" }]
                    : [o],
        ),
        events: [
            ...slots.map((slot) => ({
                kind: "zoneChange",
                slot,
                from: "battlefield",
                to: "graveyard",
                reason: "destroy",
            })),
            {
                kind: "zoneChange",
                slot: damn.slot,
                from: "stack",
                to: "graveyard",
                reason: "resolve",
            },
        ],
    };
    const run = await motionFixture("sweep-together", [before, after]);
    await check(
        sweepTogether,
        async (page) => {
            await page.clock.runFor(1000);
            await page.clock.pauseAt(Date.now() + 60_000);
            await page
                .getByRole("button", { name: "Resolve", exact: true })
                .evaluate((button) => button.click());
            const left = new Map();
            const zones = new Map(slots.map((slot) => [slot, new Set()]));
            let damnLeft;
            for (let t = 0; t <= 6000; t += 50) {
                for (const slot of slots) {
                    const cards = page.locator(`.card-layer ${slotOf(slot)}`);
                    const count = await cards.count();
                    assert.ok(count <= 1, `one card for ${slot} at ${t}ms`);
                    const zone =
                        count &&
                        (await cards.first().getAttribute("data-zone"));
                    if (zone) zones.get(slot).add(zone);
                    if (zone !== "battlefield" && !left.has(slot))
                        left.set(slot, t);
                }
                const spell = await page
                    .locator(`.card-layer ${slotOf(damn.slot)}`)
                    .getAttribute("data-zone");
                if (spell === "graveyard" && damnLeft === undefined)
                    damnLeft = t;
                await page.clock.runFor(50);
            }
            const starts = slots.map((slot) => left.get(slot));
            assert.ok(
                starts.every((t) => t !== undefined),
                `every permanent leaves the battlefield: ${starts}`,
            );
            assert.ok(
                Math.max(...starts) - Math.min(...starts) <= 350,
                `the swept permanents leave together, not in waves: ${starts}`,
            );
            assert.ok(
                damnLeft > Math.max(...starts),
                `Damn stays on the stack until its creatures have gone: ${damnLeft}`,
            );
            assert.ok(
                zones.get(802).has("graveyard"),
                "the token reaches the graveyard before it ceases to exist",
            );
            assert.equal(
                await page.locator(`.card-layer ${slotOf(802)}`).count(),
                0,
                "the token ceases to exist",
            );
            assert.equal(
                await page
                    .locator(`.card-layer ${slotOf(803)}`)
                    .getAttribute("data-zone"),
                "graveyard",
            );
            // Sequential departures stay apart: a token sacrificed for a cost leaves
            // in a beat of its own, before the creature an effect then destroys.
            const apart = await page.evaluate(
                async ({ before, after }) => {
                    const { beats } = await import("/src/game/beats.ts");
                    const [, , token, creature] = after.events;
                    const sequential = {
                        ...after,
                        events: [{ ...token, reason: "sacrifice" }, creature],
                    };
                    return beats(
                        { session: 1, view: before },
                        { session: 1, view: sequential },
                    ).map((beat) =>
                        beat.update.view.events.flatMap((e) =>
                            e.kind === "zoneChange" ? [e.slot] : [],
                        ),
                    );
                },
                { before, after },
            );
            assert.ok(
                apart.some(
                    (slots) => slots.includes(802) && !slots.includes(803),
                ),
                `the sacrificed token leaves before the destroyed creature: ${JSON.stringify(apart)}`,
            );
        },
        { ...run, ready: slotOf(803) },
    );
}

const manifestFlight =
    "manifest cards travel directly from their chooser to their zones";
// Opens for every check below, whose titles all start so.
if (wanted("manifest cards")) {
    const base = (await load("arrow-spell")).at(-1);
    const model = base.objects.find((o) => o.zone === "battlefield");
    const cards = [800, 801].map((slot, i) => ({
        ...model,
        slot,
        zone: "library",
        owner: "you",
        controller: "you",
        facing: "up",
        name: i === 0 ? "Snapcaster Mage" : "Island",
        tapped: false,
    }));
    const candidates = cards.map((o) => ({
        label: o.name,
        option: { kind: "object", slot: o.slot },
        payments: [],
        blocks: [],
        nth: null,
        targets: [],
    }));
    const before = {
        ...base,
        objects: [...base.objects, ...cards],
        players: {
            ...base.players,
            you: {
                ...base.players.you,
                library: [...base.players.you.library, 801, 800],
            },
        },
        status: {
            kind: "deciding",
            decision: {
                ...base.status.decision,
                kind: "chooseObject",
                prompt: asking(
                    base.status.decision.prompt,
                    "pick",
                    "manifest-dread",
                    {
                        title: "Manifest one of them.",
                        words: { confirm: "Submit {picked}" },
                        count: 2,
                        from: { player: "you", zone: "library" },
                    },
                ),
                candidates,
            },
        },
        events: [],
    };
    const after = {
        ...before,
        objects: before.objects.map((o) =>
            o.slot === 800
                ? {
                      ...o,
                      zone: "battlefield",
                      facing: "down",
                      types: ["creature"],
                      pt: { power: 2, toughness: 2 },
                  }
                : o.slot === 801
                  ? { ...o, zone: "graveyard" }
                  : o,
        ),
        battlefield: [...before.battlefield, 800],
        players: {
            ...before.players,
            you: {
                ...before.players.you,
                library: base.players.you.library,
                graveyard: [...base.players.you.graveyard, 801],
            },
        },
        status: base.status,
        events: [
            {
                kind: "zoneChange",
                slot: 800,
                from: "library",
                to: "battlefield",
                reason: "effect",
            },
            {
                kind: "zoneChange",
                slot: 801,
                from: "library",
                to: "graveyard",
                reason: "effect",
            },
        ],
    };
    const exileBefore = {
        ...before,
        objects: [
            ...base.objects,
            ...cards.map((o) => ({ ...o, zone: "graveyard" })),
        ],
        players: {
            ...base.players,
            you: {
                ...base.players.you,
                graveyard: [...base.players.you.graveyard, 800, 801],
            },
        },
        status: {
            ...before.status,
            decision: {
                ...before.status.decision,
                kind: "chooseObject",
                prompt: asking(
                    before.status.decision.prompt,
                    "pick",
                    "exile-cost",
                    {
                        title: "Exile a card.",
                        words: { confirm: "Submit {picked}" },
                        most: 1,
                        from: { player: "you", zone: "graveyard" },
                    },
                ),
            },
        },
    };
    const exileAfter = {
        ...exileBefore,
        objects: exileBefore.objects.map((o) =>
            o.slot === 800 ? { ...o, zone: "exile" } : o,
        ),
        players: {
            ...exileBefore.players,
            you: {
                ...exileBefore.players.you,
                graveyard: [...base.players.you.graveyard, 801],
            },
        },
        status: base.status,
        events: [
            {
                kind: "zoneChange",
                slot: 800,
                from: "graveyard",
                to: "exile",
                reason: "cost",
            },
        ],
    };
    const exileRun = await motionFixture("choice-exile-flight", [
        exileBefore,
        exileAfter,
    ]);
    await check(
        "manifest cards and exile choices land even at the same pile height",
        async (page) => {
            await page.clock.runFor(1000);
            const pile = await page
                .locator(`.card-layer ${slotOf(801)}`)
                .boundingBox();
            await page.locator('.choice-card[data-slot="800"]').click();
            await page
                .getByRole("button", { name: "Submit 1", exact: true })
                .click();
            await until(page, `${slotOf(800)}[data-zone="exile"]`, 20);
            await page.clock.runFor(1800);
            const landed = await page
                .locator(`.card-layer ${slotOf(800)}`)
                .boundingBox();
            assert.ok(
                Math.abs(landed.y - pile.y) < 3,
                "card reaches the pile height",
            );
            assert.ok(
                Math.abs(landed.height - pile.height) < 3,
                "card shrinks to pile size",
            );
        },
        { ...exileRun, ready: '.choice-card[data-slot="800"]' },
    );
    const run = await motionFixture("manifest-flight", [before, after]);
    await check(
        "manifest cards travel without carrying flights into another game",
        async (page) => {
            const isolated = await page.evaluate(async () => {
                const { takeMovedCard } =
                    await import("/src/game/choiceFlight.ts");
                const stage = document.createElement("div");
                stage.className = "stage";
                const other = document.createElement("div");
                stage.append(other);
                document.body.append(stage);
                try {
                    return takeMovedCard(other, 800) === undefined;
                } finally {
                    stage.remove();
                }
            });
            assert.ok(isolated, "another stage cannot take this tray's card");
        },
        { ...run, ready: '.choice-card[data-slot="800"]' },
    );
    await check(
        manifestFlight,
        async (page) => {
            for (const slot of [800, 801]) {
                const table = page.locator(`.card-layer ${slotOf(slot)}`);
                assert.equal(
                    await table.isVisible(),
                    false,
                    "the tray owns the card",
                );
            }
            // The tray's cards come out of the library before they can be picked.
            await page.clock.runFor(1000);
            await page.locator('.choice-card[data-slot="800"]').click();
            await page.clock.runFor(700);
            const origin = center(
                await page
                    .locator('.choice-card[data-slot="800"]')
                    .boundingBox(),
            );
            // The flight's first frame, recorded in the page: a position read after the
            // card has reached the battlefield may come frames into its flight on a busy
            // machine.
            await startRecording(page);
            await page
                .getByRole("button", { name: "Submit 1", exact: true })
                .click();
            await until(page, `${slotOf(800)}[data-zone="battlefield"]`, 20);
            // A few frames of the flight: the paused clock draws none until it is stepped.
            await page.clock.runFor(48);
            const start = posesOf(await page.evaluate(collect, 0), 800).find(
                (c) => c.zone,
            );
            assert.ok(
                start &&
                    Math.hypot(origin.x - start.x, origin.y - start.y) < 90,
                "card starts in chooser, not library",
            );
            await page.clock.runFor(1800);
            assert.equal(
                await page
                    .locator(`.card-layer ${slotOf(800)} .card-face-down`)
                    .count(),
                1,
            );
            assert.equal(
                await page.locator('.choice-card[data-slot="800"]').count(),
                0,
            );
            await until(page, `${slotOf(801)}[data-zone="graveyard"]`, 50);
        },
        { ...run, ready: '.choice-card[data-slot="800"]' },
    );
}

// A single recorded update casts and resolves Staff, makes the Spirit, and
// resolves the story-counter trigger. Older reports have no resolution marker.
const staffChain = "Staff's Spirit and story counter triggers play in sequence";
if (wanted(staffChain)) {
    const base = (await load("target-up-to-one")).at(-1);
    const staff = {
        ...base.objects.find((o) => o.name === "Staff of the Storyteller"),
        slot: 800,
        counters: [],
    };
    const token = {
        ...staff,
        slot: 801,
        name: "Spirit Token",
        token: true,
        types: ["creature"],
        power: 1,
        toughness: 1,
    };
    for (const controller of ["you", "opponent"]) {
        const source = { ...staff, controller, owner: controller };
        const spirit = { ...token, controller, owner: controller };
        const spell = {
            kind: "spell",
            slot: staff.slot,
            x: 0,
            paid: [],
            targets: [],
        };
        const enters = {
            kind: "ability",
            source: staff.slot,
            name: staff.name,
            controller,
            triggered: true,
            ability: 0,
            nth: 0,
            granted: false,
            targets: [],
            x: 0,
        };
        const counter = { ...enters, ability: 1, nth: 1 };
        const before = {
            ...turnViews[mainHalt],
            stack: [],
            events: [],
            objects: [
                ...turnViews[mainHalt].objects,
                { ...source, zone: "hand" },
            ],
        };
        const finished =
            controller === "opponent" ? [{ kind: "abilityFinished" }] : [];
        const after = {
            ...before,
            objects: [
                ...before.objects.filter((o) => o.slot !== staff.slot),
                { ...source, counters: [{ kind: "story", count: 1 }] },
                spirit,
            ],
            battlefield: [...before.battlefield, staff.slot, token.slot],
            events: [
                {
                    kind: "zoneChange",
                    slot: staff.slot,
                    from: "hand",
                    to: "stack",
                    reason: "cast",
                },
                { kind: "cast", slot: staff.slot, controller, item: spell },
                {
                    kind: "zoneChange",
                    slot: staff.slot,
                    from: "stack",
                    to: "battlefield",
                    reason: "resolve",
                },
                { kind: "ability", item: enters, loyalty: null },
                {
                    kind: "zoneChange",
                    slot: token.slot,
                    from: "nowhere",
                    to: "battlefield",
                    reason: "created",
                },
                { kind: "tokensCreated", controller, tokens: [token.slot] },
                ...finished,
                { kind: "ability", item: counter, loyalty: null },
                {
                    kind: "countersAdded",
                    slot: staff.slot,
                    counter: "story",
                    amount: 1,
                },
                ...finished,
            ],
        };
        const run = await motionFixture(`staff-trigger-chain-${controller}`, [
            before,
            after,
        ]);
        await check(
            `${staffChain} (${controller})`,
            async (page) => {
                await page.keyboard.press("Space");
                const abilities = page.locator(".stack-ability");
                await abilities.first().waitFor();
                assert.match(
                    await abilities.innerText(),
                    /white Spirit creature token/,
                );
                await abilities
                    .filter({ hasText: "put a story counter" })
                    .waitFor();
                assert.equal(await abilities.count(), 1);
                assert.match(
                    await abilities.innerText(),
                    /put a story counter/,
                );
                assert.equal(await page.locator(slotOf(token.slot)).count(), 1);
                const storyCount = () =>
                    page.evaluate(
                        (slot) =>
                            window.berylShown
                                .at(-1)
                                .update.view.objects.find(
                                    (o) => o.slot === slot,
                                )
                                .counters.find((c) => c.kind === "story")
                                ?.count,
                        staff.slot,
                    );
                assert.equal(await storyCount(), undefined);
                await abilities.waitFor({ state: "detached" });
                assert.equal(await storyCount(), 1);
                assert.equal(await abilities.count(), 0);
                assert.equal(await page.locator(slotOf(staff.slot)).count(), 1);
            },
            // Off: the triggers fade as Web Animations, on the document's timeline, which a
            // fake clock does not drive: paused, the Spirit's trigger never leaves.
            { ...run, clock: "off", ready: slotOf(staff.slot) },
        );
    }
    // Multiple real items sharing a source stay on the stack. The covered
    // ability's title exposes its own rules text instead of just its source name.
    const enters = {
        kind: "ability",
        source: staff.slot,
        name: staff.name,
        controller: "you",
        triggered: true,
        ability: 0,
        nth: 0,
        granted: false,
        targets: [],
        x: 0,
    };
    const counter = { ...enters, ability: 1, nth: 1 };
    const field = {
        ...turnViews[mainHalt],
        battlefield: [...turnViews[mainHalt].battlefield, staff.slot],
        objects: [...turnViews[mainHalt].objects, staff],
        stack: [],
        events: [],
    };
    const run = await motionFixture("staff-concurrent-triggers", [
        field,
        {
            ...field,
            stack: [enters, counter],
            events: [enters, counter].map((item) => ({
                kind: "ability",
                item,
                loyalty: null,
            })),
        },
    ]);
    await check(
        `${staffChain} (concurrent stack neighbor)`,
        async (page) => {
            await page.keyboard.press("Space");
            await page.locator("#stack-1").waitFor();
            const abilities = page.locator(".stack-ability");
            assert.equal(await abilities.count(), 2);
            assert.match(
                await abilities.first().getAttribute("title"),
                /white Spirit creature token/,
            );
            assert.match(
                await abilities.last().innerText(),
                /put a story counter/,
            );
        },
        { ...run, ready: slotOf(staff.slot) },
    );
}

// `marks` and `marks-damage` end on boards where a permanent bearing one mark and no other
// stands beside a twin of the same name bearing none: +1/+1 counters, summoning sickness and
// story or time counters in one, damage in the other.
{
    const counted = (kinds) => (o) =>
        o.counters.some((c) => kinds.includes(c.kind));
    const creature = (o) => o.types.includes("creature");
    const MARKS = [
        (o) => creature(o) && counted(["plusOne"])(o),
        (o) => creature(o) && o.damage > 0,
        (o) => creature(o) && o.summoningSick,
        counted(["story", "time"]),
    ];
    const [plusOne, damaged] = MARKS;
    /** A permanent bearing `mark` alone in fixture `views`' last view, and a bare twin. */
    const pair = (views, mark) => {
        const field = views
            .at(-1)
            .objects.filter((o) => o.zone === "battlefield" && o.name !== null);
        const bare = (o) =>
            !MARKS.some((m) => m(o)) &&
            o.counters.every((c) => c.kind === "loyalty");
        for (const o of field) {
            if (!mark(o) || MARKS.some((m) => m !== mark && m(o))) continue;
            const twin = field.find(
                (t) =>
                    t.slot !== o.slot &&
                    t.name === o.name &&
                    t.token === o.token &&
                    bare(t),
            );
            if (twin) return [o, twin];
        }
        assert.fail("the fixture has a marked permanent beside a bare twin");
    };
    const el = (page, o) => page.locator(`.card-el[data-slot="${o.slot}"]`);
    const marks = await load("marks");
    const hurt = await load("marks-damage");
    await check(
        "marks: damage lowers the toughness drawn, in red",
        async (page) => {
            await page.clock.runFor(2000);
            const [o, whole] = pair(hurt, damaged);
            const toughness = el(page, o).locator(".card-pt b").last();
            assert.equal(
                await toughness.textContent(),
                String(o.pt.toughness - o.damage),
            );
            assert.match(await toughness.getAttribute("class"), /stat-down/);
            assert.equal(
                await el(page, whole).locator(".card-pt b.stat-down").count(),
                0,
            );
            // Damage past lethal, as the table may show for a moment before the creature dies,
            // draws no toughness below 0, by the page's own module.
            const drawn = await page.evaluate(async (o) => {
                const { frameFace } = await import("/src/game/faces.ts");
                const face = {
                    name: o.name,
                    type_line: "Creature",
                    colors: [],
                    power: String(o.pt.power),
                    toughness: String(o.pt.toughness),
                };
                const lethal = { ...o, damage: o.pt.toughness + 3 };
                return frameFace(lethal, { face }).toughness;
            }, o);
            assert.equal(drawn, "0", "damage past lethal draws toughness 0");
        },
        { fixture: "marks-damage", at: hurt.length, ready: ".card-pt" },
    );
    await check(
        "marks: a countered creature's inspector shows its current P/T",
        async (page) => {
            await page.clock.runFor(2000);
            const [o] = pair(marks, plusOne);
            await page.hover(`.card-el[data-slot="${o.slot}"]`);
            const pt = page.locator(".inspect .inspect-pt");
            await pt.waitFor();
            const text = await pt.textContent();
            assert.ok(
                text.endsWith(`${o.pt.power}/${o.pt.toughness}`),
                `${o.name}'s inspector reads ${text}`,
            );
            await page
                .locator(".inspect .inspect-marks .mark-plusOne")
                .waitFor();
        },
        { fixture: "marks", at: marks.length, ready: ".card-counters" },
    );
}

// A card the opponent's Elite Spellbinder exiled from the human's hand, which they may cast from
// exile (`exile-cast`): it waits in their hand fan, outlined apart, and a click there casts it.
// The human also knows a card in the opponent's hand, one their Sink into Stupor returned there.
const exileViews = await load("exile-cast");
const exileView = exileViews.at(-1);
const exileCast = exileView.status.decision.candidates.findIndex(
    (c) =>
        c.option.kind === "cast" &&
        exileView.objects.find((o) => o.slot === c.option.slot)?.zone ===
            "exile",
);
assert.ok(exileCast >= 0, "exile-cast offers a cast from exile");
const exiledSlot = exileView.status.decision.candidates[exileCast].option.slot;
const yourExile = '[data-pile="exile"][data-side="you"]';
const exileRun = {
    fixture: "exile-cast",
    at: exileViews.length,
    ready: `${slotOf(exiledSlot)}.card-el-hand`,
};

await check(
    "a card castable from exile waits in the hand fan and is cast from there",
    async (page) => {
        await page.clock.runFor(2000);
        assert.ok(
            await page.$(`${slotOf(exiledSlot)} .card-elsewhere`),
            "it is outlined apart from the hand's own cards",
        );
        assert.equal(await page.$(yourExile), null, "its exile pile is gone");
        await page.click(slotOf(exiledSlot));
        // With nothing to choose, the pick casts it at once.
        assert.deepEqual(
            await chosen(page),
            [exileCast],
            `chose "${exileView.status.decision.candidates[exileCast].label}"`,
        );
    },
    exileRun,
);

const theirHand = exileView.objects.filter(
    (o) => o.zone === "hand" && o.owner === "opponent",
);
const knownCard = theirHand.find((o) => o.name !== null);
assert.ok(knownCard, "exile-cast: the human knows a card in their hand");
assert.ok(
    theirHand.some((o) => o.name === null),
    "exile-cast: the opponent holds a card the human does not know",
);

await check(
    "a card you know in the opponent's hand lies face up",
    async (page) => {
        await page.clock.runFor(2000);
        for (const o of theirHand) {
            const back = await page.$(`${slotOf(o.slot)} .card-back`);
            assert.equal(
                !back,
                o.name !== null,
                `slot ${o.slot} (${o.name ?? "unknown"}) is ` +
                    (back ? "a back" : "face up"),
            );
        }
        // It peeks in from the top edge; hovering what shows of it shows it whole.
        const box = await page.locator(slotOf(knownCard.slot)).boundingBox();
        await page.mouse.move(box.x + box.width / 2, box.y + box.height - 12);
        await page.getByLabel(`${knownCard.name}, inspected`).waitFor();
    },
    exileRun,
);

// Tokens, piles of identical permanents and stolen permanents: the `tokens` fixture holds two
// identical Map tokens of yours, whose ability the engine offers through one of them only
// (architecture §7.2), a token changed by an Aura, and a permanent its owner doesn't control.
const tokenViews = await load("tokens");
const tokenView = tokenViews.at(-1);
const tokenField = tokenView.objects.filter((o) => o.zone === "battlefield");
const mapAct = tokenView.status.decision.candidates.findIndex(
    (c) =>
        c.option.kind === "activate" &&
        tokenField.find((o) => o.slot === c.option.slot)?.token,
);
assert.ok(mapAct >= 0, "tokens: a token's ability is offered");
const offeredToken = tokenField.find(
    (o) => o.slot === tokenView.status.decision.candidates[mapAct].option.slot,
);
const tokenPile = [
    offeredToken,
    ...tokenField.filter((o) => twins(offeredToken, o)),
];
assert.ok(tokenPile.length > 1, "tokens: the offered token has a twin");
const stolen = tokenField.filter((o) => o.owner !== o.controller);
assert.ok(stolen.length > 0, "tokens: a permanent is stolen");
assert.ok(
    tokenField.some(
        (o) =>
            o.token &&
            o.name !== offeredToken.name &&
            tokenField.some((a) => a.attachedTo === o.slot),
    ),
    "tokens: a token wears an Aura",
);
const tokenRun = {
    fixture: "tokens",
    at: tokenViews.length,
    ready: slotOf(offeredToken.slot),
};

await check(
    "tokens wear their frame and stolen permanents their edge",
    async (page) => {
        await page.clock.runFor(2000);
        for (const o of tokenField) {
            if (o.types.includes("land")) continue;
            const marks = await page
                .locator(`${slotOf(o.slot)} .card`)
                .evaluate((e) => [
                    e.classList.contains("card-token"),
                    !!e.querySelector(".card-stolen-edge"),
                ]);
            assert.deepEqual(
                marks,
                [o.token, o.owner !== o.controller],
                `${o.name} in slot ${o.slot}: [token, stolen]`,
            );
        }
        // The inspector names the owner.
        const thief = stolen[0];
        await page.hover(slotOf(thief.slot));
        await page
            .getByLabel(`${thief.name}, inspected`)
            .getByText(`Owner: ${thief.owner === "you" ? "You" : "Opponent"}`)
            .waitFor();
    },
    tokenRun,
);

await check(
    "clicking a pile's top card acts on the one the engine offers",
    async (page) => {
        await page.clock.runFor(2000);
        // One card of the pile wears its count, and it is the one on top.
        const counted = [];
        for (const o of tokenPile) {
            const badge = await page.$(`${slotOf(o.slot)} .card-count`);
            if (badge) {
                assert.equal(
                    await badge.textContent(),
                    `×${tokenPile.length}`,
                    "the count",
                );
                counted.push(o.slot);
            }
        }
        assert.equal(counted.length, 1, "one card of the pile is counted");
        const top = counted[0];
        // The engine offers the lowest slot of twins, which lies on top, so what a choice takes
        // from a pile leaves it from the top.
        assert.equal(top, offeredToken.slot, "the offered token is on top");
        const box = await page.locator(slotOf(top)).boundingBox();
        const at = center(box);
        const hit = await page.evaluate(
            ({ x, y }) =>
                document.elementFromPoint(x, y)?.closest("[data-slot]")?.dataset
                    .slot,
            at,
        );
        assert.equal(Number(hit), top, "the counted card is on top");
        // The token's one use acts on the click.
        await page.mouse.click(at.x, at.y);
        assert.deepEqual(await settled(page, 1), [mapAct], "the ability");
    },
    tokenRun,
);

await check(
    "a pile splits on any difference, and on a target",
    async (page) => {
        // The board's layout of variants of the fixture's view, by the page's own module.
        const counts = await page.evaluate(
            async ({ view, map }) => {
                const { layout } = await import("/src/game/layout.ts");
                const { zonePiles } = await import("/src/game/zoneViews.ts");
                const order = new Map();
                const borrowed = new Set();
                // Each card of the pile of `map`'s copies that wears a count, with the count.
                const counted = (v) =>
                    [
                        ...layout(
                            v,
                            "held",
                            false,
                            {},
                            order,
                            borrowed,
                            zonePiles(v, order),
                            [],
                        ).values(),
                    ]
                        .filter((p) => p.count !== undefined)
                        .filter((p) =>
                            v.objects.some(
                                (o) => o.slot === p.slot && o.name === map.name,
                            ),
                        )
                        .map((p) => p.count);
                const vary = (change) => {
                    const v = structuredClone(view);
                    const copy = { ...structuredClone(map), slot: 250 };
                    v.objects.push(copy);
                    v.battlefield.push(copy.slot);
                    change(
                        v,
                        v.objects.find((o) => o.slot === copy.slot),
                    );
                    return counted(v);
                };
                const aura = view.objects.find(
                    (o) => o.zone === "battlefield" && o.attachedTo !== null,
                ).slot;
                return {
                    fixture: counted(view),
                    third: vary(() => {}),
                    types: vary((_, o) => o.types.push("creature")),
                    owner: vary((_, o) => (o.owner = "opponent")),
                    tapped: vary((_, o) => (o.tapped = true)),
                    enchanted: vary((v, o) => {
                        v.objects.find((a) => a.slot === aura).attachedTo =
                            o.slot;
                    }),
                    targeted: vary((v, o) =>
                        v.stack.push({
                            kind: "ability",
                            source: null,
                            ability: 0,
                            granted: false,
                            name: "Test",
                            triggered: true,
                            nth: 0,
                            controller: "opponent",
                            targets: [{ kind: "object", slot: o.slot }],
                            x: 0,
                        }),
                    ),
                };
            },
            { view: tokenView, map: offeredToken },
        );
        assert.deepEqual(counts, {
            fixture: [2],
            third: [3],
            types: [2],
            owner: [2],
            tapped: [2],
            enchanted: [2],
            targeted: [2],
        });
    },
    tokenRun,
);

// Every object an open decision offers something for has an element a player can see and point
// at: the card on the table or in a choice's tray, a twin the engine offers it through, or the
// button of the pile it lies in. Every exiled card can be seen the same way, if not pointed at
// under a screen over the table. Checked at the last decision of every fixture, with motion
// reduced so that nothing is caught halfway through fading in.
for (const fixture of recorded) {
    const fixtureViews = await load(fixture);
    const last = fixtureViews.findLastIndex(
        (v) => v.status.kind === "deciding",
    );
    if (last < 0) continue;
    const view = fixtureViews[last];
    const byId = new Map(view.objects.map((o) => [o.slot, o]));
    const describe = (slot, why) =>
        `slot ${slot} (${byId.get(slot)?.name ?? "unknown"}, ${why})`;
    const wanted = [];
    for (const c of view.status.decision.candidates) {
        const slot = offeredSlot(c.option);
        if (slot === undefined) continue;
        const o = byId.get(slot);
        const twinSlots = o
            ? view.objects.filter((t) => twins(o, t)).map((t) => t.slot)
            : [];
        wanted.push({
            what: describe(slot, `${c.option.kind} "${c.label}"`),
            slots: [slot, ...twinSlots],
            point: true,
        });
    }
    for (const o of view.objects) {
        if (o.zone === "exile") {
            wanted.push({
                what: describe(o.slot, "exiled"),
                slots: [o.slot],
                point: false,
            });
        }
    }
    const owners = Object.fromEntries(
        view.objects.map((o) => [o.slot, o.owner]),
    );
    await check(
        `${fixture}: what the decision offers and what is exiled can be seen`,
        async (page) => {
            await page.clock.runFor(5000);
            const look = () =>
                page.evaluate(
                    ({ wanted, owners }) => {
                        const seen = (el, point) => {
                            for (let e = el; e; e = e.parentElement) {
                                const style = getComputedStyle(e);
                                if (Number(style.opacity) < 0.5) return false;
                                if (style.visibility === "hidden") return false;
                            }
                            const r = el.getBoundingClientRect();
                            if (r.width < 1 || r.height < 1) return false;
                            // Some part of it lies on the screen, and, to be pointed at, is the
                            // topmost thing there.
                            for (let i = 1; i < 10; i++) {
                                for (let j = 1; j < 10; j++) {
                                    const x = r.left + (r.width * i) / 10;
                                    const y = r.top + (r.height * j) / 10;
                                    if (x < 0 || y < 0) continue;
                                    if (x >= innerWidth || y >= innerHeight)
                                        continue;
                                    if (!point) return true;
                                    const hit = document.elementFromPoint(x, y);
                                    if (hit && el.contains(hit)) return true;
                                }
                            }
                            return false;
                        };
                        const shown = (slot, point) =>
                            [
                                ...document.querySelectorAll(
                                    `[data-slot="${slot}"]`,
                                ),
                            ].some((el) => {
                                if (!el.classList.contains("card-el-pile")) {
                                    return seen(el, point);
                                }
                                const pile = document.querySelector(
                                    `[data-pile="${el.dataset.zone}"]` +
                                        `[data-side="${owners[slot]}"]`,
                                );
                                return !!pile && seen(pile, point);
                            });
                        return wanted
                            .filter(
                                (w) => !w.slots.some((s) => shown(s, w.point)),
                            )
                            .map((w) => w.what);
                    },
                    { wanted, owners },
                );
            // Fades run on the compositor's clock, not the page's fake one, and a loaded
            // machine slows them: look until all show, the deadline only a hang guard.
            let unseen = await look();
            for (
                const end = Date.now() + 20_000;
                unseen.length && Date.now() < end;
            ) {
                await page.waitForTimeout(100);
                unseen = await look();
            }
            assert.deepEqual(unseen, [], "every one can be seen");
        },
        // The board draws its first view, the whole fixture's, once the fixture has loaded.
        {
            fixture,
            at: last + 1,
            ready: ".table",
            reducedMotion: "reduce",
        },
    );
}

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

// `mana-floating` stops at the human's priority in their main phase with Mana Crypt's {C}
// unspent, then plays on past their pass, which ends the step and empties the pool.
const floatingViews = await load("mana-floating");
const floatingAt =
    floatingViews.findIndex(
        (v) =>
            v.status.kind === "deciding" &&
            Object.values(v.players.you.mana).some((n) => n > 0),
    ) + 1;

/** The words standing on the table, outside cards, stack items, open prompts and panels and
 * the game log (the record a player reads back, which grows with the game): what a player
 * must read past to see the board. Text hidden or transparent, such as a closed tip, does not
 * count. */
const tableWords = (page) =>
    page.evaluate(() => {
        const SKIP = [
            ".card-el",
            ".card-slot",
            ".card-back",
            ".stack-item",
            ".prompt",
            ".action-menu",
            ".choice",
            ".zone-viewer",
            ".inspect",
            ".settings",
            ".game-log",
        ].join(", ");
        const shown = (el) => {
            if (getComputedStyle(el).visibility !== "visible") return false;
            for (let a = el; a; a = a.parentElement) {
                if (parseFloat(getComputedStyle(a).opacity) === 0) return false;
            }
            return el.getClientRects().length > 0;
        };
        const words = [];
        const table = document.querySelector(".table");
        const walk = document.createTreeWalker(table, NodeFilter.SHOW_TEXT);
        for (let n = walk.nextNode(); n; n = walk.nextNode()) {
            const el = n.parentElement;
            if (!el || el.closest(SKIP) || !shown(el)) continue;
            words.push(...(n.textContent.match(/[\p{L}\p{N}]+/gu) ?? []));
        }
        return words;
    });

// The table's standing words in states that show its piles, mana and the stack: the badges,
// the life totals and the turn button. Moving the piles' names and counts into their tips took
// these from 15, 13, 11 and 8; no change adds standing words to the table.
const WORD_BUDGETS = [
    { fixture: "choice-scry", at: pilesAt + 1, budget: 7 },
    { fixture: FIXTURE, at: 119, budget: 7 },
    { fixture: "stack-x", budget: 7 },
    { fixture: "mana-floating", at: floatingAt, budget: 8 },
];
for (const { fixture, at, budget } of WORD_BUDGETS) {
    await check(
        `${fixture}: the table stands no more words than before`,
        async (page) => {
            await page.clock.runFor(3000);
            const words = await tableWords(page);
            assert.ok(
                words.length <= budget,
                `${words.length} words, over ${budget}: ${words.join(" ")}`,
            );
        },
        {
            fixture,
            at: at ?? (await load(fixture)).length,
            ready: ".table",
            reducedMotion: "reduce",
        },
    );
}

await check(
    "unspent mana shows by the life orb and pulses on a pass, unasked",
    async (page) => {
        await page.clock.runFor(3000);
        await page.clock.pauseAt(Date.now() + 60_000);
        const decision = floatingViews[floatingAt - 1].status.decision;
        assert.equal(decision.passTo.kind, "step", "the pass ends the step");
        const pool = page.locator(".mana-pool");
        assert.equal(
            await pool.getAttribute("aria-label"),
            "Unspent mana: {C}",
        );
        await page.getByRole("button", { name: /^Next/ }).click();
        assert.equal(await page.locator(".mana-pool-pulse").count(), 1);
        assert.equal(
            await page.getByRole("dialog").count(),
            0,
            "passing asks nothing",
        );
        assert.deepEqual(await chosen(page), [
            decision.candidates.findIndex((c) => c.option.kind === "pass"),
        ]);
        // The step ends and the pool empties; it goes once the pulse is over.
        await page.clock.runFor(5000);
        assert.equal(await pool.count(), 0, "the emptied pool goes");
    },
    {
        fixture: "mana-floating",
        at: floatingAt,
        ready: ".card-el:is(.card-el-hand, .card-el-land, .card-el-field)",
    },
);

await check(
    "each stack item names whose it is and shows any X announced",
    async (page) => {
        await page.clock.runFor(3000);
        const view = (await load("stack-x")).at(-1);
        const abilities = page.locator(".stack-ability");
        let ability = 0;
        for (const item of view.stack) {
            const [el, controller] =
                item.kind === "spell"
                    ? [
                          page.locator(
                              `.card-el-stack[data-slot="${item.slot}"]`,
                          ),
                          view.objects.find((o) => o.slot === item.slot)
                              .controller,
                      ]
                    : [abilities.nth(ability++), item.controller];
            assert.equal(
                await el
                    .locator("[data-controller]")
                    .getAttribute("data-controller"),
                controller,
            );
            const x = el.locator(".stack-x");
            if (item.x > 0) {
                assert.equal(await x.innerText(), `X=${item.x}`);
            } else {
                assert.equal(await x.count(), 0, "no X, no badge");
            }
        }
        const controllers = view.stack.map((i) =>
            i.kind === "spell"
                ? view.objects.find((o) => o.slot === i.slot).controller
                : i.controller,
        );
        assert.deepEqual(
            new Set(controllers),
            new Set(["you", "opponent"]),
            "the fixture's stack holds both players' items",
        );
        assert.ok(
            view.stack.some((i) => i.x > 0),
            "the fixture's stack holds an X",
        );
    },
    {
        fixture: "stack-x",
        at: (await load("stack-x")).length,
        ready: ".card-el-stack",
        reducedMotion: "reduce",
    },
);

// A late, crowded table: at least eight nonland permanents a side and a spell on the stack.
const crowdedViews = await load("crowded");
const crowdedAt = crowdedViews.findLastIndex(
    (v) =>
        v.status.decision?.kind === "priority" &&
        v.stack.some((item) => item.kind === "spell"),
);
assert.ok(crowdedAt >= 0, "the crowded fixture has a spell on the stack");
for (const side of ["you", "opponent"]) {
    assert.ok(
        nonlands(crowdedViews[crowdedAt], side).length >= 8,
        `the crowded fixture has eight nonland permanents for ${side}`,
    );
}
const crowdedRun = (at, viewport) => ({
    fixture: "crowded",
    at: at + 1,
    ready: '[data-zone="battlefield"]',
    reducedMotion: "reduce",
    ...(viewport && { viewport }),
});

/** The permanents of each row a player sees whole, by side: each but the lands, what lies
 * tucked under another, and the copies beneath a pile's top card, with the box of the card and
 * of each of its name, power and toughness (or loyalty), count and counter badges. */
const rows = (page) =>
    page.evaluate(() => {
        const cards = [
            ...document.querySelectorAll(
                '.card-el[data-zone="battlefield"]:not(.card-el-land)',
            ),
        ].filter((e) => !e.querySelector(".card-under"));
        const z = (e) => Number(e.style.zIndex) || 0;
        const box = (e) => {
            const r = e.getBoundingClientRect();
            return { x: r.x, y: r.y, width: r.width, height: r.height };
        };
        const beneath = (e) => {
            const r = e.getBoundingClientRect();
            return cards.some((f) => {
                const s = f.getBoundingClientRect();
                return (
                    f !== e &&
                    f.dataset.card === e.dataset.card &&
                    z(f) > z(e) &&
                    Math.abs(s.x - r.x) < r.width * 0.2 &&
                    Math.abs(s.y - r.y) < r.height * 0.2
                );
            });
        };
        const out = { you: [], opponent: [] };
        for (const e of cards.filter((e) => !beneath(e))) {
            const side = e.classList.contains("card-el-you")
                ? "you"
                : "opponent";
            out[side].push({
                slot: e.dataset.slot,
                name: e.dataset.card,
                box: box(e),
                marks: [
                    ...e.querySelectorAll(
                        ".card-name, .card-pt, .card-count, .mark-badge",
                    ),
                ].map((m) => ({ mark: m.className, box: box(m) })),
            });
        }
        for (const side of Object.values(out))
            side.sort((a, b) => a.box.x - b.box.x);
        return out;
    });

/** Of the points across the middle of `box`, inset 2 px, those where the topmost element is
 * not inside the card in `slot`. */
const coveredPoints = (page, slot, box) =>
    page.evaluate(
        ({ slot, box }) => {
            const y = box.y + box.height / 2;
            const xs = [
                box.x + 2,
                box.x + box.width / 2,
                box.x + box.width - 2,
            ];
            return xs.filter((x) => {
                const top = document.elementFromPoint(x, y);
                return top?.closest("[data-slot]")?.dataset.slot !== slot;
            });
        },
        { slot, box },
    );

/** Whether two boxes overlap by more than a pixel each way. */
const overlap = (a, b) =>
    Math.min(a.x + a.width, b.x + b.width) - Math.max(a.x, b.x) > 1 &&
    Math.min(a.y + a.height, b.y + b.height) - Math.max(a.y, b.y) > 1;

await check(
    "no permanent's name or marks are covered on a crowded table",
    async (page) => {
        await page.clock.runFor(2000);
        const covered = [];
        for (const cards of Object.values(await rows(page))) {
            for (const { slot, name, marks } of cards) {
                for (const { mark, box } of marks) {
                    const at = await coveredPoints(page, slot, box);
                    if (at.length) covered.push(`${name} ${mark} at x ${at}`);
                }
            }
        }
        assert.deepEqual(covered, [], "covered names and marks");
        await assertClearOfLands(page);
    },
    crowdedRun(crowdedAt),
);

await check(
    "sweeping a crowded table moves no card but inspects each, clear of the stack and lives",
    async (page) => {
        await page.clock.runFor(2000);
        const still = await drawn(page);
        const forbidden = await page.evaluate(() =>
            [
                ...document.querySelectorAll(
                    ".card-el-stack, .stack-ability, [data-life]",
                ),
            ].map((e) => {
                const r = e.getBoundingClientRect();
                return {
                    what: e.dataset.card ?? e.dataset.life ?? "an ability",
                    x: r.x,
                    y: r.y,
                    width: r.width,
                    height: r.height,
                };
            }),
        );
        assert.ok(
            forbidden.filter((f) => f.what === "you" || f.what === "opponent")
                .length === 2 && forbidden.length >= 3,
            "the stack and both lives are drawn",
        );
        const { you, opponent } = await rows(page);
        const hand = await page.$$eval(".card-el-hand", (cards) =>
            cards.map((e) => {
                const r = e.getBoundingClientRect();
                return {
                    slot: e.dataset.slot,
                    name: e.dataset.card,
                    box: { x: r.x, y: r.y, width: r.width, height: r.height },
                };
            }),
        );
        for (const { slot, name, box } of [...opponent, ...you, ...hand]) {
            // Where the card shows: its frame's middle, or a hand card's top.
            const at = {
                x: box.x + box.width / 2,
                y:
                    box.y +
                    (hand.some((h) => h.slot === slot) ? 30 : box.height / 2),
            };
            await page.mouse.move(at.x, at.y);
            await page.clock.runFor(400);
            const now = await drawn(page);
            for (const [other, { box: b }] of Object.entries(now)) {
                if (other === slot || !still[other]) continue;
                const moved = Math.max(
                    ...b.map((v, i) => Math.abs(v - still[other].box[i])),
                );
                assert.ok(
                    moved <= 1,
                    `hovering ${name} moved slot ${other} by ${moved}`,
                );
            }
            const inspect = page.locator(
                `.inspect[aria-label="${name}, inspected"]`,
            );
            assert.equal(
                await inspect.count(),
                1,
                `the pointer at ${name}'s middle inspects it`,
            );
            const shown = await inspect.boundingBox();
            for (const f of forbidden) {
                assert.ok(
                    !overlap(shown, f),
                    `${name}'s inspector covers ${f.what}`,
                );
            }
        }
    },
    crowdedRun(crowdedAt),
);

/** A prompt of `shape` for `purpose` in place of `prompt`, the record's other fields empty
 * but for `fields`: what a view edited by hand says its decision asks. */
function asking(prompt, shape, purpose, fields = {}) {
    return {
        ...prompt,
        shape,
        purpose,
        part: "pick",
        title: "",
        count: 0,
        most: 0,
        places: ["none", "none"],
        cost: null,
        about: [],
        from: null,
        ...fields,
        words: {
            confirm: "",
            counted: "",
            decline: "",
            yes: "",
            no: "",
            auto: "",
            cancel: "Cancel",
            piles: ["", ""],
            ...fields.words,
        },
    };
}

/** Writes the fixture `as`: fixture `name` up to its view at index `at`, that view changed by
 * `edit`, so that a check can see one state of the table with and without something. */
async function derive(name, as, at, edit) {
    const fixture = JSON.parse(
        await readFile(join(fixtures, `${name}.json`), "utf8"),
    );
    const messages = [];
    let seen = -1;
    for (const m of fixture.messages) {
        if (m.kind === "event" && ++seen > at) break;
        if (m.kind === "event" && seen === at) {
            m.payload.view = edit(m.payload.view);
        }
        messages.push(m);
    }
    await writeFixture(as, { ...fixture, messages });
}

// The crowded table with a spell on the stack, and the same table with the stack empty, as it
// stands before the spell is cast and after it resolves. The spell names nothing here: a pile
// the stack names splits (by-isv.2), which changes the table.
const topSpell = crowdedViews[crowdedAt].stack.findLast(
    (item) => item.kind === "spell",
);
await derive("crowded", "crowded-stacked", crowdedAt, (v) => ({
    ...v,
    stack: [{ ...topSpell, targets: [] }],
}));
await derive("crowded", "crowded-unstacked", crowdedAt, (v) => ({
    ...v,
    stack: [],
}));
await check(
    "the stack appearing or resolving moves no permanent on a crowded table",
    async (page) => {
        const places = async (fixture) => {
            await page.goto(
                new URL(`/?fixture=${fixture}&at=${crowdedAt + 1}#/game`, base)
                    .href,
                { waitUntil: "load" },
            );
            await page.waitForSelector('[data-zone="battlefield"]');
            await page.clock.runFor(2000);
            return page.$$eval('.card-el[data-zone="battlefield"]', (cards) =>
                Object.fromEntries(
                    cards.map((e) => {
                        const r = e.getBoundingClientRect();
                        return [e.dataset.slot, [r.x, r.y, r.width, r.height]];
                    }),
                ),
            );
        };
        const stacked = await places("crowded-stacked");
        assert.ok(
            await page.$(".card-el-stack"),
            "the spell is drawn on the stack",
        );
        const unstacked = await places("crowded-unstacked");
        assert.equal(
            await page.$(".card-el-stack"),
            null,
            "the stack is empty",
        );
        for (const [slot, box] of Object.entries(stacked)) {
            const moved = Math.max(
                ...box.map((v, i) => Math.abs(v - unstacked[slot][i])),
            );
            assert.ok(moved <= 4, `the stack moves slot ${slot} by ${moved}`);
        }
    },
    crowdedRun(crowdedAt),
);

// A coin flip is announced for a moment, below the opponent's life orb.
await derive("crowded", "crowded-flip", crowdedAt, (v) => ({
    ...v,
    events: [...v.events, { kind: "coinFlip", player: "opponent", won: false }],
}));
await check(
    "a coin flip's banner clears the life orbs and goes after a moment",
    async (page) => {
        const banner = page.locator(".step-banner");
        await banner.waitFor();
        assert.match(await banner.innerText(), /flip/i, "the flip's banner");
        const coin = banner.locator(".step-banner-coin");
        await coin.waitFor();
        assert.equal(
            await coin.evaluate((e) => getComputedStyle(e).animationDuration),
            "0.3s",
        );
        const shown = await banner.boundingBox();
        for (const side of ["you", "opponent"]) {
            const life = await page
                .locator(`[data-life="${side}"]`)
                .boundingBox();
            assert.ok(!overlap(shown, life), `the banner covers ${side}r life`);
        }
        await page.clock.runFor(4000);
        await banner.waitFor({ state: "detached" });
    },
    { ...crowdedRun(crowdedAt), fixture: "crowded-flip" },
);

/** Each permanent's power and toughness (or loyalty) plaque on the table, by its card, and its
 * numerals' size on screen in window pixels: their font's size by the scales its card and the
 * stage are drawn at. A scan's printed text is too small to read there; the plaques are not. */
const plaqueSizes = (page) =>
    page.$$eval(
        '.card-el[data-zone="battlefield"]:not(.card-el-land) .card-pt span',
        (names) => {
            const scale = (e) => {
                const m = new DOMMatrix(getComputedStyle(e).transform);
                return Math.hypot(m.a, m.b);
            };
            const stage = scale(document.querySelector(".stage"));
            return names.map((e) => {
                const card = e.closest(".card-el");
                return {
                    name: card.dataset.card,
                    px:
                        parseFloat(getComputedStyle(e).fontSize) *
                        scale(card) *
                        stage,
                };
            });
        },
    );

// A crowded table's rows still fill the table between them, and their plaques stay readable.
await check(
    "a crowded table's plaques read at 14 px, its rows filling the room between them",
    async (page) => {
        await page.clock.runFor(2000);
        const sizes = await plaqueSizes(page);
        assert.ok(sizes.length >= 8, "both rows' plaques are drawn");
        const small = sizes
            .filter((n) => n.px < 14)
            .map((n) => `${n.name} at ${n.px.toFixed(1)} px`);
        assert.deepEqual(small, [], "plaques under 14 px");
        const { you, opponent } = await rows(page);
        const far = Math.max(...opponent.map((c) => c.box.y + c.box.height));
        const near = Math.min(...you.map((c) => c.box.y));
        // Only the lane where combat and the stack's arrows run lies between them.
        assert.ok(near - far <= 48, `${near - far} px between the rows`);
        assert.deepEqual(await cutText(page), [], "no name is cut");
    },
    crowdedRun(crowdedAt),
);

/** What lies over the opponent's hand, a permanent or a land, of what `by` matches: at each one's
 * middle and at its corners inset 8 px, as the ui-live harness's `probe.covered` looks. A copy
 * beneath another in a pile is covered by its own pile, which `by` does not match. */
const coveredBy = (page, by) =>
    page.evaluate((by) => {
        const out = [];
        for (const e of document.querySelectorAll(
            ".card-el-held, .card-el-field, .card-el-land",
        )) {
            const r = e.getBoundingClientRect();
            const points = [
                [r.left + r.width / 2, r.top + r.height / 2],
                [r.left + 8, r.top + 8],
                [r.right - 8, r.top + 8],
                [r.left + 8, r.bottom - 8],
                [r.right - 8, r.bottom - 8],
            ].filter(
                ([x, y]) =>
                    x >= 0 && y >= 0 && x < innerWidth && y < innerHeight,
            );
            for (const [x, y] of points) {
                const over = document.elementFromPoint(x, y)?.closest(by);
                if (over && !e.contains(over)) {
                    const what = e.dataset.card ?? "a back";
                    out.push(
                        `${what} under ${over.className} at ${Math.round(x)}, ${Math.round(y)}`,
                    );
                }
            }
        }
        return out;
    }, by);

/** What may never lie over a card on the table: a life orb, a tip, the game log or the reach of
 * a raised hand card. */
const COVERS = "[data-life], .tip, .game-log, .hand-hit";

// Each side of the crowded table has six lands. Add one Island to either side in turn,
// then remove it. Only the side that crosses seven should change its piling.
const landThreshold = JSON.parse(
    await readFile(join(fixtures, "crowded.json"), "utf8"),
);
const landUpdate = landThreshold.messages.filter((m) => m.kind === "event")[
    crowdedAt
];
const sixLands = landUpdate.payload.view;
const seventhLand = Object.fromEntries(
    ["you", "opponent"].map((side, i) => [
        side,
        {
            ...sixLands.objects.find(
                (o) =>
                    o.controller === side &&
                    o.name === "Island" &&
                    o.zone === "battlefield",
            ),
            slot: 9300 + i,
        },
    ]),
);
await writeFixture("land-threshold", {
    messages: [
        ...landThreshold.messages.filter((m) => m.kind === "response"),
        { kind: "response", command: "choose", value: null },
        ...[undefined, "you", "opponent", undefined].map((side, i) => {
            const land = seventhLand[side];
            return {
                ...landUpdate,
                payload: {
                    ...landUpdate.payload,
                    view: {
                        ...sixLands,
                        events: [],
                        battlefield: land
                            ? [...sixLands.battlefield, land.slot]
                            : sixLands.battlefield,
                        objects: land
                            ? [...sixLands.objects, land]
                            : sixLands.objects,
                        status: {
                            ...sixLands.status,
                            decision: {
                                ...sixLands.status.decision,
                                id: sixLands.status.decision.id + i,
                            },
                        },
                    },
                },
            };
        }),
    ],
});
await check(
    "lands pile only from seven on each side and separate again at six",
    async (page) => {
        for (const [i, stacked] of [
            undefined,
            "you",
            "opponent",
            undefined,
        ].entries()) {
            if (i) await page.keyboard.press(" ");
            await page.clock.runFor(1500);
            for (const side of ["you", "opponent"]) {
                const lands = page.locator(`.card-el-land.card-el-${side}`);
                const seventh = lands.locator(
                    `xpath=self::*[@data-slot="${seventhLand[side].slot}"]`,
                );
                const counts = lands.locator(".card-count");
                assert.equal(
                    await seventh.count(),
                    side === stacked ? 1 : 0,
                    `view ${i}: ${side}'s seventh land`,
                );
                assert.equal(
                    (await counts.count()) > 0,
                    side === stacked,
                    `view ${i}: ${side}'s land piles`,
                );
            }
        }
    },
    { ...crowdedRun(0), fixture: "land-threshold" },
);

// Eight cards in your hand while you may pass on to the next stop: the fan, each card at rest
// and raised in turn, stays clear of the round button left of the turn button (by-biv).
await derive("crowded", "crowded-eight", crowdedAt, (v) => {
    const hand = v.objects.filter(
        (o) => o.owner === "you" && o.zone === "hand",
    );
    const more = Array.from({ length: 8 - hand.length }, (_, i) => ({
        ...hand[i % hand.length],
        slot: 9200 + i,
    }));
    const decision = v.status.decision;
    return {
        ...v,
        objects: [...v.objects, ...more],
        status: {
            ...v.status,
            decision: {
                ...decision,
                passOn: decision.passOn ?? { kind: "nextTurn" },
            },
        },
    };
});
await check(
    "a full hand keeps clear of the pass-on button",
    async (page) => {
        await page.clock.runFor(2000);
        const button = await page.locator(".pass-on-button").boundingBox();
        assert.ok(button, "the pass-on button shows");
        // The right edge of everything drawn for a hand card, a raised one's copy included.
        const right = () =>
            page.$$eval(
                ".card-el-hand, .card-el-hand *, .card-ghost *",
                (all) =>
                    Math.max(
                        ...all.map((e) => e.getBoundingClientRect().right),
                    ),
            );
        const cards = await page.locator(".card-el-hand").all();
        assert.equal(cards.length, 8, "eight cards in hand");
        assert.ok(
            (await right()) < button.x,
            `the hand reaches ${await right()}, the button begins at ${button.x}`,
        );
        for (const card of cards) {
            const box = await card.boundingBox();
            await page.mouse.move(box.x + box.width / 2, box.y + 30);
            await page.clock.runFor(600);
            const name = await card.getAttribute("data-card");
            const reach = await right();
            assert.ok(
                reach < button.x,
                `${name} raised reaches ${reach}, the button begins at ${button.x}`,
            );
        }
    },
    { ...crowdedRun(crowdedAt), fixture: "crowded-eight" },
);

// The opponent holding nine cards: their fan's outer backs, which its turn lifts, still show.
await derive(FIXTURE, "held-nine", views.length - 2, (v) => {
    const held = v.objects.filter(
        (o) => o.owner === "opponent" && o.zone === "hand",
    );
    const more = Array.from({ length: 9 - held.length }, (_, i) => ({
        ...held[0],
        slot: 9100 + i,
    }));
    return { ...v, objects: [...v.objects, ...more] };
});
for (const [title, run] of [
    ["a crowded table", crowdedRun(crowdedAt)],
    [
        "a full opposing hand",
        {
            at: views.length - 1,
            ready: ".card-el-held",
            reducedMotion: "reduce",
        },
    ],
    [
        "nine cards in the opposing hand",
        {
            fixture: "held-nine",
            at: views.length - 1,
            ready: ".card-el-held",
            reducedMotion: "reduce",
        },
    ],
]) {
    await check(
        `on ${title}, no orb, tip, log or raised hand card covers a card on the table`,
        async (page) => {
            await page.clock.runFor(2000);
            assert.deepEqual(await coveredBy(page, COVERS), [], "at rest");
            // Each of the opponent's backs reaches well into the window, to be counted.
            const feet = await page.$$eval(".card-el-held", (backs) =>
                backs.map((e) => Math.round(e.getBoundingClientRect().bottom)),
            );
            assert.ok(
                feet.every((y) => y >= 24),
                `the backs end at ${feet.join(", ")}`,
            );
            // Each hand card raised in turn, the pointer at its top.
            for (const card of await page.locator(".card-el-hand").all()) {
                const box = await card.boundingBox();
                await page.mouse.move(box.x + box.width / 2, box.y + 30);
                await page.clock.runFor(600);
                const name = await card.getAttribute("data-card");
                assert.deepEqual(
                    await coveredBy(page, COVERS),
                    [],
                    `${name} raised`,
                );
            }
            // Each of your piles' tips, over its pile, clear of your badge if it shows.
            const badges = page.locator(".player-badge");
            const badge = (await badges.count())
                ? await badges.last().boundingBox()
                : undefined;
            for (const pile of await page
                .locator(".has-tip:has([aria-label^=Library]), [data-side=you]")
                .all()) {
                const box = await pile.boundingBox();
                if (!box || box.y < 450) continue;
                await page.mouse.move(box.x + box.width / 2, box.y + 20);
                await page.clock.runFor(300);
                const tip = page.locator(".tip").filter({ visible: true });
                assert.equal(await tip.count(), 1, "the pile's tip shows");
                assert.ok(
                    !badge || !overlap(await tip.boundingBox(), badge),
                    `${await tip.innerText()} covers your badge`,
                );
                assert.deepEqual(await coveredBy(page, COVERS), [], "a tip");
            }
            const toggle = page.getByRole("button", { name: "Show log" });
            if (await toggle.isVisible()) {
                await toggle.click();
                await page.clock.runFor(2000);
                assert.deepEqual(
                    await coveredBy(page, COVERS),
                    [],
                    "the log open",
                );
            }
        },
        run,
    );
}

// At 16:10, the table reaches the window's edges rather than stand in bands, and each hand
// runs off its edge of the window as at 16:9, showing no less of each card.
await check(
    "a 16:10 window shows the table to its edges, both hands reaching them",
    async (page) => {
        await page.clock.runFor(2000);
        const { width, height } = page.viewportSize();
        const bare = await page.evaluate(
            ([w, h]) => {
                const frame = document.querySelector(".stage-frame");
                const points = [0.1, 0.3, 0.5, 0.7, 0.9].flatMap((f) => [
                    [w * f, 16],
                    [w * f, h - 16],
                    [16, h * f],
                    [w - 16, h * f],
                ]);
                return points
                    .filter(
                        ([x, y]) => document.elementFromPoint(x, y) === frame,
                    )
                    .map((p) => p.map(Math.round).join(", "));
            },
            [width, height],
        );
        assert.deepEqual(
            bare,
            [],
            "points 16 px from an edge showing no table",
        );
        // Each card of both hands runs off the window's edge, as at 16:9, showing at least as
        // much of itself as a 16:9 window would: as much as lies on the stage.
        const hands = await page.$$eval(
            ".card-el-hand, .card-el-held",
            (cards) => {
                const stage = document
                    .querySelector(".stage")
                    .getBoundingClientRect();
                const part = (r, top, bottom) =>
                    (Math.min(r.bottom, bottom) - Math.max(r.top, top)) /
                    r.height;
                return cards.map((e) => {
                    const r = e.getBoundingClientRect();
                    const held = e.classList.contains("card-el-held");
                    return {
                        slot: e.dataset.slot,
                        held,
                        shown: part(r, 0, innerHeight),
                        wide: part(r, stage.top, stage.bottom),
                        reaches: held ? r.top < 0 : r.bottom > innerHeight,
                        inside: r.left >= 0 && r.right <= innerWidth,
                    };
                });
            },
        );
        assert.ok(
            hands.some((c) => c.held) && hands.some((c) => !c.held),
            "both hands hold cards",
        );
        for (const c of hands) {
            assert.ok(
                c.reaches,
                `slot ${c.slot} ends short of the window's edge`,
            );
            assert.ok(c.inside, `slot ${c.slot} is cut at the window's side`);
            assert.ok(
                c.shown >= c.wide - 0.01,
                `slot ${c.slot} shows ${c.shown.toFixed(2)} of itself, ` +
                    `${c.wide.toFixed(2)} at 16:9`,
            );
        }
    },
    {
        at: views.length - 1,
        ready: ".card-el-held",
        reducedMotion: "reduce",
        viewport: { width: 1280, height: 800 },
    },
);

// A pile too long for its viewer fades out where more of it lies, until scrolled there.
const crowdedYou = crowdedViews[crowdedAt].objects.find(
    (o) =>
        o.zone === "battlefield" &&
        o.controller === "you" &&
        !o.types.includes("land") &&
        o.name,
);
await derive("crowded", "crowded-graveyard", crowdedAt, (v) => {
    const dead = Array.from({ length: 14 }, (_, i) => ({
        ...crowdedYou,
        slot: 9000 + i,
        zone: "graveyard",
        tapped: false,
        combat: null,
        counters: [],
        damage: 0,
    }));
    return {
        ...v,
        objects: [...v.objects, ...dead],
        players: {
            ...v.players,
            you: {
                ...v.players.you,
                graveyard: [
                    ...v.players.you.graveyard,
                    ...dead.map((o) => o.slot),
                ],
            },
        },
    };
});
await check(
    "a long pile's viewer fades where more of it lies beyond the scroll",
    async (page) => {
        await page.clock.runFor(2000);
        await page.click("[data-pile=graveyard][data-side=you]");
        await page.clock.runFor(1000);
        const list = page.locator(".zone-viewer-cards");
        const state = () =>
            list.evaluate((e) => ({
                over: e.scrollHeight > e.clientHeight,
                more: e.dataset.more,
                mask:
                    getComputedStyle(e).maskImage ||
                    getComputedStyle(e).webkitMaskImage,
            }));
        const top = await state();
        assert.ok(top.over, "fourteen cards overflow the viewer");
        assert.equal(top.more, "below");
        assert.notEqual(top.mask, "none", "the list's foot fades");
        await list.evaluate((e) => {
            e.scrollTop = e.scrollHeight;
        });
        // The browser sends the scroll event with its next frame, in its own time.
        let more;
        for (let tries = 0; tries < 30 && more !== "above"; tries++) {
            await new Promise((resolve) => setTimeout(resolve, 100));
            await page.clock.runFor(100);
            ({ more } = await state());
        }
        assert.equal(more, "above", "scrolled to its end");
    },
    {
        ...crowdedRun(crowdedAt),
        fixture: "crowded-graveyard",
    },
);

// Cards over the table are small to fit; each shows whole on hover, wherever it is.
for (const [title, run, selector] of [
    ["the opening hand", { at: 2, ready: ".opening-card" }, ".opening-card"],
    [
        "a scry",
        {
            fixture: "choice-scry",
            at: scry.views.length,
            ready: ".choice-card .shown-card",
        },
        ".choice-card .shown-card",
    ],
]) {
    await check(
        `hovering a card in ${title} inspects it`,
        async (page) => {
            await page.clock.runFor(2000);
            const cards = page.locator(selector);
            const target = cards.last();
            const box = await target.boundingBox();
            // The card's visible right edge: cards in a tray may overlap.
            await page.mouse.move(
                box.x + box.width - 12,
                box.y + box.height / 2,
            );
            await page.clock.runFor(300);
            const name = await target.evaluate(
                (e) => e.closest("[data-card]")?.dataset.card,
            );
            await page
                .getByLabel(`${name}, inspected`)
                .waitFor({ state: "visible" });
            await page.mouse.move(1, 1);
            await page.clock.runFor(300);
            assert.equal(
                await page.$(".inspect"),
                null,
                "the inspector goes with the pointer",
            );
        },
        run,
    );
}

/** The inspector's box, once it shows the card `name`. */
async function inspectorOf(page, name) {
    const inspect = page.locator(`.inspect[aria-label="${name}, inspected"]`);
    await inspect.waitFor({ state: "visible" });
    return inspect.boundingBox();
}

/** A point of the element `selector` picks (the `nth`) where it is on top, so the pointer
 * reaches it: its middle if nothing covers that, else the first such point on a 10 px grid.
 * Whole pixels, where WebKit delivers the pointer: a fraction of a pixel inside a leaning
 * card's edge may lie outside it there. */
const visiblePoint = (page, selector, nth) =>
    page.evaluate(
        ([selector, nth]) => {
            const e = document.querySelectorAll(selector)[nth];
            const r = e.getBoundingClientRect();
            const on = (x, y) => e.contains(document.elementFromPoint(x, y));
            const mid = {
                x: Math.round(r.x + r.width / 2),
                y: Math.round(r.y + r.height / 2),
            };
            if (on(mid.x, mid.y)) return mid;
            for (let y = Math.ceil(r.top) + 5; y < r.bottom; y += 10) {
                for (let x = Math.ceil(r.left) + 5; x < r.right; x += 10) {
                    if (on(x, y)) return { x, y };
                }
            }
            return null;
        },
        [selector, nth],
    );

// A prompt's cards show whole on hover, beside its title, options and cards, never over
// them, on an opaque inspector (by-isv.17): a look at the top four.
for (const [fixture, at] of [
    ["choice-look", (await load("choice-look")).length],
]) {
    await check(
        `hovering a card in ${fixture}'s prompt keeps its inspector off the prompt`,
        async (page) => {
            await page.clock.runFor(2000);
            const prompt = await page.$$eval(
                ".overlay-main :is(.overlay-title, button, .shown-card)",
                (els) =>
                    els.map((e) => {
                        const r = e.getBoundingClientRect();
                        return {
                            what:
                                e.closest("[data-card]")?.dataset.card ??
                                e.textContent.trim().slice(0, 30),
                            x: r.x,
                            y: r.y,
                            width: r.width,
                            height: r.height,
                        };
                    }),
            );
            assert.ok(
                prompt.some((p) => p.what) && prompt.length >= 3,
                "the prompt is drawn",
            );
            const cards = ".overlay .shown-card";
            const count = await page.locator(cards).count();
            assert.ok(count >= 1, "the prompt shows a card");
            for (let i = 0; i < count; i++) {
                const point = await visiblePoint(page, cards, i);
                if (!point) continue;
                await page.mouse.move(point.x, point.y);
                await page.clock.runFor(300);
                const inspect = page.locator(".inspect");
                await inspect.waitFor({ state: "visible" });
                const name = await inspect.getAttribute("aria-label");
                const shown = await inspect.boundingBox();
                for (const p of prompt) {
                    assert.ok(
                        !overlap(shown, p),
                        `${name}'s inspector covers ${p.what}`,
                    );
                }
                const fill = await inspect.evaluate(
                    (e) => getComputedStyle(e).backgroundColor,
                );
                assert.match(
                    fill,
                    /^rgb\(/,
                    `the inspector is opaque, not ${fill}`,
                );
            }
        },
        { fixture, at, ready: ".overlay .shown-card" },
    );
}

// In a target step, a card shows whole clear of every target it might cover (by-isv.17):
// Fractured Identity's in greedy-turn-9, whose targets fill the table's middle, and Swift
// Reconfiguration's in attached-across and in crew, whose targets lie on both sides.
for (const fixture of ["greedy-turn-9", "attached-across", "crew"]) {
    const views = await load(fixture);
    const at = views.findIndex((v) => v.status.decision?.kind === "target");
    await check(
        `hovering a card in ${fixture}'s target step covers no target`,
        async (page) => {
            await page.clock.runFor(2000);
            const targets = await page.$$eval(
                '[data-zone="battlefield"]:has(.card-target)',
                (els) =>
                    els.map((e) => {
                        const r = e.getBoundingClientRect();
                        return {
                            what: e.dataset.card,
                            x: r.x,
                            y: r.y,
                            width: r.width,
                            height: r.height,
                        };
                    }),
            );
            assert.ok(targets.length > 0, "the targets glow");
            const cards = '.card-el-hand, .card-el[data-zone="battlefield"]';
            const count = await page.locator(cards).count();
            for (let i = 0; i < count; i++) {
                const name = await page
                    .locator(cards)
                    .nth(i)
                    .getAttribute("data-card");
                const point = name && (await visiblePoint(page, cards, i));
                if (!point) continue;
                await page.mouse.move(point.x, point.y);
                await page.clock.runFor(400);
                const shown = await inspectorOf(page, name);
                for (const t of targets) {
                    assert.ok(
                        !overlap(shown, t),
                        `${name}'s inspector covers the target ${t.what}`,
                    );
                }
            }
        },
        {
            fixture,
            at: at + 1,
            ready: '[data-zone="battlefield"]',
            reducedMotion: "reduce",
        },
    );
}

// Up to one target is one targeting step (`target-up-to-one`: Teferi, Time Raveler's −3, "up to
// one target artifact, creature, or enchantment"): its targets glow at the count, and clicking
// one announces one target and then it; a single button chooses none.
const upTo = await load("target-up-to-one");
const countAt = upTo.findLastIndex(
    (v) => v.status.decision?.kind === "targetCount",
);
const counted = upTo[countAt].status.decision;
const targeted = upTo.slice(countAt + 1).find((v) => v.status.decision)
    .status.decision;
assert.equal(targeted.kind, "target", "the fixture announces one target");
const countOf = (n) =>
    counted.candidates.findIndex(
        (c) => c.option.kind === "number" && c.option.n === n,
    );
const aimed = targeted.candidates.find(
    (c) => c.label === "Staff of the Storyteller",
).option.slot;
const upToRun = {
    fixture: "target-up-to-one",
    at: countAt + 1,
    ready: ".prompt",
};
await check(
    "up to one target is picked on the board with its count",
    async (page) => {
        await page.clock.runFor(2000);
        assert.ok(
            await page.$(`${slotOf(aimed)} .card-target`),
            "a target glows",
        );
        const prompt = page.getByRole("dialog");
        assert.match(await prompt.innerText(), /Choose a target/);
        assert.deepEqual(
            await prompt.locator(".prompt-choices button").allInnerTexts(),
            ["Submit 0", "Cancel"],
            "no count is asked",
        );
        await page.click(slotOf(aimed));
        assert.deepEqual(await settled(page, 2), [
            countOf(1),
            targeted.candidates.findIndex(
                (c) => c.option.kind === "object" && c.option.slot === aimed,
            ),
        ]);
    },
    upToRun,
);
await check(
    "up to one target chooses none from its one button",
    async (page) => {
        await page.getByRole("button", { name: "Submit 0" }).click();
        assert.deepEqual(await settled(page, 1), [countOf(0)]);
    },
    upToRun,
);

await check(
    "optional self-removal waits for confirmation before sending either pick",
    async (page) => {
        await page.clock.runFor(2000);
        const ownIndex = targeted.candidates.findIndex(
            (c) => c.label === "Jace, Vryn's Prodigy",
        );
        const slot = targeted.candidates[ownIndex].option.slot;
        const warning = page.getByRole("dialog", {
            name: "Target your Jace, Vryn's Prodigy?",
        });
        await page.click(slotOf(slot));
        await warning.waitFor();
        assert.deepEqual(await chosen(page), []);
        const cancel = warning.getByRole("button", { name: "Cancel" });
        assert.deepEqual(
            await cancel.evaluate((e) => ({
                focused: document.activeElement === e,
                outline: getComputedStyle(e).outlineStyle,
                offset: getComputedStyle(e).outlineOffset,
            })),
            { focused: true, outline: "solid", offset: "-2px" },
            "Cancel's focus outline replaces the rim in place",
        );
        await page.keyboard.press("Escape");
        await page.getByRole("button", { name: "Submit 0" }).waitFor();
        await page.click(slotOf(slot));
        await cancel.click();
        await page.getByRole("button", { name: "Submit 0" }).waitFor();
        assert.equal(await cancels(page), 0);
        assert.deepEqual(await chosen(page), []);

        await page.click(slotOf(slot));
        await warning.getByRole("button", { name: "Confirm" }).click();
        assert.deepEqual(await settled(page, 2), [countOf(1), ownIndex]);
    },
    upToRun,
);

// A click never aims a spell for the player (`target-own`: Oust, whose one legal target is
// the human's own Jace, Vryn's Prodigy): the click starts the cast, the target is asked on the
// board, and Escape takes the cast back.
const own = await load("target-own");
const [ownBefore, ownAfter] = own.slice(-2);
const oust = ownBefore.objects.find((o) => o.name === "Oust").slot;
const oustCast = ownBefore.status.decision.candidates.findIndex(
    (c) => c.option.kind === "cast" && c.option.slot === oust,
);
const ownTarget = ownAfter.status.decision;
assert.equal(ownTarget.kind, "target", "the fixture asks Oust's target");
assert.deepEqual(
    ownTarget.candidates.map((c) => c.label),
    ["Jace, Vryn's Prodigy"],
    "the human's own creature is the one target",
);
await check(
    "a click on a spell with one target, the player's own, asks it",
    async (page) => {
        await page.clock.runFor(2000);
        await page.click(slotOf(oust));
        const targeting = page.getByRole("dialog", {
            name: "(Oust) Choose a target.",
        });
        await targeting.waitFor();
        assert.ok(
            await page.$(
                `${slotOf(ownTarget.candidates[0].option.slot)} .card-target`,
            ),
            "the target glows",
        );
        await page.clock.runFor(2000);
        assert.deepEqual(await chosen(page), [oustCast], "only the cast sent");
        await page.keyboard.press("Escape");
        await page.waitForSelector(`${slotOf(oust)}[data-zone="hand"]`);
        assert.equal(await cancels(page), 1, "cancelled");
        assert.equal(await targeting.count(), 0, "the target is not asked");
        assert.deepEqual(await chosen(page), [oustCast]);
    },
    {
        fixture: "target-own",
        at: own.length - 1,
        ready: `${slotOf(oust)}[data-zone="hand"]`,
    },
);

await check(
    "required self-removal sends its target once on confirmation",
    async (page) => {
        await page.clock.runFor(2000);
        await page.click(slotOf(ownTarget.candidates[0].option.slot));
        const warning = page.getByRole("dialog", {
            name: "Target your Jace, Vryn's Prodigy?",
        });
        await warning.waitFor();
        assert.deepEqual(await chosen(page), []);
        await warning.getByRole("button", { name: "Confirm" }).click();
        assert.deepEqual(await settled(page, 1), [0]);
    },
    { fixture: "target-own", at: own.length, ready: ".prompt" },
);

// Among two spells on the stack none opens picked, and a target picked remains tentative
// until Confirm. The committed link below Counterspell is Mana Tithe's, and both ends touch
// the exposed name strips.
const counterViews = await load("counterspell-targets");
const counterTarget = counterViews.at(-1);
const counterDecision = counterTarget.status.decision;
const cryptSlot = counterDecision.candidates.find(
    (c) => c.label === "Mana Crypt",
).option.slot;
const titheIndex = counterDecision.candidates.findIndex(
    (c) => c.label === "Mana Tithe",
);
const titheSlot = counterDecision.candidates[titheIndex].option.slot;
const counterSlot = counterDecision.source;
const counterRun = {
    fixture: "counterspell-targets",
    at: counterViews.length,
    ready: ".prompt",
};

async function pickStackCard(page, slot) {
    const rect = await page.locator(`#card-${slot}`).boundingBox();
    await page.mouse.click(rect.x + rect.width / 2, rect.y + 8);
}

await check(
    "Counterspell opens with no spell picked and confirms the one clicked",
    async (page) => {
        const prompt = page.getByRole("dialog");
        assert.doesNotMatch(await prompt.innerText(), /Mana Tithe|Mana Crypt/);
        assert.equal(
            await prompt.getByRole("button", { name: "Confirm" }).count(),
            0,
        );
        await pickStackCard(page, titheSlot);
        assert.match(await prompt.innerText(), /Target Mana Tithe/);
        assert.deepEqual(await chosen(page), []);
        await pickStackCard(page, cryptSlot);
        assert.match(
            await prompt.innerText(),
            /Counter your own spell, Mana Crypt\?/,
        );
        assert.deepEqual(await chosen(page), []);
        await pickStackCard(page, titheSlot);
        assert.match(await prompt.innerText(), /Target Mana Tithe/);
        await prompt.getByRole("button", { name: "Confirm" }).click();
        assert.deepEqual(await settled(page, 1), [titheIndex]);
    },
    counterRun,
);

await check(
    "Counterspell can back out of its self-target warning",
    async (page) => {
        await pickStackCard(page, cryptSlot);
        const prompt = page.getByRole("dialog");
        assert.match(
            await prompt.innerText(),
            /Counter your own spell, Mana Crypt\?/,
        );
        await prompt.getByRole("button", { name: "Cancel" }).click();
        assert.equal(await cancels(page), 1);
        assert.deepEqual(await chosen(page), []);
    },
    counterRun,
);

await check(
    "Counterspell can confirm the warned-about self-target",
    async (page) => {
        await pickStackCard(page, cryptSlot);
        await page.getByRole("button", { name: "Confirm" }).click();
        const ownIndex = counterDecision.candidates.findIndex(
            (c) => c.label === "Mana Crypt",
        );
        assert.deepEqual(await settled(page, 1), [ownIndex]);
    },
    counterRun,
);

const stackLinks =
    "pending Counterspell preserves only Mana Tithe's committed link";
if (wanted(stackLinks)) {
    for (const seat of ["yours", "theirs"]) {
        const view = structuredClone(counterTarget);
        if (seat === "theirs") {
            for (const o of view.objects) {
                o.controller = o.controller === "you" ? "opponent" : "you";
            }
            view.status = { kind: "thinking" };
        }
        const run = await motionFixture(`counterspell-links-${seat}`, [view]);
        await check(
            stackLinks + ` (${seat})`,
            async (page) => {
                await page.clock.runFor(1300);
                const links = page.locator(".board-arrows .arrow");
                assert.equal(await links.count(), 1);
                assert.equal(
                    await links.getAttribute("data-from"),
                    "stack-peek-1",
                );
                assert.equal(
                    await links.getAttribute("data-to"),
                    "stack-peek-0",
                );
                assert.ok(
                    await links.evaluate((link) =>
                        link.classList.contains("arrow-red"),
                    ),
                );
                const [shaftWidth, headHeight] = await links.evaluate(
                    (link) => {
                        const shaft = link.querySelector(".arrow-line path");
                        const head = link.querySelector(
                            ".arrow-line .arrow-head",
                        );
                        return [
                            Number.parseFloat(
                                getComputedStyle(shaft).strokeWidth,
                            ),
                            head.getBBox().height,
                        ];
                    },
                );
                assert.ok(shaftWidth <= 4, `shaft width: ${shaftWidth}px`);
                assert.ok(headHeight <= 14, `head height: ${headHeight}px`);
                const edges = await page.evaluate(
                    ({ cryptSlot, titheSlot, counterSlot }) => {
                        const crypt = document
                            .getElementById(`card-${cryptSlot}`)
                            .getBoundingClientRect();
                        const tithe = document
                            .getElementById(`card-${titheSlot}`)
                            .getBoundingClientRect();
                        const counter = document
                            .getElementById(`card-${counterSlot}`)
                            .getBoundingClientRect();
                        const shaft = document.querySelector(
                            ".board-arrows .arrow-line path",
                        );
                        const head = document.querySelector(
                            ".board-arrows .arrow-head",
                        );
                        const actualStart = shaft.getPointAtLength(0);
                        const from = new DOMPoint(
                            actualStart.x,
                            actualStart.y,
                        ).matrixTransform(shaft.getScreenCTM());
                        const to = new DOMPoint().matrixTransform(
                            head.getScreenCTM(),
                        );
                        return [
                            Math.min(
                                Math.abs(from.x - tithe.left),
                                Math.abs(from.x - tithe.right),
                            ),
                            Math.abs(from.y - (tithe.top + counter.top) / 2),
                            Math.min(
                                Math.abs(to.x - crypt.left),
                                Math.abs(to.x - crypt.right),
                            ),
                            Math.abs(to.y - (crypt.top + tithe.top) / 2),
                        ];
                    },
                    { cryptSlot, titheSlot, counterSlot },
                );
                assert.ok(
                    edges.every((distance) => distance < 1),
                    `arrow offsets: ${edges}`,
                );
                // Moving the covering spell changes the usable strip even if both
                // linked cards are otherwise stationary.
                const cover = page.locator(`#card-${counterSlot}`);
                await page.clock.pauseAt(Date.now() + 60_000);
                for (const y of [3, 6, 9]) {
                    await cover.evaluate((card, y) => {
                        card.style.translate = `0 ${y}px`;
                    }, y);
                    await page.clock.runFor(50);
                    assert.equal(
                        await links.count(),
                        0,
                        "the old strip no longer anchors a link",
                    );
                }
                await page.clock.runFor(200);
                assert.equal(
                    await links.count(),
                    1,
                    "the link returns once the strip is still",
                );
            },
            { ...run, ready: `#card-${counterSlot}` },
        );
    }
}

const objectColors =
    "target arrow color follows the effect and object controllers";
if (wanted(objectColors)) {
    const base = (await load("arrow-ability")).at(-1);
    for (const controller of ["you", "opponent"]) {
        for (const hostile of [false, true]) {
            const view = structuredClone(base);
            const item = view.stack.at(-1);
            item.controller = controller;
            const target = view.objects.find(
                (o) => o.slot === item.targets[0].slot,
            );
            target.controller = hostile
                ? controller === "you"
                    ? "opponent"
                    : "you"
                : controller;
            const tone = hostile ? "red" : "green";
            const run = await motionFixture(
                `object-arrow-${controller}-${tone}`,
                [view],
            );
            await check(
                `${objectColors}: ${controller} ${tone}`,
                async (page) => {
                    const selector = `.arrow[data-to="card-${target.slot}"]`;
                    // A followed arrow goes the moment either end moves (Stack.tsx REST_MS),
                    // and on a loaded machine one long frame before the cards' entrance can
                    // show it early: let the table settle before counting.
                    await page.clock.runFor(2000);
                    await until(page, selector, 30);
                    const links = page.locator(selector);
                    assert.equal(await links.count(), 1);
                    assert.ok(
                        await links.evaluate(
                            (link, tone) =>
                                link.classList.contains(`arrow-${tone}`),
                            tone,
                        ),
                    );
                },
                { ...run, ready: ".board-arrows .arrow" },
            );
        }
    }
}

// The session takes Reprieve's lone target, the opponent's spell alone on the stack, so the
// page is never asked for it: the cast and its planned payment are all it sends.
const reprieveTitle =
    "Reprieve takes its lone stack target before planned payment";
if (wanted(reprieveTitle)) {
    const recording = await load("reprieve-target");
    assert.ok(
        recording.every((v) => v.status.decision?.kind !== "target"),
        "the session asks for no target",
    );
    const paymentAt = recording.findIndex(
        (v) => v.status.decision?.kind === "paymentPlan",
    );
    const start = recording.findLastIndex(
        (v, i) => i < paymentAt && v.status.decision?.kind === "priority",
    );
    const castAt = recording[start].status.decision.candidates.findIndex(
        (c) => c.label === "Cast Reprieve",
    );
    const cast = recording[start].status.decision.candidates[castAt];
    assert.ok(
        cast.payments.length > 1,
        "the cast has an explicit payment plan",
    );
    const planAt = recording[paymentAt].status.decision.candidates.findIndex(
        (c) => c.option.index === cast.payments[0].plan,
    );
    await check(
        reprieveTitle,
        async (page) => {
            await page.click(slotOf(cast.option.slot));
            assert.deepEqual(await settled(page, 2), [castAt, planAt]);
        },
        {
            fixture: "reprieve-target",
            at: start + 1,
            ready: slotOf(cast.option.slot),
        },
    );
}

// An attack on a player who controls a planeswalker (`attack-past-walker`: Snapcaster Mage
// against The Wandering Emperor) attacks the player unasked: the engine's question of what it
// attacks (CR 508.1b) is answered from the declaration.
const pastWalker = await load("attack-past-walker");
const firstAim = pastWalker.findIndex(
    (v) => v.status.decision?.kind === "attackTarget",
);
const attackAt = pastWalker
    .slice(0, firstAim)
    .findLastIndex((v) => v.status.decision?.kind === "attacker");
const attacking = pastWalker[attackAt].status.decision;
const attacker = attacking.candidates.findIndex(
    (c) => c.option.kind === "attack",
);
const aiming = pastWalker[firstAim].status.decision;
await check(
    "an attack past a planeswalker asks nothing of what it attacks",
    async (page) => {
        await page.clock.runFor(2000);
        await page.click(slotOf(attacking.candidates[attacker].option.slot));
        await page.getByRole("button", { name: "Attack", exact: true }).click();
        const [first, second] = await settled(page, 2);
        assert.equal(first, attacker);
        assert.deepEqual(
            aiming.candidates[second].option,
            { kind: "defender", planeswalker: null },
            "the player is attacked",
        );
    },
    {
        fixture: "attack-past-walker",
        at: attackAt + 1,
        ready: slotOf(attacking.candidates[attacker].option.slot),
    },
);

// Explore (`choice-explore`, a Map's) shows the card it revealed, each answer named by where
// the card goes.
const explore = await lastDecision("choice-explore");
await check(
    "a beneficial own permanent target submits directly",
    async (page) => {
        const at = explore.views.findLastIndex(
            (v) => v.status.decision?.kind === "target",
        );
        const candidate = explore.views[at].status.decision.candidates[0];
        await page.clock.runFor(2000);
        await page.click(slotOf(candidate.option.slot));
        assert.deepEqual(await settled(page, 1), [0]);
    },
    {
        fixture: "choice-explore",
        at:
            explore.views.findLastIndex(
                (v) => v.status.decision?.kind === "target",
            ) + 1,
        ready: ".prompt",
    },
);
const revealed = explore.views.at(-1).players.you.library.at(-1);
await check(
    "explore shows the revealed card and answers by where it goes",
    async (page) => {
        assert.ok(await page.$(tray(revealed)), "the revealed card is shown");
        await page
            .getByRole("button", { name: "Graveyard", exact: true })
            .click();
        assert.deepEqual(await settled(page, 1), [
            explore.decision.candidates.findIndex(
                (c) => c.option.kind === "yes",
            ),
        ]);
    },
    { fixture: "choice-explore", at: explore.views.length, ready: ".choice" },
);

// A loot (`choice-loot`, Jace, Vryn's Prodigy's) shows its draw before it asks for the
// discard: the card drawn is in hand, and the log says so, as the discard is asked.
const loot = await lastDecision("choice-loot");
const lootAt = loot.views.findLastIndex(
    (v) => v.status.decision?.kind === "priority",
);
const lootHand = (v) =>
    v.objects
        .filter((o) => o.zone === "hand" && o.owner === "you")
        .map((o) => o.slot);
const lootDrawn = lootHand(loot.views.at(-1)).filter(
    (slot) => !lootHand(loot.views[lootAt]).includes(slot),
);
assert.equal(lootDrawn.length, 1, "the loot draws one card");
const jaceVp = named(loot.views[lootAt], "Jace, Vryn's Prodigy")[0];
await check(
    "a loot's draw shows before the discard is asked, from the raised hand",
    async (page) => {
        await page.clock.runFor(2000);
        await page.click(slotOf(jaceVp));
        await until(page, 'role=dialog[name="Discard a card."]');
        assert.equal(await page.$(".choice"), null, "no overlay opens");
        await page.clock.runFor(1500);
        const drawn = page.locator(
            `[data-slot="${lootDrawn[0]}"][data-zone="hand"]`,
        );
        assert.ok(
            (await drawn.locator(".card-target").count()) > 0,
            "the drawn card is offered, lit",
        );
        const box = await drawn.boundingBox();
        assert.ok(box.y + box.height < 900, "the hand is raised whole");
        // A click picks it, raised higher; a second card takes its place.
        const before = box.y;
        await drawn.click();
        await page.clock.runFor(1000);
        assert.ok(
            (await drawn.boundingBox()).y < before - 20,
            "the pick stands higher",
        );
        const entries = await page.locator(".game-log-entry").allTextContents();
        assert.match(entries.at(-1), /You drew a card/);
        assert.deepEqual(await chosen(page), [
            loot.views[lootAt].status.decision.candidates.findIndex(
                (c) => c.option.kind === "activate" && c.option.slot === jaceVp,
            ),
        ]);
    },
    { fixture: "choice-loot", at: lootAt + 1, ready: slotOf(jaceVp) },
);

// Time Spiral's untap (`choice-untap`) is picked on the board: the tapped lands it may untap
// glow, wherever they sit and whoever controls them, and no others. The engine offers only
// tapped lands, so the human's own are picked to begin with, though they are fewer than it
// untaps; the opponent's, whose mana would let them answer, only by a click. Each pick turns
// gold, its count on the confirm button; picks of twins the engine offers one of go as that
// one.
const untapping = await lastDecision("choice-untap");
const untapView = untapping.views.at(-1);
const landsOf = (tapped, controller) =>
    untapView.objects
        .filter(
            (o) =>
                o.zone === "battlefield" &&
                o.types.includes("land") &&
                o.tapped === tapped &&
                (controller === undefined || o.controller === controller),
        )
        .map((o) => o.slot)
        .sort((a, b) => a - b);
const tappedLands = landsOf(true);
const untappedLands = landsOf(false);
const ownTapped = landsOf(true, "you");
const theirTapped = landsOf(true, "opponent");
const untapMax = untapping.decision.prompt.most;
assert.ok(untappedLands.length > 0, "the fixture has untapped lands");
assert.ok(
    ownTapped.length > 0 && ownTapped.length < untapMax,
    "the human's tapped lands leave room",
);
assert.ok(theirTapped.length > 0, "the opponent has tapped lands");
/** The slots of the permanents glowing `highlight`, in order. */
const glowing = (page, highlight) =>
    page.$$eval(`[data-zone="battlefield"] .card-${highlight}`, (els) =>
        [
            ...new Set(
                els.map((e) => Number(e.closest("[data-slot]").dataset.slot)),
            ),
        ].sort((a, b) => a - b),
    );
/** The first of `slots` whose card is on top where it sits, not covered by another in its
 * pile, so a click lands on it. */
const uncovered = (page, slots) =>
    page.evaluate(
        (slots) =>
            slots.find((slot) => {
                const el = document.querySelector(`[data-slot="${slot}"]`);
                const r = el.getBoundingClientRect();
                const top = document.elementFromPoint(
                    r.left + r.width / 2,
                    r.top + r.height / 2,
                );
                return top?.closest("[data-slot]") === el;
            }),
        slots,
    );
await check(
    "an untap is picked on the board, the human's tapped lands picked to begin with",
    async (page) => {
        await page.clock.runFor(2000);
        assert.equal(await page.$(".choice"), null, "no overlay opens");
        assert.deepEqual(await glowing(page, "source"), ownTapped);
        assert.deepEqual(
            await glowing(page, "target"),
            theirTapped,
            "the opponent's are not picked",
        );
        const picked = (n) => `Submit ${n}`;
        // An untapped land can't be picked.
        await page.click(slotOf(await uncovered(page, untappedLands)));
        await page
            .getByRole("button", { name: picked(ownTapped.length) })
            .waitFor();
        // A land let go glows to be picked again: of a pile, the deepest of its picks, so a
        // pile's picks stay its top cards.
        const mine = await uncovered(page, ownTapped);
        await page.click(slotOf(mine));
        const letGo = (await glowing(page, "target")).filter((slot) =>
            ownTapped.includes(slot),
        );
        assert.equal(letGo.length, 1, "one of the human's lands is let go");
        assert.equal(
            nameIn(untapView, letGo[0]),
            nameIn(untapView, mine),
            "the land let go is of the clicked land's pile",
        );
        const theirs = await uncovered(page, theirTapped);
        await page.click(slotOf(theirs));
        assert.ok((await glowing(page, "source")).includes(theirs));
        await page
            .getByRole("button", { name: picked(ownTapped.length) })
            .click();
        const [first] = await settled(page, 1);
        const o = untapping.decision.candidates[first].option;
        assert.equal(o.kind, "object", "a land is untapped first");
        assert.ok(
            ownTapped
                .filter((slot) => slot !== letGo[0])
                .some(
                    (slot) =>
                        nameIn(untapView, slot) === nameIn(untapView, o.slot),
                ),
            "one of the human's lands still picked",
        );
    },
    {
        fixture: "choice-untap",
        at: untapping.views.length,
        ready: ".prompt",
    },
);

// Teferi, Hero of Dominaria's untap at the end step (`choice-teferi`): the human's two tapped
// lands are picked to begin with, so Space untaps them.
const teferi = await lastDecision("choice-teferi");
const teferiView = teferi.views.at(-1);
const teferiTapped = teferiView.objects
    .filter(
        (o) =>
            o.zone === "battlefield" &&
            o.types.includes("land") &&
            o.tapped &&
            o.controller === "you",
    )
    .map((o) => o.slot)
    .sort((a, b) => a - b);
assert.equal(teferiTapped.length, 2, "the human has two tapped lands");
await check(
    "Teferi's untap picks the tapped lands to begin with",
    async (page) => {
        await page.clock.runFor(2000);
        // Teferi glows gold too, as what asks.
        assert.deepEqual(
            await glowing(page, "source"),
            [...teferiTapped, teferi.decision.source].sort((a, b) => a - b),
        );
        await page.getByRole("button", { name: "Submit 2" }).waitFor();
        await page.keyboard.press("Space");
        const [first] = await settled(page, 1);
        assert.ok(
            teferiTapped.includes(
                teferi.decision.candidates[first].option.slot,
            ),
            "a tapped land is untapped",
        );
    },
    { fixture: "choice-teferi", at: teferi.views.length, ready: ".prompt" },
);

// Thirst for Discovery (`choice-thirst`) asks its discard from the hand, the table in view: a
// basic land and Discard (two gestures), or two cards.
const thirstViews = await load("choice-thirst");
const thirstAt = thirstViews.findIndex(
    (v) =>
        v.status.decision?.prompt.purpose === "discard" &&
        v.status.decision.prompt.part === "instead",
);
assert.ok(thirstAt >= 0, "the fixture asks Thirst's discard");
const thirstView = thirstViews[thirstAt];
const thirstAsk = thirstView.status.decision;
const handOf = (view) =>
    view.objects
        .filter((o) => o.zone === "hand" && o.owner === "you")
        .map((o) => o.slot);
const plainsInHand = handOf(thirstView).find(
    (slot) => nameIn(thirstView, slot) === "Plains",
);
const spellsInHand = handOf(thirstView).filter(
    (slot) => !["Plains", "Island"].includes(nameIn(thirstView, slot)),
);
const thirstDiscard = thirstViews.at(-1).status.decision;
const answerIndex = (decision, kind) =>
    decision.candidates.findIndex((c) => c.option.kind === kind);
const thirstRun = {
    fixture: "choice-thirst",
    at: thirstAt + 1,
    ready: ".prompt",
};
const discardButton = (page) =>
    page.locator(".prompt .prompt-row .button", { hasText: "Submit" });
await check(
    "Thirst's basic land is discarded from the hand in two clicks",
    async (page) => {
        await page.clock.runFor(2000);
        assert.equal(await page.$(".choice"), null, "no overlay opens");
        assert.equal(await page.$(".table-dimmed"), null, "the table shows");
        const lit = await page.$$eval(
            '[data-zone="hand"] .card-target',
            (els) =>
                [
                    ...new Set(
                        els.map((e) =>
                            Number(e.closest("[data-slot]").dataset.slot),
                        ),
                    ),
                ].sort((a, b) => a - b),
        );
        assert.deepEqual(
            lit,
            [...handOf(thirstView)].sort((a, b) => a - b),
        );
        await page.click(slotOf(plainsInHand));
        await discardButton(page).click();
        assert.deepEqual(await settled(page, 2), [
            answerIndex(thirstAsk, "yes"),
            thirstDiscard.candidates.findIndex(
                (c) =>
                    c.option.kind === "object" &&
                    nameIn(thirstViews.at(-1), c.option.slot) === "Plains",
            ),
        ]);
    },
    thirstRun,
);
await check(
    "Thirst's two cards are picked from the hand before Discard is offered",
    async (page) => {
        await page.clock.runFor(2000);
        await page.click(slotOf(spellsInHand[0]));
        assert.equal(await discardButton(page).textContent(), "Submit 1");
        assert.ok(await discardButton(page).isDisabled(), "one is not enough");
        await page.click(slotOf(spellsInHand[1]));
        await discardButton(page).click();
        assert.deepEqual((await settled(page, 1)).slice(0, 1), [
            answerIndex(thirstAsk, "no"),
        ]);
    },
    thirstRun,
);

// Snapcaster Mage's target in the graveyard (`target-graveyard`) is picked by its face in the
// prompt.
const snap = await lastDecision("target-graveyard");
await check(
    "graveyard targets show their faces",
    async (page) => {
        await page.clock.runFor(2000);
        const offered = await page.$$eval(".prompt-card", (els) =>
            els.map((e) => Number(e.dataset.option)),
        );
        assert.deepEqual(
            offered.sort((a, b) => a - b),
            [...snap.bySlot.keys()].sort((a, b) => a - b),
        );
        const names = await page
            .locator(".prompt-card .card-name")
            .allTextContents();
        assert.deepEqual(
            names.sort(),
            [...snap.bySlot.keys()]
                .map((slot) => nameIn(snap.views.at(-1), slot))
                .sort(),
            "each shows its card",
        );
        assert.equal(
            await page.$(".prompt-choices"),
            null,
            "no name-only buttons",
        );
        const [first] = offered;
        // Each is shown whole on hover.
        await page.hover(`.prompt-card[data-option="${first}"]`);
        await inspectorOf(page, nameIn(snap.views.at(-1), first));
        await page.click(`.prompt-card[data-option="${first}"]`);
        assert.deepEqual(await settled(page, 1), [snap.bySlot.get(first)]);
    },
    { fixture: "target-graveyard", at: snap.views.length, ready: ".prompt" },
);

// Trinket Mage's search (`choice-search`) finds up to one of two artifacts: a second click
// lights the second card in place of the first, never both, and Find sends the one lit.
const searching = await lastDecision("choice-search");
assert.equal(searching.decision.prompt.most, 1, "the search finds up to one");
assert.ok(
    searching.views.every((v) => v.status.decision?.kind !== "yesNo"),
    "the smart stops search without asking",
);
await check(
    "a search finds nothing from its second button",
    async (page) => {
        await page.clock.runFor(2000);
        await page.getByRole("button", { name: "Decline" }).click();
        assert.deepEqual(await settled(page, 1), [
            answerIndex(searching.decision, "done"),
        ]);
    },
    {
        fixture: "choice-search",
        at: searching.views.length,
        ready: ".choice",
    },
);
const [crypt, converter] = [...searching.bySlot.keys()];
await check(
    "an up-to-one search lights one card and sends it",
    async (page) => {
        await page.clock.runFor(2000);
        await page.click(tray(crypt));
        await page.click(tray(converter));
        const lit = await page.$$eval(
            '.choice-zone [aria-pressed="true"]',
            (cards) => cards.map((c) => Number(c.dataset.slot)),
        );
        assert.deepEqual(lit, [converter], "one card is lit, the last");
        await page
            .getByRole("button", { name: "Submit 1", exact: true })
            .click();
        assert.deepEqual(
            await settled(page, 2),
            [searching.bySlot.get(converter)],
            "Find sends one pick",
        );
    },
    {
        fixture: "choice-search",
        at: searching.views.length,
        ready: tray(crypt),
    },
);

await check(
    "ability frames have square tops and shield bottoms",
    async (page) => {
        await page.clock.runFor(1000);
        const corners = await page
            .locator(".stack-item")
            .first()
            .evaluate((e) => {
                const s = getComputedStyle(e);
                return [s.borderTopLeftRadius, s.borderBottomLeftRadius];
            });
        assert.equal(parseFloat(corners[0]), 0);
        assert.ok(
            parseFloat(corners[1]) >= 20,
            `rounded shield bottom ${corners[1]}`,
        );
    },
    {
        fixture: "arrow-ability",
        at: (await load("arrow-ability")).length,
        ready: ".stack-item",
    },
);

// The cleanup discard (`choice-cleanup`: eight cards, one over) is picked from the raised hand,
// its picks counted on its button.
const cleanup = await lastDecision("choice-cleanup");
const [extra] = [...cleanup.bySlot.keys()];
const discardBesideStack = "the raised discard hand stays clear of the stack";
if (wanted(discardBesideStack)) {
    const view = cleanup.views.at(-1);
    const resolving = {
        kind: "ability",
        source: null,
        name: "Currency Converter",
        controller: "you",
        triggered: false,
        ability: 1,
        granted: false,
        nth: 0,
        targets: [],
        x: 0,
    };
    const run = await motionFixture("discard-beside-stack", [
        {
            ...view,
            status: {
                ...view.status,
                decision: { ...view.status.decision, resolving },
            },
            events: [],
        },
    ]);
    await check(
        discardBesideStack,
        async (page) => {
            await page.clock.runFor(1500);
            const stack = await page.locator(".stack-ability").boundingBox();
            const hand = page.locator(
                '.card-el-you[data-zone="hand"][data-slot]',
            );
            assert.equal(await hand.count(), cleanup.bySlot.size);
            const boxes = await hand.evaluateAll((cards) =>
                cards.map((card) => ({
                    slot: card.dataset.slot,
                    right: card
                        .querySelector(".card-body")
                        .getBoundingClientRect().right,
                })),
            );
            const edge = boxes.sort((a, b) => b.right - a.right)[0];
            assert.ok(
                edge.right < stack.x,
                "the whole hand clears the ability",
            );
            await page.locator(slotOf(edge.slot)).click();
            assert.ok(await discardButton(page).isEnabled());
        },
        { ...run, ready: ".prompt", viewport: { width: 1728, height: 963 } },
    );
}
const inHand = (slot) => `[data-slot="${slot}"][data-zone="hand"]`;
await check(
    "the cleanup discard counts its picks",
    async (page) => {
        await page.getByRole("button", { name: "Submit 0" }).waitFor();
        assert.equal(await page.$(".choice"), null, "no overlay opens");
        await page.clock.runFor(1500);
        await page.click(inHand(extra));
        await page.getByRole("button", { name: "Submit 1" }).click();
        assert.deepEqual(await settled(page, 1), [cleanup.bySlot.get(extra)]);
    },
    {
        fixture: "choice-cleanup",
        at: cleanup.views.length,
        ready: inHand(extra),
    },
);

// Every choice overlay the fixtures hold keeps its words in their places: the title above
// the header's first rule, each label in its button.
for (const fixture of [
    "choice-scry",
    "choice-jace",
    "choice-explore",
    "choice-search",
]) {
    const { views } = await lastDecision(fixture);
    await check(
        `${fixture}'s choice text fits its box`,
        async (page) => {
            await page.evaluate(() => document.fonts.ready.then(() => {}));
            await page.clock.runFor(2000);
            const misfits = await page.evaluate(() => {
                const header = document
                    .querySelector(".choice .overlay-header")
                    .getBoundingClientRect();
                const out = [];
                const within = (selector, top, bottom) => {
                    const e = document.querySelector(`.choice ${selector}`);
                    const text = e?.textContent.trim() ?? "";
                    if (!text) return;
                    const r = e.getBoundingClientRect();
                    if (
                        r.top < header.top + top - 1 ||
                        r.bottom > header.top + bottom + 1
                    )
                        out.push(
                            `${selector} "${text}" spans ${r.top - header.top}..${r.bottom - header.top}`,
                        );
                };
                // The header's first rule lies 68 pixels down (components.css).
                within(".overlay-title", -8, 68);
                for (const b of document.querySelectorAll(".choice button")) {
                    if (
                        b.scrollWidth > b.clientWidth + 1 ||
                        b.scrollHeight > b.clientHeight + 1
                    )
                        out.push(`button "${b.textContent}" overflows`);
                }
                return out;
            });
            assert.deepEqual(misfits, [], "choice text out of its box");
        },
        { fixture, at: views.length, ready: ".choice" },
    );
}

// Adeline's trigger (`attack-token`) asks what its Human attacks with the Human on the table,
// attacking the player; the turn button keeps that, and clicking The Wandering Emperor turns
// it on her.
const tokenAim = await lastDecision("attack-token");
const human = named(tokenAim.views.at(-1), "Human Token")[0];
const emperor = named(tokenAim.views.at(-1), "The Wandering Emperor")[0];
assert.ok(
    human !== undefined,
    "the Human is on the table as it is asked about",
);
const defending = (walker) =>
    tokenAim.decision.candidates.findIndex(
        (c) => c.option.kind === "defender" && c.option.planeswalker === walker,
    );
const adelineRun = {
    fixture: "attack-token",
    at: tokenAim.views.length,
    ready: slotOf(human),
};
await check(
    "Adeline's Human is in view as what it attacks is asked",
    async (page) => {
        await page.clock.runFor(2000);
        assert.ok(
            await page.$(`${slotOf(emperor)} .card-target`),
            "the planeswalker glows",
        );
        await page.click(slotOf(emperor));
        assert.deepEqual(await settled(page, 1), [defending(emperor)]);
    },
    adelineRun,
);
await check(
    "Adeline's Human requires a highlighted direct attack target",
    async (page) => {
        await page.clock.runFor(2000);
        assert.equal(
            await page
                .getByRole("button", { name: "Attack", exact: true })
                .count(),
            0,
        );
        assert.ok(await page.locator(`${slotOf(human)} .card-source`).count());
        assert.equal(
            await page.locator(`.arrow[data-from="card-${human}"]`).count(),
            0,
            "an undecided attacker has no destination link",
        );
        assert.equal(
            await page
                .getByRole("heading", {
                    name: "Choose attack target for Human",
                    exact: true,
                })
                .count(),
            1,
        );
        await page
            .getByRole("button", { name: "Target opponent", exact: true })
            .click();
        assert.deepEqual(await settled(page, 1), [defending(null)]);
    },
    adelineRun,
);

const combatLink = (slot) => `.combat-link[data-from="card-${slot}"]`;

const tokenHandoff = "declared attack lines survive a token's defender choice";
if (wanted(tokenHandoff)) {
    const adeline = named(
        tokenAim.views.at(-1),
        "Adeline, Resplendent Cathar",
    )[0];
    const start = tokenAim.views.findIndex(
        (v) => v.status.decision?.kind === "attacker",
    );
    const [declaration, defender] = tokenAim.views.slice(start);
    const pending = structuredClone(tokenAim.views.at(-1));
    // Playground batches declaration and trigger resolution into the Human's question.
    pending.events = tokenAim.views.slice(start + 2).flatMap((v) => v.events);
    pending.objects.find((o) => o.slot === adeline).combat.planeswalker =
        emperor;
    const answered = structuredClone(pending);
    answered.events = [];
    answered.pendingAttackers = undefined;
    answered.status = { kind: "playing" };
    const run = await motionFixture("attack-token-handoff", [
        declaration,
        defender,
        pending,
        answered,
    ]);
    await check(
        tokenHandoff,
        async (page) => {
            const line = page.locator(combatLink(adeline));
            const tokenLine = page.locator(combatLink(human));
            await page.click(slotOf(adeline));
            await page.click(slotOf(emperor));
            const destination = `card-${emperor}`;
            assert.equal(await line.getAttribute("data-to"), destination);
            await page.evaluate(
                ({ existing, token, destination }) => {
                    const frames = { wrong: [], sawDeclaration: false };
                    let next;
                    const sample = () => {
                        const links = [...document.querySelectorAll(existing)];
                        const ends = links.map((link) => link.dataset.to);
                        const tokenLinks =
                            document.querySelectorAll(token).length;
                        if (
                            ends.length !== 1 ||
                            ends[0] !== destination ||
                            tokenLinks
                        )
                            frames.wrong.push({ ends, tokenLinks });
                        const shown = window.berylShown.at(-1).update.view;
                        frames.sawDeclaration ||= shown.events.some(
                            (event) => event.kind === "attack",
                        );
                        next = requestAnimationFrame(sample);
                    };
                    window.finishAttackFrames = () => {
                        cancelAnimationFrame(next);
                        return frames;
                    };
                    next = requestAnimationFrame(sample);
                },
                {
                    existing: combatLink(adeline),
                    token: combatLink(human),
                    destination,
                },
            );
            await page
                .getByRole("button", { name: "Attack", exact: true })
                .click();
            await until(page, ':text-is("Choose attack target for Human")');
            const frames = await page.evaluate(() =>
                window.finishAttackFrames(),
            );
            assert.ok(frames.sawDeclaration, "sampled the declaration beat");
            assert.deepEqual(
                frames.wrong,
                [],
                "only the declared line in every frame",
            );
            await page
                .getByRole("button", { name: "Target opponent", exact: true })
                .click();
            await page.clock.runFor(500);
            assert.equal(await line.getAttribute("data-to"), `card-${emperor}`);
            assert.equal(
                await tokenLine.getAttribute("data-to"),
                "opponent-life-orb",
            );
        },
        { ...run, ready: slotOf(adeline) },
    );
}

async function chevronGap(page, slot) {
    return page.locator(`${combatLink(slot)} path`).evaluate((path, slot) => {
        const mark = document.querySelector(`#card-${slot}-mark`);
        const tip = new DOMPoint(50, 0.5).matrixTransform(mark.getScreenCTM());
        const start = path
            .getPointAtLength(0)
            .matrixTransform(path.getScreenCTM());
        return Math.hypot(tip.x - start.x, tip.y - start.y);
    }, slot);
}

const regroup =
    "attack targeting previews immediately, follows moving chevrons, and regroups";
if (wanted(regroup)) {
    const v = structuredClone(pastWalker[attackAt]);
    const original = attacking.candidates[attacker];
    const first = original.option.slot;
    const second = Math.max(...v.objects.map((o) => o.slot)) + 1;
    const copy = { ...v.objects.find((o) => o.slot === first), slot: second };
    const walker = named(v, "The Wandering Emperor")[0];
    v.objects.push(copy);
    v.battlefield.push(second);
    v.status.decision.candidates.push({
        ...original,
        option: { kind: "attack", slot: second },
    });
    const run = await motionFixture("attack-retarget-group", [v]);
    await check(
        regroup,
        async (page) => {
            await page.clock.runFor(600);
            const links = (slot) => page.locator(combatLink(slot));
            const target = (slot) => links(slot).getAttribute("data-to");
            const walkerId = `card-${walker}`;
            const player = page.getByRole("button", {
                name: "Attack opponent",
                exact: true,
            });
            assert.ok(
                await page
                    .getByText("Choose attackers, then a target")
                    .isVisible(),
            );
            await page.clock.pauseAt(Date.now() + 60_000);
            await page.click(slotOf(first));
            assert.equal(
                await links(first).count(),
                1,
                "selection draws the line immediately",
            );
            const mark = page.locator(`#card-${first}-mark`);
            const [, startY] = await where(mark);
            for (let i = 0; i < 20; i++) {
                await page.clock.runFor(16);
                const gap = await chevronGap(page, first);
                assert.ok(
                    gap < 3,
                    `line tracks the tip during the glide (gap ${gap}px)`,
                );
            }
            const [, endY] = await where(mark);
            assert.ok(
                startY - endY > 10,
                "the sampled frames include the card's glide",
            );
            await page.click(slotOf(second));
            await page.click(slotOf(walker));
            assert.deepEqual(
                await Promise.all([target(first), target(second)]),
                [walkerId, walkerId],
            );
            await player.click();
            assert.deepEqual(
                await Promise.all([target(first), target(second)]),
                ["opponent-life-orb", "opponent-life-orb"],
            );
            await page.click(slotOf(walker));
            assert.deepEqual(
                await Promise.all([target(first), target(second)]),
                [walkerId, walkerId],
            );
            await page.click(slotOf(first));
            await player.click();
            assert.deepEqual(
                await Promise.all([target(first), target(second)]),
                ["opponent-life-orb", walkerId],
            );
            await page.click(slotOf(walker));
            assert.deepEqual(
                await Promise.all([target(first), target(second)]),
                [walkerId, walkerId],
            );
            await page
                .getByRole("button", { name: "Clear", exact: true })
                .click();
            await page
                .getByRole("button", { name: "Attack With All", exact: true })
                .click();
            await page.click(slotOf(walker));
            assert.deepEqual(
                await Promise.all([target(first), target(second)]),
                [walkerId, walkerId],
            );
        },
        { ...run, ready: slotOf(first) },
    );
}

const ordering =
    "attack lines sort by destination and stay present through blockers";
if (wanted(ordering)) {
    const v = structuredClone(tokenAim.views.at(-1));
    const first = named(v, "Adeline, Resplendent Cathar")[0];
    v.pendingAttackers = undefined;
    const a = v.objects.find((o) => o.slot === first);
    const b = v.objects.find((o) => o.slot === human);
    // The fixture's first attacker stands left of the Human before sorting.
    a.combat = { kind: "attacking", planeswalker: emperor, blocked: false };
    b.combat = { kind: "attacking", planeswalker: null, blocked: false };
    v.status.decision.prompt = asking(
        v.status.decision.prompt,
        "pick",
        "priority",
    );
    v.status.decision.kind = "priority";
    v.status.decision.candidates = [
        { ...v.status.decision.candidates[0], option: { kind: "pass" } },
    ];
    const blockers = structuredClone(v);
    blockers.turn.step = "declareBlockers";
    blockers.status = { kind: "playing" };
    blockers.events = [];
    a.tapped = false;
    blockers.objects.find((o) => o.slot === first).tapped = true;
    const run = await motionFixture("attack-continuity", [v, blockers]);
    await check(
        ordering,
        async (page) => {
            const ordered = () =>
                page.evaluate(() =>
                    [...document.querySelectorAll(".combat-link path")]
                        .map((p) => [
                            p.getPointAtLength(0).x,
                            p.getPointAtLength(p.getTotalLength()).x,
                        ])
                        .sort((a, b) => a[0] - b[0]),
                );
            await page.clock.runFor(800);
            const before = await ordered();
            assert.equal(before.length, 2);
            assert.ok(
                before[0][1] < before[1][1],
                "source and target order agree",
            );
            await page.keyboard.press("Space");
            for (let i = 0; i < 22; i++) {
                await page.clock.runFor(16);
                assert.equal(
                    (await ordered()).length,
                    2,
                    "both lines survive the blockers transition",
                );
            }
            assert.equal(
                await page.evaluate(
                    () => window.berylShown.at(-1).update.view.turn.step,
                ),
                "declareBlockers",
            );
        },
        { ...run, ready: slotOf(first) },
    );
}

const departingWalker =
    "attacks lose only the link to a departing planeswalker";
if (wanted(departingWalker)) {
    for (const attackingSide of ["you", "opponent"]) {
        const before = structuredClone(tokenAim.views.at(-1));
        const attacker = before.objects.find((o) => o.slot === human);
        const walker = before.objects.find((o) => o.slot === emperor);
        if (attackingSide === "opponent") {
            for (const o of before.objects) {
                o.controller = o.controller === "you" ? "opponent" : "you";
                o.owner = o.owner === "you" ? "opponent" : "you";
            }
            [before.players.you, before.players.opponent] = [
                before.players.opponent,
                before.players.you,
            ];
        }
        const extraSlot = Math.max(...before.objects.map((o) => o.slot)) + 1;
        const otherWalker = { ...walker, slot: extraSlot };
        attacker.combat = {
            kind: "attacking",
            planeswalker: emperor,
            blocked: false,
        };
        const atPlayer = {
            ...attacker,
            slot: extraSlot + 1,
            combat: { ...attacker.combat, planeswalker: null },
        };
        const atOtherWalker = {
            ...attacker,
            slot: extraSlot + 2,
            combat: { ...attacker.combat, planeswalker: otherWalker.slot },
        };
        before.objects.push(otherWalker, atPlayer, atOtherWalker);
        before.battlefield.push(
            otherWalker.slot,
            atPlayer.slot,
            atOtherWalker.slot,
        );
        before.pendingAttackers = undefined;
        before.events = [];
        before.turn.active = attackingSide;
        before.turn.step = "combatDamage";
        before.status.decision.prompt = asking(
            before.status.decision.prompt,
            "pick",
            "priority",
        );
        before.status.decision.kind = "priority";
        before.status.decision.candidates = [
            {
                ...before.status.decision.candidates[0],
                option: { kind: "pass" },
            },
        ];
        const after = structuredClone(before);
        after.turn.step = "endOfCombat";
        after.status = { kind: "playing" };
        after.battlefield = after.battlefield.filter(
            (slot) => slot !== emperor,
        );
        after.objects.find((o) => o.slot === emperor).zone = "graveyard";
        after.players[walker.owner].graveyard.push(emperor);
        after.objects.find((o) => o.slot === human).combat = {
            ...attacker.combat,
            planeswalker: null,
            targetDeparted: true,
        };
        after.events = [
            {
                kind: "zoneChange",
                slot: emperor,
                from: "battlefield",
                to: "graveyard",
                reason: "stateBased",
            },
            { kind: "stepBegins", step: "endOfCombat", active: attackingSide },
        ];
        const run = await motionFixture(`departing-walker-${attackingSide}`, [
            before,
            after,
        ]);
        await check(
            `${departingWalker} from the ${attackingSide} seat`,
            async (page) => {
                const target = (slot) =>
                    page.locator(combatLink(slot)).getAttribute("data-to");
                await page.clock.runFor(800);
                assert.equal(await target(human), `card-${emperor}`);
                await page.keyboard.press("Space");
                const departing = page.locator(combatLink(human));
                const walkerCard = page.locator(slotOf(emperor));
                for (let i = 0; i < 55; i++) {
                    await page.clock.runFor(16);
                    if (
                        (await walkerCard.getAttribute("data-zone")) ===
                        "graveyard"
                    ) {
                        assert.equal(
                            await departing.count(),
                            0,
                            "no link during the graveyard flight",
                        );
                    } else if (await departing.count()) {
                        assert.equal(
                            await target(human),
                            `card-${emperor}`,
                            "never retarget to the player",
                        );
                    }
                    assert.equal(
                        await target(atPlayer.slot),
                        `${walker.controller}-life-orb`,
                    );
                    assert.equal(
                        await target(atOtherWalker.slot),
                        `card-${otherWalker.slot}`,
                    );
                }
                assert.equal(await page.locator(combatLink(human)).count(), 0);
                assert.equal(
                    await page
                        .locator(slotOf(emperor))
                        .getAttribute("data-zone"),
                    "graveyard",
                );
            },
            { ...run, ready: combatLink(human) },
        );
    }
}

const observerLink =
    "the defending seat draws only the resolved token attack destination";
if (wanted(observerLink)) {
    const waiting = structuredClone(tokenAim.views.at(-1));
    waiting.status = { kind: "thinking" };
    for (const o of waiting.objects) {
        o.controller = o.controller === "you" ? "opponent" : "you";
        o.owner = o.owner === "you" ? "opponent" : "you";
    }
    const chosen = structuredClone(waiting);
    chosen.pendingAttackers = undefined;
    chosen.objects.find((o) => o.slot === human).combat.planeswalker = emperor;
    const run = await motionFixture("observed-token-target", [waiting, chosen]);
    const line = combatLink(human);
    await check(
        observerLink,
        async (page) => {
            await page.clock.runFor(800);
            assert.equal(
                await page.locator(line).count(),
                0,
                "no provisional attack on the life total",
            );
        },
        { ...run, ready: `#card-${human}-mark` },
    );
    await check(
        observerLink + " once chosen",
        async (page) => {
            const link = page.locator(line);
            assert.equal(await link.getAttribute("data-to"), `card-${emperor}`);
            assert.ok(
                (await chevronGap(page, human)) < 3,
                "line starts at the reversed chevron tip",
            );
        },
        { ...run, at: 2, ready: line },
    );
}

// Resolved destinations use the same quiet geometry as blocking links, from either seat.
if (wanted("attack destinations use thin lines")) {
    for (const seat of ["you", "opponent"]) {
        const v = structuredClone(tokenAim.views.at(-1));
        v.status = { kind: "playing" };
        v.pendingAttackers = undefined;
        v.events = [];
        if (seat === "opponent") {
            for (const o of v.objects) {
                o.controller = o.controller === "you" ? "opponent" : "you";
                o.owner = o.owner === "you" ? "opponent" : "you";
            }
        }
        for (const walker of [null, emperor]) {
            v.objects.find((o) => o.slot === human).combat.planeswalker =
                walker;
            const run = await motionFixture(`attack-line-${seat}-${walker}`, [
                structuredClone(v),
            ]);
            await check(
                `attack destinations use thin lines: ${seat} ${walker ?? "player"}`,
                async (page) => {
                    await page.clock.runFor(2000);
                    const link = page.locator(
                        `.arrow[data-from="card-${human}"]`,
                    );
                    assert.equal(await link.count(), 1);
                    assert.equal(
                        await link.locator("path").count(),
                        1,
                        "no arrowhead or glow",
                    );
                    const width = await link
                        .locator("path")
                        .evaluate((e) =>
                            parseFloat(getComputedStyle(e).strokeWidth),
                        );
                    assert.ok(width <= 2, `subtle line width ${width}`);
                },
                { ...run, ready: slotOf(human) },
            );
        }
    }
}

// Crewing (`crew`: Swift Reconfiguration's crew 5) is picked on the board, a meter of pips
// showing the power tapped against the cost, and what the creature pointed at would add.
const crewViews = await load("crew");
const crewAt = crewViews.findIndex(
    (v) => v.status.decision?.prompt.purpose === "crew",
);
const crewing = crewViews[crewAt].status.decision;
const crewer = (name) =>
    crewing.candidates.find((c) => c.label === name).option.slot;
const crewMeter = (page) => page.getByRole("meter", { name: "Crew" });
await check(
    "crewing shows the power picked against the crew cost without text",
    async (page) => {
        await page.clock.runFor(2000);
        const meter = crewMeter(page);
        assert.deepEqual(
            [
                await meter.getAttribute("aria-valuenow"),
                await meter.getAttribute("aria-valuemax"),
                (await meter.innerText()).trim(),
            ],
            ["0", "5", ""],
        );
        await page.hover(slotOf(crewer("Elite Spellbinder")));
        assert.equal(await page.locator(".crew-pip-next").count(), 3);
        const snapcaster = crewer("Snapcaster Mage");
        await page.click(slotOf(snapcaster));
        assert.deepEqual(await settled(page, 1), [
            crewing.candidates.findIndex(
                (c) =>
                    c.option.kind === "object" && c.option.slot === snapcaster,
            ),
        ]);
    },
    { fixture: "crew", at: crewAt + 1, ready: ".crew-meter" },
);
await check(
    "the crew meter fills with the power tapped",
    async (page) => {
        await page.clock.runFor(2000);
        assert.equal(await crewMeter(page).getAttribute("aria-valuenow"), "2");
        assert.equal(await page.locator(".crew-pip-on").count(), 2);
    },
    { fixture: "crew", at: crewAt + 2, ready: ".crew-meter" },
);

// Auto Pay previews on hover, with no selected permanents until the player picks. The
// session holds the payment: a pick and the button are clicks it answers, so each fixture
// here is `choice-scry` with the replies its clicks get.
const scryViews = await load("choice-scry");
const bendAt = scryViews.findIndex(
    (v) => v.status.decision?.prompt.purpose === "waterbend",
);
assert.ok(bendAt >= 0, "waterbend opens a payment review");
const bending = scryViews[bendAt].status.decision;
const markedBy = (prompt, mark) =>
    prompt.marks
        .filter((m) => m.mark === mark && m.on.kind === "object")
        .map((m) => m.on.slot);
const suggestedTaps = markedBy(bending.prompt, "suggested");
assert.ok(suggestedTaps.length > 0, "the payment opens with a default");
/** `choice-scry` with `replies` to the clicks the page sends, in order. */
async function bendFixture(name, replies) {
    const fixture = JSON.parse(
        await readFile(join(fixtures, "choice-scry.json"), "utf8"),
    );
    fixture.messages.unshift(
        ...replies.map((value) => ({
            kind: "response",
            command: "click",
            value,
        })),
    );
    await writeFixture(name, fixture);
}
/** The clicks the page has sent, as recorded by the fixture transport. */
const clicksSent = (page) =>
    page.evaluate(() =>
        window.berylFixture.calls
            .filter((c) => c.command === "click")
            .map((c) => c.args.request.click),
    );
/** The clicks sent once there are `count` of them, or as many as there are after a while. */
async function clicked(page, count) {
    for (let i = 0; i < 20 && (await clicksSent(page)).length < count; i++) {
        await page.clock.runFor(100);
    }
    return clicksSent(page);
}
const AUTO = { kind: "button", button: { kind: "auto" } };
const autoPayButton = (page) =>
    page.getByRole("button", { name: "Auto Pay", exact: true });
const autoBend =
    "waterbend Auto Pay highlights only on hover and accepts on click";
await bendFixture("waterbend-auto", [{ kind: "answered" }]);
await check(
    autoBend,
    async (page) => {
        await page.clock.runFor(2000);
        const autoPay = autoPayButton(page);
        assert.ok(await autoPay.isEnabled());
        assert.deepEqual(await lit(page), []);
        assert.deepEqual(await glowing(page, "target"), []);
        await autoPay.hover();
        await committed(page);
        assert.deepEqual(await lit(page), ascending(suggestedTaps));
        assert.deepEqual(await chosen(page), []);
        await page.mouse.move(0, 0);
        await committed(page);
        assert.deepEqual(await lit(page), []);
        await autoPay.click();
        assert.deepEqual(await clicked(page, 1), [AUTO]);
        await until(page, ".choice");
        assert.deepEqual(await chosen(page), [], "the session sends the picks");
    },
    { fixture: "waterbend-auto", at: bendAt + 1, ready: ".prompt-cost" },
);

const bendCost = "waterbend prompt renders its generic mana symbol";
if (wanted(bendCost)) {
    for (const fixture of ["choice-scry", "waterbend-tap"]) {
        const views = await load(fixture);
        const at = views.findIndex(
            (v) => v.status.decision?.prompt.purpose === "waterbend",
        );
        const cost = views[at].status.decision.prompt.cost;
        await check(
            `${bendCost} (${fixture})`,
            async (page) => {
                const symbol = page.locator(".prompt-cost").getByRole("img", {
                    name: cost,
                    exact: true,
                });
                assert.equal(
                    await symbol.count(),
                    1,
                    `${fixture} should display ${cost}`,
                );
                assert.ok(await symbol.isVisible());
            },
            { fixture, at: at + 1, ready: ".prompt-cost" },
        );
    }
}

const mixedBend = "waterbend Auto Pay keeps manual picks";
if (wanted(mixedBend)) {
    const bender = bending.candidates.find((c) => c.option.kind === "object")
        .option.slot;
    assert.ok(
        markedBy(bending.prompt, "eligible").includes(bender),
        "the creature can be picked",
    );
    // The creature picked pays one; the default taps two lands for the rest.
    const rest = suggestedTaps.slice(0, 2);
    const mark = (slot, mark) => ({ on: { kind: "object", slot }, mark });
    const mixed = {
        ...bending.prompt,
        revision: 1,
        marks: [
            mark(bender, "paying"),
            ...rest.map((slot) => mark(slot, "suggested")),
        ],
    };
    const reply = (prompt, revision) => ({
        kind: "open",
        prompt: { ...prompt, revision },
    });
    await bendFixture("waterbend-mixed", [
        reply(mixed, 1),
        reply(bending.prompt, 2),
        reply(mixed, 3),
        { kind: "answered" },
    ]);
    const ON = { kind: "on", on: { kind: "object", slot: bender } };
    await check(
        mixedBend,
        async (page) => {
            const autoPay = autoPayButton(page);
            const permanent = page.locator(slotOf(bender));
            const marked = permanent.locator(".waterbend-tap");
            await permanent.click();
            await marked.waitFor();
            assert.deepEqual(await lit(page), [bender]);
            await autoPay.hover();
            assert.deepEqual(await lit(page), ascending([bender, ...rest]));
            await permanent.click();
            await marked.waitFor({ state: "detached" });
            assert.deepEqual(await lit(page), []);
            await permanent.click();
            await marked.waitFor();
            await autoPay.click();
            assert.deepEqual(await clicked(page, 4), [ON, ON, ON, AUTO]);
            assert.deepEqual(
                await chosen(page),
                [],
                "the session sends the picks",
            );
        },
        { fixture: "waterbend-mixed", at: bendAt + 1, ready: ".prompt-cost" },
    );
}

await check(
    "phone waterbend long press previews without accepting",
    async (page) => {
        const autoPay = autoPayButton(page);
        assert.deepEqual(await lit(page), []);
        await autoPay.dispatchEvent("pointerdown", { pointerType: "touch" });
        await page.clock.runFor(500);
        assert.deepEqual(await lit(page), ascending(suggestedTaps));
        await autoPay.dispatchEvent("pointerup", { pointerType: "touch" });
        await autoPay.dispatchEvent("click", { detail: 1 });
        assert.deepEqual(await lit(page), []);
        assert.deepEqual(await clicksSent(page), []);
        await autoPay.tap();
        assert.deepEqual(await clicked(page, 1), [AUTO]);
        assert.deepEqual(await chosen(page), []);
    },
    {
        fixture: "waterbend-auto",
        at: bendAt + 1,
        ready: ".phone-dock",
        phone: true,
        viewport: { width: 390, height: 844 },
    },
);

// The opening: `opening-bottom` ends on the human's first card to put on the bottom after two
// mulligans and a keep. The picks fill two slots and go back on a click; nothing is sent until
// both are chosen and confirmed, and then the first goes at once.
const openingViews = await load("opening-bottom");
const bottoming = openingViews.at(-1);
assert.equal(bottoming.status.decision.prompt.purpose, "put-on-bottom");
assert.equal(bottoming.players.you.mulligans, 2, "two cards go on the bottom");
await check(
    "cards for the bottom toggle in their slots until confirmed",
    async (page) => {
        const hand = page.locator(".opening-hand .opening-card");
        const slotted = page.locator(".opening-slots .opening-card");
        const done = page.getByRole("button", { name: "Done" });
        assert.equal(await page.locator(".opening-slot").count(), 2);
        const first = await hand.first().getAttribute("data-slot");
        await hand.first().click();
        assert.equal(await slotted.count(), 1, "a pick fills a slot");
        assert.equal(await done.isDisabled(), true, "one slot is still empty");
        await slotted.first().click();
        assert.equal(await slotted.count(), 0, "a second click takes it back");
        assert.equal(await hand.count(), 7);
        await hand.nth(2).click();
        await hand.nth(4).click();
        assert.equal(await slotted.count(), 2);
        assert.equal(await done.isEnabled(), true);
        assert.deepEqual(await chosen(page), [], "nothing sent before Done");
        await done.click();
        const [sent] = await settled(page, 1);
        const option = bottoming.status.decision.candidates[sent].option;
        const picked = await slotted.first().getAttribute("data-slot");
        const name = (slot) =>
            bottoming.objects.find((o) => o.slot === Number(slot)).name;
        assert.notEqual(picked, first, "the card taken back stays in hand");
        assert.equal(name(option.slot), name(picked), "the first pick is sent");
    },
    {
        fixture: "opening-bottom",
        at: openingViews.length,
        ready: ".opening-slot",
    },
);

// The start screen: Enter starts the game, and the opponent picked is offered again after a
// reload.
const startCalls = (page) =>
    page.evaluate(() =>
        window.berylFixture.calls.filter((c) => c.command === "start_game"),
    );
await check(
    "Enter on the start screen starts a game",
    async (page) => {
        await page.keyboard.press("Enter");
        await page.waitForURL(/#\/game$/);
        assert.equal((await startCalls(page)).length, 1);
    },
    { at: 0, hash: "#/", ready: ".start-agent input:checked" },
);
await check(
    "Enter with a game in progress returns to it",
    async (page) => {
        await page.getByRole("button", { name: "Return to Game" }).waitFor();
        await page.keyboard.press("Enter");
        await page.waitForURL(/#\/game$/);
        assert.equal((await startCalls(page)).length, 0, "no new game");
    },
    { at: 50, hash: "#/", ready: ".start-agent input:checked" },
);
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
    { at: 0, hash: "#/", ready: ".start-agent input:checked" },
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
const lifeShown = async (page) =>
    Number(
        (
            await page
                .locator('[data-life="you"] .life-orb-total')
                .textContent()
        ).replace("\u2212", "-"),
    );
await check(
    "the life-loss result waits for the killing blow and can be put aside",
    async (page) => {
        await page.clock.runFor(2000);
        await page.keyboard.press(" ");
        const result = page.getByRole("dialog", { name: "Defeat" });
        // When the damage shows, and when the view of the end does (its log line).
        let [lost, ended] = [undefined, undefined];
        let t = 0;
        await committed(page);
        for (; t < 60 && !(await result.count()); t++) {
            lost ??= (await lifeShown(page)) < 0 ? t : undefined;
            const log = await page.locator(".game-log-entry").allTextContents();
            ended ??= log.some((e) => e.includes("lost the game"))
                ? t
                : undefined;
            await page.clock.runFor(100);
            await committed(page);
        }
        assert.ok(await result.count(), "the result shows");
        assert.ok(lost !== undefined && lost < t, "the damage shows first");
        // The end's events (the loss) hold it as long as an action (playback.ts' HOLD).
        assert.ok(
            ended !== undefined && (t - ended) * 100 >= 800,
            `the end plays first: ${ended}, then ${t}`,
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
    overAt,
);

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
        { ...overAt, fixture, at: overViews.length },
    );
}

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

// A reloaded page asks for the game in progress (`current_game`) and takes it up: here the
// fixture records no views, only that answer, the human's last pass in `game-over`.
await writeFixture("game-over-resumed", {
    messages: [
        ...overFixture.messages.filter((m) => m.kind === "response"),
        {
            kind: "response",
            command: "current_game",
            value: overEvents[lastPass].payload,
        },
    ],
});
await check(
    "a reloaded page takes up the game in progress",
    async (page) => {
        assert.equal(await lifeShown(page), 3);
        assert.equal(await page.locator(".start-panel").count(), 0);
        await page.keyboard.press(" ");
        const chosen = await settled(page, 1);
        assert.deepEqual(chosen, [0], "the open decision is answered");
    },
    { fixture: "game-over-resumed", at: 0, ready: "[data-life]" },
);

const pileAlignment = "zone pile faces share a bottom edge";
if (wanted(pileAlignment)) {
    const spare = (await load("cast-exile-choice"))[0].objects.find(
        (o) => o.name === "Thirst for Discovery" && o.zone === "graveyard",
    );
    for (const side of ["you", "opponent"]) {
        for (const graveyardCount of [0, 1]) {
            const view = structuredClone(oculusViews[1]);
            view.events = [];
            if (graveyardCount) {
                const slot = Math.max(...view.objects.map((o) => o.slot)) + 1;
                view.objects.push({ ...spare, slot });
                view.players.you.graveyard.push(slot);
            }
            if (side === "opponent") {
                [view.players.you, view.players.opponent] = [
                    view.players.opponent,
                    view.players.you,
                ];
                for (const o of view.objects) {
                    o.owner = o.owner === "you" ? "opponent" : "you";
                    o.controller = o.controller === "you" ? "opponent" : "you";
                }
            }
            const { library, graveyard } = view.players[side];
            const exile = view.objects
                .filter((o) => o.zone === "exile" && o.owner === side)
                .map((o) => o.slot);
            assert.equal(exile.length, 6);
            const run = await motionFixture(
                `pile-alignment-${side}-${graveyardCount}`,
                [view],
            );
            const anchor = `[data-table-library="${side}"]`;
            await check(
                `${pileAlignment}: ${side}, ${graveyardCount} in graveyard`,
                async (page) => {
                    const baseline = await page
                        .locator(anchor)
                        .evaluate((e) => e.getBoundingClientRect().bottom);
                    const graveyardButton = page.locator(
                        `.pile-button[data-side="${side}"][data-pile="graveyard"]`,
                    );
                    assert.equal(await graveyardButton.count(), graveyardCount);
                    for (const [zone, slots] of Object.entries({
                        library,
                        exile,
                        graveyard,
                    })) {
                        if (slots.length === 0) continue;
                        const edges = await page
                            .locator(
                                slots
                                    .map(
                                        (slot) =>
                                            `.card-el-pile${slotOf(slot)}`,
                                    )
                                    .join(","),
                            )
                            .evaluateAll((cards) => {
                                const z = (card) =>
                                    Number(getComputedStyle(card).zIndex);
                                return cards
                                    .sort((a, b) => z(b) - z(a))
                                    .map(
                                        (card) =>
                                            card.getBoundingClientRect().bottom,
                                    );
                            });
                        assert.equal(edges.length, slots.length);
                        assert.ok(
                            Math.abs(edges[0] - baseline) < 1 &&
                                Math.abs(Math.max(...edges) - baseline) < 1,
                            `${zone} face and pile end at ${baseline}, measured ${edges}`,
                        );
                    }
                },
                { ...run, ready: anchor, reducedMotion: "reduce" },
            );
        }
    }
}

await check("libraries match the other piles", async (page) => {
    await page.clock.runFor(3000);
    for (const side of ["you", "opponent"]) {
        const cards = page.locator(`.card-layer [data-library-pile="${side}"]`);
        assert.ok((await cards.count()) > 0);
        const direction = await cards.first().evaluate((e) => {
            const m = new DOMMatrix(getComputedStyle(e).transform);
            return Math.sign(m.a);
        });
        assert.equal(
            direction,
            1,
            `${side} library uses the same upright orientation as the other piles`,
        );
    }
});

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
        ready: ".board-arrows",
    },
);

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
        ready: ".board-arrows",
    },
);

for (const controller of ["you", "opponent"]) {
    const view = structuredClone((await load("arrow-player")).at(-1));
    const item = view.stack.at(-1);
    item.controller = controller;
    item.targets = [{ kind: "player", player: controller }];
    view.events = [];
    const run = await motionFixture(`self-target-${controller}`, [view]);
    await check(
        `self-target is green for ${controller}`,
        async (page) => {
            await page.clock.runFor(2000);
            assert.equal(
                await page
                    .locator(`.arrow-green[data-to="${controller}-life-orb"]`)
                    .count(),
                1,
            );
        },
        { ...run, at: 1, ready: ".board-arrows" },
    );
}

if (wanted("undo does not repeat an automatic target")) {
    const targetSlot = ownTarget.candidates[0].option.slot;
    const before = structuredClone(ownBefore);
    const target = structuredClone(ownAfter);
    for (const view of [before, target]) {
        const object = view.objects.find((o) => o.slot === targetSlot);
        object.owner = "opponent";
        object.controller = "opponent";
    }
    target.status.decision.prompt.state.warned = [];
    const resolved = structuredClone(target);
    resolved.status = structuredClone(before.status);
    resolved.status.decision.id.seq += 100;
    resolved.status.decision.undo += 2;
    resolved.stack = [];
    resolved.events = [];
    resolved.objects.find((o) => o.slot === oust).zone = "graveyard";
    resolved.players.you.graveyard.push(oust);
    const run = await motionFixture("undo-auto-target", [
        before,
        target,
        resolved,
    ]);
    await check(
        "undo does not repeat an automatic target",
        async (page) => {
            await page.clock.runFor(2000);
            await page.click(slotOf(oust));
            await settled(page, 2);
            await page.clock.runFor(4000);
            await page
                .getByRole("button", { name: "Undo", exact: true })
                .click();
            await page.clock.runFor(3000);
            assert.equal(
                await page.locator(`${slotOf(oust)}[data-zone="hand"]`).count(),
                1,
            );
            assert.equal(
                (await chosen(page)).length,
                2,
                "undo does not automatically recast",
            );
        },
        { ...run, at: 1, ready: slotOf(oust) },
    );
}

if (wanted("bug report thaw never revives resolved arrows")) {
    const before = structuredClone((await load("arrow-stacked")).at(-1));
    before.stack = [before.stack[0]];
    before.events = [];
    const source = before.stack.at(-1).slot;
    const after = structuredClone(before);
    after.stack = [];
    after.objects.find((o) => o.slot === source).zone = "graveyard";
    after.players.you.graveyard.push(source);
    after.events = [
        {
            kind: "zoneChange",
            slot: source,
            from: "stack",
            to: "graveyard",
            reason: "resolve",
        },
    ];
    const run = await motionFixture("thaw-resolved-arrow", [before, after]);
    await check(
        "bug report thaw never revives resolved arrows",
        async (page) => {
            await page.clock.runFor(2000);
            assert.equal(
                await page
                    .locator(`.arrow[data-from="card-${source}"]`)
                    .count(),
                1,
            );
            await page.evaluate(async () =>
                (await import("/src/freeze.ts")).freeze(),
            );
            await page.clock.runFor(50);
            await page
                .getByRole("button", { name: "Resolve", exact: true })
                .click();
            await page.evaluate(async () =>
                (await import("/src/freeze.ts")).thaw(),
            );
            await page.clock.runFor(5000);
            assert.equal(
                await page
                    .locator(`${slotOf(source)}[data-zone="graveyard"]`)
                    .count(),
                1,
            );
            assert.equal(
                await page
                    .locator(`.arrow[data-from="card-${source}"]`)
                    .count(),
                0,
            );
        },
        { ...run, ready: slotOf(source) },
    );
}

if (wanted("tokens reach their destination before ceasing")) {
    for (const to of ["hand", "library", "graveyard", "exile"]) {
        for (const side of ["you", "opponent"]) {
            const base = structuredClone((await load("arrow-spell")).at(-1));
            const token = structuredClone(
                (await load("tokens")).at(-1).objects.find((o) => o.token),
            );
            token.controller = side;
            token.owner = side;
            const before = {
                ...base,
                objects: [...base.objects, token],
                battlefield: [...base.battlefield, token.slot],
                events: [],
            };
            const after = {
                ...base,
                events: [
                    {
                        kind: "zoneChange",
                        slot: token.slot,
                        from: "battlefield",
                        to,
                        reason: "effect",
                    },
                ],
            };
            const run = await motionFixture(`token-destination-${side}-${to}`, [
                before,
                after,
            ]);
            await check(
                `tokens reach their destination before ceasing: ${side} ${to}`,
                async (page) => {
                    await page.clock.runFor(2000);
                    assert.equal(
                        await page
                            .locator(`${slotOf(token.slot)} .token-label`)
                            .innerText(),
                        "Token",
                    );
                    await startRecording(page);
                    // Only explicit steps may advance the flight: awaited browser calls
                    // otherwise consume its hold under load before the 650ms sample.
                    await page.clock.pauseAt(Date.now() + 60_000);
                    await page
                        .getByRole("button", { name: "Resolve", exact: true })
                        .click();
                    await until(
                        page,
                        `${slotOf(token.slot)}[data-zone="${to}"]`,
                        20,
                    );
                    await page.clock.runFor(650);
                    assert.equal(
                        await page
                            .locator(`[data-ceasing-slot="${token.slot}"]`)
                            .count(),
                        0,
                        "travel before dissolve",
                    );
                    await page.clock.runFor(1500);
                    assert.equal(
                        await page.locator(slotOf(token.slot)).count(),
                        0,
                    );
                    const recording = await page.evaluate(collect, 0);
                    assert.deepEqual(duplicatesIn(recording), []);
                    const track = posesOf(recording, token.slot);
                    assert.ok(
                        track.some((p) => p.zone === to),
                        "destination represented before disappearance",
                    );
                },
                { ...run, ready: slotOf(token.slot) },
            );
        }
    }
}

for (const next of ["player", "object", "attack"]) {
    const title = `bug report thaw preserves later ${next} arrows`;
    if (!wanted(title)) continue;
    const before = structuredClone((await load("arrow-stacked")).at(-1));
    before.stack = [before.stack[0]];
    before.events = [];
    const source = before.stack[0].slot;
    const after = structuredClone(before);
    after.stack = [];
    after.events = [];
    after.objects.find((o) => o.slot === source).zone = "graveyard";
    after.events = [
        {
            kind: "zoneChange",
            slot: source,
            from: "stack",
            to: "graveyard",
            reason: "resolve",
        },
    ];
    const later = structuredClone(
        next === "attack"
            ? pastWalker[attackAt]
            : (
                  await load(
                      next === "player" ? "arrow-player" : "arrow-ability",
                  )
              ).at(-1),
    );
    later.events = [];
    const run = await motionFixture(`thaw-later-${next}`, [
        before,
        after,
        later,
    ]);
    await check(
        title,
        async (page) => {
            await page.clock.runFor(2000);
            await page.evaluate(async () =>
                (await import("/src/freeze.ts")).freeze(),
            );
            await page.clock.runFor(50);
            await page
                .getByRole("button", { name: "Resolve", exact: true })
                .click();
            await page.evaluate(async () =>
                (await import("/src/freeze.ts")).thaw(),
            );
            await page.clock.runFor(2000);
            await page.keyboard.press("Space");
            await page.clock.runFor(2000);
            let from, to;
            if (next === "attack") {
                const slot = later.status.decision.candidates.find(
                    (c) => c.option.kind === "attack",
                ).option.slot;
                const walker = later.objects.find(
                    (o) =>
                        o.zone === "battlefield" &&
                        o.controller === "opponent" &&
                        o.types.includes("planeswalker"),
                ).slot;
                await page.click(slotOf(slot));
                await page.click(slotOf(walker));
                await page.mouse.move(0, 0);
                from = `card-${slot}`;
                to = `card-${walker}`;
            } else {
                const target = later.stack[0].targets[0];
                from = "stack-0";
                to =
                    target.kind === "player"
                        ? `${target.player}-life-orb`
                        : `card-${target.slot}`;
            }
            await page.clock.runFor(3000);
            await until(
                page,
                `.arrow[data-from="${from}"][data-to="${to}"]`,
                100,
            );
            assert.equal(
                await page
                    .locator(`.arrow[data-from="${from}"][data-to="${to}"]`)
                    .count(),
                1,
            );
        },
        { ...run, ready: slotOf(source) },
    );
}

await queue.drain();
await browser.close();
// A run of every check plays views and answers: none sounding means the sounds are unwired.
if (
    filters.length === 0 &&
    !chosenTitles &&
    (sounded.views === 0 || sounded.heard === 0)
) {
    failed = true;
    console.error(
        `sound: ${JSON.stringify(sounded)} checked; expected some of each`,
    );
}
if (ran === 0 && !chosenTitles) {
    failed = true;
    console.error(`no check's title contains ${filters.join(" or ")}`);
}
if (errors.length) {
    failed = true;
    console.error(`page errors:\n  ${errors.join("\n  ")}`);
}
process.exit(failed ? 1 : 0);
