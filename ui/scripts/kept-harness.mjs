// The harness of the kept checks (kept.mjs): the page on a fixture, opened on the stage, on
// Playwright's paused clock, with page errors, coverage, the hang guard and the run log. Each
// check holds a property no generic audit can express (plan K20), and says why in a comment.
import assert from "node:assert/strict";
import { readdir, readFile, writeFile } from "node:fs/promises";
import { basename, join } from "node:path";
import { strings } from "../src/cardRecords.ts";
import { coverage, launch, recordResult, workers } from "./capture.mjs";
import { CHECK_SECONDS, HangError, hangGuard } from "./hang.mjs";

export const [base, fixtures, ...filters] = process.argv.slice(2);
if (!base || !fixtures) {
    console.error(
        "usage: node scripts/kept.mjs <base-url> <fixtures-dir> [title-filter...]",
    );
    process.exit(2);
}
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

/** A recorded fixture's views, oldest first. */
export async function load(name) {
    const fixture = JSON.parse(
        await readFile(join(fixtures, `${name}.json`), "utf8"),
    );
    return fixture.messages
        .filter((m) => m.kind === "event")
        .map((m) => m.payload.view);
}

/** The card records a fixture recorded (its `card_records` response): what its page draws
 * its cards from. */
export async function cardRecords(name) {
    const fixture = JSON.parse(
        await readFile(join(fixtures, `${name}.json`), "utf8"),
    );
    return fixture.messages.find(
        (m) => m.kind === "response" && m.command === "card_records",
    ).value;
}

// Every fixture recorded for this run (ui/fixtures.json defines them), and none a script wrote
// into the folder: interactions.mjs runs first and writes its own beside them.
const { fixtures: defined } = JSON.parse(
    await readFile(new URL("../fixtures.json", import.meta.url), "utf8"),
);
const listed = new Set(await readdir(fixtures));
export const recorded = defined
    .map(({ name }) => name)
    .filter((name) => listed.has(`${name}.json`));

/** The records of every card the run's recordings show, by the name asked. */
export const recordedCards = { cards: {}, tokens: {} };
for (const name of recorded) {
    const { cards, tokens, ...art } = await cardRecords(name);
    Object.assign(recordedCards, art);
    Object.assign(recordedCards.cards, cards);
    Object.assign(recordedCards.tokens, tokens);
}

/** Writes the fixture `name` with the records of the cards it shows, as `beryl_cli
 * record-fixture` gives a recording its own: here from those the run's recordings hold,
 * whichever game showed the card, so a view cut, edited or renamed draws the cards it names. */
export async function writeFixture(name, fixture) {
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

// The fixture a check opens unless it names another: `greedy-turn-9` ends on a target decision
// for a spell the human had just cast from their hand; the page starts one view earlier, at the
// priority decision.
export const FIXTURE = "greedy-turn-9";
export const views = await load(FIXTURE);
const [before, after] = views.slice(-2);
const cast = after.events.find(
    (e) => e.kind === "zoneChange" && e.from === "hand" && e.to === "stack",
);
assert.ok(cast, "the fixture's last view follows a cast from the hand");
/** The spell cast between the last two views: its slot, its name and its card's selector. */
export const slot = cast.slot;
export const name = before.objects.find((o) => o.slot === slot).name;
export const card = `[data-slot="${slot}"]`;

/** The `choose` commands the page has sent, as recorded by the fixture transport. */
export const chosen = (page) =>
    page.evaluate(() =>
        window.berylFixture.calls
            .filter((c) => c.command === "choose")
            .map((c) => c.args.request.index),
    );

const browser = await launch();
const errors = [];
let failed = false;
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

/** Runs `body` on a fresh page of a fixture's first `at` views, drawn by the stage (`?stage`,
 * plan K25) whatever the run's switch, once `ready` matches and its fonts have loaded, alongside
 * other checks (capture.mjs' workers): awaiting it waits only for a free worker. Its run-log id
 * is `ui-test:<title>`, as the checks of interactions.mjs were. The page's fixtures come from
 * `fixtures`, not from what Vite serves. The page runs on Playwright's fake clock, paused once
 * the page is ready so that a check cannot race the wall clock; `clock: "off"` leaves the
 * browser's own, and says why. */
export async function check(
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
                const route = `/?fixture=${fixture}&at=${at}&stage${phone ? "&phone" : ""}${hash}`;
                await page.goto(new URL(route, base).href, {
                    waitUntil: "load",
                });
                await page.waitForSelector(ready);
                booted = true;
                await page.evaluate(() => document.fonts.ready.then(() => {}));
                if (clock === "paused") await pause(page);
                await body(page);
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

// Fixtures and helpers more than one part of the checks reads.

/** The clicks the page has sent, as recorded by the fixture transport. */
export const clicksSent = (page) =>
    page.evaluate(() =>
        window.berylFixture.calls
            .filter((c) => c.command === "click")
            .map((c) => c.args.request.click),
    );
/** The clicks sent once there are `count` of them, or as many as there are after a while. */
export async function clicked(page, count) {
    for (let i = 0; i < 20 && (await clicksSent(page)).length < count; i++) {
        await page.clock.runFor(100);
    }
    return clicksSent(page);
}

/** Writes the fixture `name`: `source` with `replies` to the clicks the page sends, in order
 * (the last again once they run out). On the stage the session holds a prompt's state, so a
 * click on a card or a button is one it answers: with the prompt as it then is (`SAME`, for
 * a click that changes nothing), or by saying the click answered it (`ANSWERED`), after which
 * the recording plays on. */
export async function replied(name, source, replies) {
    const fixture = JSON.parse(
        await readFile(join(fixtures, `${source}.json`), "utf8"),
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
export const SAME = (decision) => ({ kind: "open", prompt: decision.prompt });
export const ANSWERED = { kind: "answered" };
/** A click on the card in `slot`, as the page sends it. */
export const ON = (slot) => ({ kind: "on", on: { kind: "object", slot } });
/** A press of the button `kind`, as the page sends it. */
export const PRESS = (kind) => ({ kind: "button", button: { kind } });

/** The center of a bounding box. */
export const center = (box) => ({
    x: box.x + box.width / 2,
    y: box.y + box.height / 2,
});

/** Where every card is drawn, by slot: the box of what its element draws (a raised hand card
 * rises inside its element, which a copy over the hand shows), and whether a layout animation
 * is moving its frame. */
export const drawn = (page) =>
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

/** Whether two boxes overlap by more than a pixel each way. */
export const overlap = (a, b) =>
    Math.min(a.x + a.width, b.x + b.width) - Math.max(a.x, b.x) > 1 &&
    Math.min(a.y + a.height, b.y + b.height) - Math.max(a.y, b.y) > 1;

/** Lets React commit what the clock's last step, or the pointer, set off. Its scheduler posts
 * each render as a MessageChannel message, which Playwright's clock does not drive, so under
 * load a poll made right after could see the page a step behind; a message posted now lands
 * after it. */
export function committed(page) {
    return page.evaluate(
        () =>
            new Promise((resolve) => {
                const channel = new MessageChannel();
                channel.port1.onmessage = () => resolve();
                channel.port2.postMessage(null);
            }),
    );
}

/** The selector of the card in `slot`. */
export const slotOf = (slot) => `[data-slot="${slot}"]`;

/** Writes the fixture `as`: fixture `name`'s messages up to its view `at`, which `edit` rewrites. */
export async function derive(name, as, at, edit) {
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

/** A fixture's views and last decision, and the candidate index of each card it offers, by
 * slot. */
export async function lastDecision(name) {
    const views = await load(name);
    const decision = views.at(-1).status.decision;
    const bySlot = new Map(
        decision.candidates.flatMap((c, i) =>
            c.option.kind === "object" ? [[c.option.slot, i]] : [],
        ),
    );
    return { views, decision, bySlot };
}

export const scry = await lastDecision("choice-scry");

/** The cards of `side` in exile in view `v` that lie in their exile pile: none a permanent holds
 * beneath it. */
export const exiledFrom = (v, side) =>
    v.objects.filter(
        (o) =>
            o.owner === side &&
            o.zone === "exile" &&
            !v.battlefield.includes(o.exiledBy),
    );

// The last priority in the scry fixture where both players have graveyards and exiled cards,
// and the human holds a hand wide enough to fan out as far as it goes.
export const pilesAt = scry.views.findLastIndex(
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

// Tokens, piles of identical permanents and stolen permanents: the `tokens` fixture holds two
// identical Map tokens of yours, whose ability the engine offers through one of them only
// (architecture §7.2), a token changed by an Aura, and a permanent its owner doesn't control.
const tokenViews = await load("tokens");
export const tokenView = tokenViews.at(-1);
export const tokenField = tokenView.objects.filter(
    (o) => o.zone === "battlefield",
);
const mapAct = tokenView.status.decision.candidates.findIndex(
    (c) =>
        c.option.kind === "activate" &&
        tokenField.find((o) => o.slot === c.option.slot)?.token,
);
/** The Map token the engine offers the ability through. */
export const offeredToken = tokenField.find(
    (o) => o.slot === tokenView.status.decision.candidates[mapAct].option.slot,
);
export const tokenRun = {
    fixture: "tokens",
    at: tokenViews.length,
    ready: slotOf(offeredToken.slot),
};

/** Waits for every check to finish, then exits non-zero if one failed, a page logged an error,
 * or the filters reached none. */
export async function finish() {
    await queue.drain();
    await browser.close();
    if (ran === 0 && !chosenTitles && !errors.length) {
        // `by ui-test` hands a filter to every script it may be for, and fails only if it
        // reaches no check in any (selection.py NONE_REACHED).
        console.error(`no kept check's title contains ${filters.join(" or ")}`);
        process.exit(3);
    }
    if (errors.length) {
        failed = true;
        console.error(`page errors:\n  ${errors.join("\n  ")}`);
    }
    process.exit(failed ? 1 : 0);
}
