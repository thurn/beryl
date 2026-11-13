// The shape suite: every prompt shape the stage draws, answered from the
// seat that is asked and watched from the seat across the table. `by ui-test` serves the UI
// and runs this after kept.mjs and verbs.mjs. Usage:
//   node scripts/shapes.mjs <base-url> <fixtures-dir> [check...]
// A check is `shape:<shape>/<seat>/<layout>` (ui/shapes.json lists them), named whole or in
// part to run it alone, and logged by that name.
//
// The session holds a prompt's state, so a check has no rule of its own to hold
// the screen to: a fixture recorded with `clicks` (ui/fixtures.json) holds each click with
// the session's reply, and the check makes those clicks on the stage with the pointer, on
// the card, the player or the button each names, and holds the screen after each to that
// reply: every mark, in the stage's model and on the page, is one the session sent and none
// is missing; the prompt's title, a cost in it drawn as its symbols, and each button's words, whether it can be pressed and where
// it stands are the session's; and the click the page sent is the recording's, which the
// fixture's transport refuses any other in place of (src/ipc.ts). The cases of a shape are
// its purposes, each a stretch of the one recording. The desktop is played twice: by clicks
// alone, and with each card of the hand carried out of it instead. A phone's hand is read by
// a touch and played by carrying, so there the cards are always carried, and a touch on one
// is held to send nothing.
//
// A cast is taken back on a page of its own, which a recording plays no further from: by
// Escape on the desktop and by the button the prompt names in every layout, each sends the
// one `cancel`, and the screen is the priority the cast began at.
//
// A click on one of a pile of identical permanents is on the one the pile shows: the twin
// the session offers wears the pile's count and lies on top (the shape `pile`).
//
// What a prompt holds up off the table is held to the session's state as its marks are: a
// split's cards lie in the box of their pile, under its name and in its order; a pick's
// tray is up, each card showing its face; an order lists its triggers as they will resolve;
// and no card is drawn twice. A card of a split is dropped by carrying it to its place, and
// a trigger moved the same way, or put first by a click (shape-cases.mjs). The scry flight
// is a case of the split: the cards a scry looks at, from the look to the library again.
// A cost paid with permanents says how much is left as the session wrote it, and a
// declaration draws a line from each creature to what the session has it aimed at, where
// the session has one drawn, and says by the player's life total the combat damage the blocks
// picked so far leave on its way; paying with permanents and the combat line are their cases
// (shape-cases-table.mjs), the combat line followed on every recording, where no line of an
// attack declared crosses another; each line is a thin stroke with no head, and where there
// is a mouse a creature that awaits its aim pulls a line to it (shape-cases-lines.mjs). The ways to
// use a card are buttons over the table, each named as the session names the way; a number,
// a catalog's entries and a division's shares are held to the session's state, and set by
// the steps and entries the screen draws (shape-cases-panel.mjs).
//
// The seat across the table is shown none of it: its fixture plays the same game on, and at
// every tick the stage holds no mark and the dock no button, until the view that asks that
// seat itself, whose own prompt is then drawn.
//
// The stage's clock is held throughout and the stage settled after each click, so nothing
// here waits on the page's time but for the screen to draw what it was sent. Exits non-zero
// if a check fails or a page logs an error.
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { basename, join } from "node:path";
import {
    coverage,
    launch,
    recordResult,
    screenFaults,
    stageFaults,
    workers,
} from "./capture.mjs";
import { CHECK_SECONDS, HangError, hangGuard } from "./hang.mjs";
import { HANG_MS } from "./moments.mjs";
import {
    cardsUp,
    carry as carryTo,
    heldUp,
    onTable,
    reach,
    scryFlight,
    topOfPile,
} from "./shape-cases.mjs";
import { keyOf, panelOf, set, watching } from "./shape-cases-panel.mjs";
import { lineLooks, pointerLines } from "./shape-cases-lines.mjs";
import { drawn } from "./shape-drawn.mjs";
import {
    combatLines,
    declared,
    leftOf,
    payingWithPermanents,
} from "./shape-cases-table.mjs";

const [base, fixtures, ...filters] = process.argv.slice(2);
if (!base || !fixtures) {
    console.error(
        "usage: node scripts/shapes.mjs <base-url> <fixtures-dir> [check...]",
    );
    process.exit(2);
}
const { shapes, layouts } = JSON.parse(
    await readFile(new URL("../shapes.json", import.meta.url), "utf8"),
);
/** The checks the fast check chose (`by ui-test --select`, tools' selection.py), from the file
 * `BERYL_CHECKS` names, one a line; null to run every check the filters reach. */
const chosen = process.env.BERYL_CHECKS
    ? new Set((await readFile(process.env.BERYL_CHECKS, "utf8")).split("\n"))
    : null;
const wanted = (check) =>
    (filters.length === 0 || filters.some((f) => check.includes(f))) &&
    (!chosen || chosen.has(check));

const { shots } = JSON.parse(
    await readFile(new URL("../shots.json", import.meta.url), "utf8"),
);
/** What each layout of the stage is checked at: the query that asks for it, its windows (a
 * phone's are the phone sizes ui/shots.json has), where its dock's buttons are and where it
 * writes what the prompt asks, and whether a card of the hand is played by a click. */
const LAYOUTS = {
    desktop: {
        query: "",
        windows: [{ width: 1600, height: 900 }],
        dock: ".stage-prompt, .stage-ways",
        title: ".stage-prompt .prompt-title",
        captions: true,
        handClicks: true,
    },
    phone: {
        query: "phone&",
        windows: [
            ...new Map(
                shots
                    .filter((shot) =>
                        new URL(shot.route, base).searchParams.has("phone"),
                    )
                    .map(({ width, height }) => [
                        `${width}x${height}`,
                        { width, height },
                    ]),
            ).values(),
        ],
        dock: ".phone-dock, .phone-prompt-more, .stage-ways",
        // The prompt's own pill, not a scry's, which may still be passing over the table
        // (it outlasts its script: timing.ts, `passes`).
        title: ".phone-pill:not(.stage-notice-scry)",
        captions: false,
        handClicks: false,
    },
};
assert.deepEqual(
    layouts.filter((layout) => !LAYOUTS[layout]?.windows.length),
    [],
    "ui/shapes.json's layouts and their windows",
);
/** The longest a recording's views after one click take to play out, in stage ms. */
const SETTLE = 600_000;
/** The tick the watching seat's stage is stepped by, in stage ms. */
const TICK = 100;

const recorded = async (fixture) =>
    JSON.parse(await readFile(join(fixtures, `${fixture}.json`), "utf8"));

/** What `view` holds of a prompt the stage draws: the prompt and its decision, or neither. */
function held(view) {
    const decision =
        view.status.kind === "deciding" ? view.status.decision : undefined;
    return decision && onTable(decision.prompt)
        ? { decision, prompt: decision.prompt, view }
        : { view };
}

/** What a recording asks of its seat, from its stop on (its first view, or the view its
 * board's own play led to, the `at`th): the prompt that view opens, then each click with the prompt the screen holds after it (none while the game
 * plays on or asks what the stage does not draw) and the decision that prompt is of. */
function asked(fixture) {
    const messages = fixture.messages.filter(
        (m) =>
            m.kind === "event" ||
            (m.command === "click" && m.request !== undefined),
    );
    // The board's own play comes before the stop, where the clicks begin.
    const stop = messages.findIndex((m) => m.kind !== "event") - 1;
    const first = messages[stop];
    const rest = messages.slice(stop + 1);
    let now = held(first.payload.view);
    const clicks = [];
    for (const message of rest) {
        if (message.kind === "event") {
            // A view the click before it led to: the last of them is what is shown.
            now = held(message.payload.view);
            clicks.at(-1).after = now;
            continue;
        }
        const { click } = message.request;
        const reply = message.value;
        const before = now;
        if (reply.kind === "open") now = { ...before, prompt: reply.prompt };
        clicks.push({
            click,
            before,
            after: now,
            answered: reply.kind !== "open",
        });
    }
    return { opens: held(first.payload.view), clicks, at: stop + 1 };
}

/** What the screen should draw of `held`, a prompt and its decision, in `layout`: its title,
 * its buttons where each stands, its marks, what it holds up, how much of its cost is left
 * and the lines of its declaration. */
function expected(held, layout) {
    const { decision, prompt } = held;
    const more = {
        up: heldUp(prompt),
        left: leftOf(prompt),
        panel: panelOf(prompt),
        ...declared(prompt, held.view),
    };
    if (!prompt)
        return { title: "", symbols: [], buttons: [], marks: [], ...more };
    const where = (button) =>
        button.primary
            ? "forward"
            : button.id.kind === "passOn"
              ? "passOn"
              : "other";
    const buttons = prompt.buttons.map((button) => ({
        label: button.label,
        enabled: button.enabled,
        where: where(button),
        caption: layout.captions && button.primary ? button.caption : undefined,
    }));
    if (decision.cancellable) {
        buttons.push({
            label: prompt.words.cancel,
            enabled: true,
            where: "other",
            caption: undefined,
        });
    }
    return {
        title: prompt.title,
        // A cost in what is asked, or in how much of it is left, is drawn as its symbols.
        symbols: `${prompt.title}${more.left}`.match(/\{[^}]+\}/g) ?? [],
        buttons: buttons.sort(byPlace),
        marks: prompt.marks
            .map(({ on, mark }) => `${mark} ${subject(on)}`)
            .sort(),
        ...more,
    };
}

/** What `drawn` returns of a screen that draws `want` (`expected`) and is at rest. */
const whole = (want) => ({
    title: want.title,
    symbols: want.symbols,
    buttons: want.buttons,
    marks: want.marks,
    model: want.marks,
    coming: 0,
    busy: false,
    up: want.up,
    left: want.left,
    panel: want.panel,
    lines: { page: want.lines, model: want.lines },
    damage: want.damage,
});

const subject = (on) =>
    on.kind === "object"
        ? String(on.slot)
        : on.kind === "ability"
          ? `a${on.id}`
          : on.player;

const byPlace = (a, b) =>
    `${a.where} ${a.label}`.localeCompare(`${b.where} ${b.label}`);

/** Holds the page to draw `want` (`expected`), once it has drawn what it was sent: the
 * screen is React's, which draws a reply a moment after the stage's model has it. */
async function holds(page, layout, want, what) {
    const asked = {
        ...layout,
        arrow: want.arrow,
        blocks: want.damage !== null,
    };
    const wanted = JSON.stringify(whole(want));
    const sorted = (got) => ({
        ...got,
        buttons: [...got.buttons].sort(byPlace),
    });
    try {
        await page.waitForFunction(
            ([drawn, asked, wanted]) => {
                const got = new Function(`return (${drawn})`)()(asked);
                const sorted = [...got.buttons].sort((a, b) =>
                    `${a.where} ${a.label}`.localeCompare(
                        `${b.where} ${b.label}`,
                    ),
                );
                return JSON.stringify({ ...got, buttons: sorted }) === wanted;
            },
            [drawn.toString(), asked, wanted],
            { timeout: HANG_MS },
        );
    } catch {
        // It never did: say how it differs.
        const got = sorted(await page.evaluate(drawn, asked));
        if (JSON.stringify(got) === wanted) return;
        assert.deepEqual(got, whole(want), what);
    }
}

/** In the page: lets what a click sent come back, and plays out all it led to; then a second
 * more of stage time, in which a card let go where its click moved nothing glides home. */
const settled = (limit) =>
    new Promise((resolve) => setTimeout(resolve)).then(() => {
        window.berylStage.settle(limit);
        window.berylStage.clock.advance(1000);
    });

const clicksSent = (page) =>
    page.evaluate(() =>
        window.berylFixture.calls
            .filter((call) => call.command === "click")
            .map((call) => call.args.request),
    );

/** Opens `fixture` at its `at`th view in a page of `context` asked for by `query`, with the
 * stage's clock held and settled. */
async function opened(context, watch, query, fixture, at = 1) {
    const page = await context.newPage();
    const done = await watch(page);
    await page.goto(
        new URL(`/?${query}fixture=${fixture}&at=${at}#/game`, base).href,
    );
    await page.waitForFunction(
        () => window.berylStage && document.querySelector(".stage-node"),
    );
    await page.evaluate(() => document.fonts.ready.then(() => {}));
    await page.evaluate(() => window.berylStage.settle());
    return { page, done };
}

/** A touch on the card `slot` of the hand reads it and sends nothing; another puts it down. */
async function touched(page, slot) {
    const at = await reach(page, slot);
    await page.mouse.click(at.x, at.y);
    await page.waitForSelector(".inspect");
    assert.deepEqual(await clicksSent(page), [], "a touch on the hand");
    await page.mouse.click(at.x, at.y);
    await page.waitForSelector(".inspect", { state: "detached" });
}

/** Answers the prompts of `fixture` on a page of its own by the recording's clicks, each made
 * with the pointer, and holds the screen to the session's state before the first and after
 * each. With `carry`, a card of the hand is carried out of it instead of clicked, and a button
 * that has a key is pressed by it. Every line a declaration draws is followed to the damage. */
async function answer(context, watch, layout, fixture, carry) {
    const { opens, clicks, at } = asked(await recorded(fixture));
    const { page, done } = await opened(
        context,
        watch,
        layout.query,
        fixture,
        at,
    );
    const paying = payingWithPermanents(page, layout.dock, TICK);
    let piled = false;
    const combat = combatLines(page, TICK);
    try {
        await holds(page, layout, expected(opens, layout), "as it opens");
        if (!layout.handClicks) {
            // A touch on a card of the hand reads it: nothing is sent.
            const hand = opens.view.objects.find(
                (o) => o.zone === "hand" && o.owner === "you",
            );
            if (hand) await touched(page, hand.slot);
        }
        for (const [
            i,
            { click, before, after, answered },
        ] of clicks.entries()) {
            const purpose = before.prompt.purpose;
            const what = `${purpose}, click ${i} (${JSON.stringify(click)})`;
            if (click.kind === "button") {
                const { label } = before.prompt.buttons.find(
                    (button) =>
                        JSON.stringify(button.id) ===
                        JSON.stringify(click.button),
                );
                const button = page
                    .locator(layout.dock)
                    .getByRole("button", { name: label, exact: true });
                const key = carry && (await keyOf(page, before.prompt, click));
                if (key) await page.keyboard.press(key);
                else await button.click();
            } else if (click.kind === "number" || click.kind === "entry") {
                await set(page, click, before);
            } else if (click.kind === "drop" && click.on.kind === "ability") {
                // A waiting trigger: put first by a click, or carried to its place.
                const item = page.locator(
                    `.stage-order-item[data-item="${click.on.id}"]`,
                );
                const box = await item.boundingBox();
                const from = {
                    x: box.x + box.width / 2,
                    y: box.y + box.height / 2,
                };
                if (click.at === 0 && !carry)
                    await page.mouse.click(from.x, from.y);
                else await carryTo(page, click, before, from);
            } else if (click.kind === "drop") {
                await carryTo(
                    page,
                    click,
                    before,
                    await reach(page, subject(click.on)),
                );
            } else {
                const name = subject(click.on);
                piled ||= await topOfPile(page, name, what);
                const at = await reach(page, name);
                // A card held up is clicked where it is, whatever zone it came from.
                const inHand =
                    !cardsUp(before.prompt).includes(Number(name)) &&
                    before.view.objects.some(
                        (o) => String(o.slot) === name && o.zone === "hand",
                    );
                if (inHand && carry) {
                    // Out of the hand, well clear of where the card rests.
                    await page.mouse.move(at.x, at.y);
                    await page.mouse.down();
                    await page.mouse.move(at.x, at.top - 120, { steps: 6 });
                    await page.mouse.move(at.x - 40, at.top - 260, {
                        steps: 6,
                    });
                    await page.mouse.up();
                } else {
                    await page.mouse.click(at.x, at.y);
                }
            }
            await page.waitForFunction(
                (count) =>
                    window.berylFixture.calls.filter(
                        (call) => call.command === "click",
                    ).length === count,
                i + 1,
            );
            if (purpose === "scry" && answered) {
                await scryFlight(page, before, after, TICK);
            }
            if (answered) await paying.answered(before);
            await combat.follow(before, answered);
            await page.evaluate(settled, SETTLE);
            await holds(page, layout, expected(after, layout), `after ${what}`);
            await lineLooks(page, after, `after ${what}`);
            if (layout.handClicks)
                await pointerLines(page, after, `after ${what}`);
            await paying.drawn(before, after);
            if (answered) {
                // The stage shows the view the recording played on to.
                const shows = await page.evaluate(() => {
                    const shown = window.berylStage.shown.view;
                    return shown === undefined
                        ? null
                        : [
                              shown.turn.number,
                              shown.turn.step,
                              shown.status.kind,
                          ];
                });
                const { turn, status } = after.view;
                assert.deepEqual(
                    shows,
                    [turn.number, turn.step, status.kind],
                    `the view after ${what}`,
                );
            }
        }
        // Each click the page sent was the recording's, on the state it was made on.
        const sent = await clicksSent(page);
        assert.deepEqual(
            sent.map((request) => request.click),
            clicks.map(({ click }) => click),
            "the clicks sent",
        );
        assert.deepEqual(
            sent.map((request) => request.revision),
            clicks.map(({ before }) => before.prompt.revision),
            "the state each click was made on",
        );
        paying.done();
        combat.done();
        assert.deepEqual(await stageFaults(page), []);
        return [
            ...clicks.map(({ before }) => before.prompt.purpose),
            ...(piled ? ["pile"] : []),
        ];
    } finally {
        await done();
        await page.close();
    }
}

/** Takes back a cast of `fixture`, on a page of its own opened at a view whose prompt is a
 * step of a cast that can be taken back: the first such view by Escape, where the layout has
 * keys, and the last by the button the prompt's own words name. One `cancel` is sent and no
 * click, and the screen draws the priority the cast began at again, its card where the view
 * has it. Nothing for a recording that casts nothing so. */
async function takeBack(context, watch, layout, fixture, key) {
    const views = (await recorded(fixture)).messages
        .filter((m) => m.kind === "event")
        .map((m) => m.payload.view);
    const casts = views.flatMap((view, i) =>
        held(view).decision?.cancellable ? [i] : [],
    );
    const at = key ? casts[0] : casts.at(-1);
    if (at === undefined) return;
    const cast = held(views[at]);
    const began = views.findLastIndex(
        (view, i) => i < at && held(view).decision?.kind === "priority",
    );
    const what = `${fixture}'s ${cast.prompt.purpose} taken back by ${key ? "Escape" : "its button"}`;
    const { page, done } = await opened(
        context,
        watch,
        layout.query,
        fixture,
        at + 1,
    );
    try {
        await holds(page, layout, expected(cast, layout), `${what}: before`);
        if (key) await page.keyboard.press("Escape");
        else
            await page
                .locator(layout.dock)
                .getByRole("button", {
                    name: cast.prompt.words.cancel,
                    exact: true,
                })
                .click();
        await page.evaluate(settled, SETTLE);
        await holds(page, layout, expected(held(views[began]), layout), what);
        const sent = await page.evaluate(() =>
            window.berylFixture.calls
                .map((call) => call.command)
                .filter((command) =>
                    ["cancel", "click", "choose", "pass_on", "undo"].includes(
                        command,
                    ),
                ),
        );
        assert.deepEqual(sent, ["cancel"], `${what}: what was sent`);
        assert.deepEqual(await screenFaults(page), [], what);
    } finally {
        await done();
        await page.close();
    }
}

/** Watches `fixture` from the seat across the table on a page of its own, up to the first
 * view that asks that seat: none of the prompt the other seat answers is shown, and the
 * prompt that view asks this seat is. */
async function watchIt(context, watch, layout, fixture) {
    const recording = await recorded(fixture);
    const views = recording.messages
        .filter((m) => m.kind === "event")
        .map((m) => m.payload.view);
    const asks = views.find((view) => view.status.kind === "deciding");
    assert.ok(asks && asks !== views[0], `${fixture} must play on to a prompt`);
    const { page, done } = await opened(context, watch, layout.query, fixture);
    try {
        const waits = {
            title: "",
            symbols: [],
            marks: [],
            // The turn button stands where it will be, with nothing to press.
            buttons: [
                {
                    label: "Pass",
                    enabled: false,
                    where: "forward",
                    caption: layout.captions ? "" : undefined,
                },
            ],
            up: heldUp(),
            left: "",
            panel: panelOf(),
            ...declared(),
        };
        await holds(page, layout, waits, "as it opens");
        const { seen, done: played } = await page.evaluate(watching, {
            tick: TICK,
            dock: layout.dock,
        });
        assert.equal(
            played,
            true,
            "the recording did not play on to its prompt",
        );
        assert.deepEqual(
            seen.slice(0, 3),
            [],
            "shown of the other seat's prompt",
        );
        const { decision } = asks.status;
        const own = onTable(decision.prompt)
            ? { decision, prompt: decision.prompt }
            : {};
        await holds(page, layout, expected(own, layout), "its own prompt");
        assert.deepEqual(await stageFaults(page), []);
    } finally {
        await done();
        await page.close();
    }
}

const browser = await launch();
const queue = workers(6);
const errors = [];
let failed = false;
let ran = 0;

/** A context of windows of `size`, whose pages fetch the fixtures this run recorded, not what
 * Vite serves. */
async function open(size) {
    const context = await browser.newContext({
        viewport: size,
        deviceScaleFactor: 1,
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
    return context;
}

for (const [shape, seats] of Object.entries(shapes)) {
    for (const [seat, fixture] of Object.entries(seats)) {
        for (const layout of layouts) {
            const check = `shape:${shape}/${seat}/${layout}`;
            if (!wanted(check)) continue;
            ran += 1;
            await queue(async () => {
                const started = performance.now();
                const at = LAYOUTS[layout];
                const contexts = [];
                const seen = [];
                let threw = false;
                let hung = false;
                let covered;
                // Every page's errors are the check's; what a page ran is taken of the first.
                const watch = async (page) => {
                    page.on("pageerror", (e) =>
                        seen.push(`${check}: ${e.message}`),
                    );
                    page.on(
                        "console",
                        (m) =>
                            m.type() === "error" &&
                            seen.push(`${check}: ${m.text()}`),
                    );
                    if (covered !== undefined) return async () => {};
                    covered = null;
                    const finish = await coverage(page);
                    return async () => {
                        if (!hung) covered = await finish();
                    };
                };
                const play = async () => {
                    for (const [i, size] of at.windows.entries()) {
                        const context = await open(size);
                        contexts.push(context);
                        if (seat === "theirs") {
                            await watchIt(context, watch, at, fixture);
                            continue;
                        }
                        // By clicks where a click plays a card, at the first window; and
                        // with the hand's cards carried, at every one.
                        const carries =
                            at.handClicks && i === 0 ? [false, true] : [true];
                        // A cast taken back, by its key and by its button.
                        for (const key of i > 0 ? [] : carries.slice(1)) {
                            await takeBack(context, watch, at, fixture, key);
                        }
                        if (i === 0) {
                            await takeBack(context, watch, at, fixture, false);
                        }
                        for (const carry of carries) {
                            const cases = await answer(
                                context,
                                watch,
                                at,
                                fixture,
                                carry,
                            );
                            assert.ok(
                                cases.length > 0,
                                "no click was recorded",
                            );
                            assert.ok(
                                shape !== "pile" || cases.includes("pile"),
                                "no click was on a pile",
                            );
                        }
                    }
                };
                try {
                    await hangGuard(play(), CHECK_SECONDS, check).catch((e) => {
                        hung = e instanceof HangError;
                        throw e;
                    });
                    console.log(`ok: ${check}`);
                } catch (e) {
                    failed = true;
                    threw = true;
                    console.error(`FAILED: ${check}\n${e.stack ?? e}`);
                } finally {
                    for (const context of contexts) await context.close();
                }
                errors.push(...seen);
                recordResult(
                    check,
                    threw || seen.length ? "fail" : "pass",
                    (performance.now() - started) / 1000,
                    covered ?? null,
                );
            });
        }
    }
}
await queue.drain();
await browser.close();

if (ran === 0 && !chosen) {
    failed = true;
    console.error(`no shape check's name contains ${filters.join(" or ")}`);
}
if (errors.length) {
    failed = true;
    console.error(`page errors:\n  ${errors.join("\n  ")}`);
}
process.exit(failed ? 1 : 0);
