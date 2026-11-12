// The shape suite (plan K24 and K26): every prompt shape the stage draws, answered from the
// seat that is asked and watched from the seat across the table. `by ui-test` serves the UI
// and runs this beside interactions.mjs and verbs.mjs. Usage:
//   node scripts/shapes.mjs <base-url> <fixtures-dir> [check...]
// A check is `shape:<shape>/<seat>/<layout>` (ui/shapes.json lists them), named whole or in
// part to run it alone, and logged by that name.
//
// The session holds a prompt's state (plan K26), so a check has no rule of its own to hold
// the screen to: a fixture recorded with `clicks` (ui/fixtures.json) holds each click with
// the session's reply, and the check makes those clicks on the stage with the pointer, on
// the card, the player or the button each names, and holds the screen after each to that
// reply: every mark, in the stage's model and on the page, is one the session sent and none
// is missing; the prompt's title and each button's words, whether it can be pressed and where
// it stands are the session's; and the click the page sent is the recording's, which the
// fixture's transport refuses any other in place of (src/ipc.ts). The cases of a shape are
// its purposes, each a stretch of the one recording. The desktop is played twice: by clicks
// alone, and with each card of the hand carried out of it instead. A phone's hand is read by
// a touch and played by carrying, so there the cards are always carried, and a touch on one
// is held to send nothing.
//
// What a prompt holds up off the table is held to the session's state as its marks are: a
// split's cards lie in the box of their pile, under its name and in its order; a pick's
// tray is up, each card showing its face; an order lists its triggers as they will resolve;
// and no card is drawn twice. A card of a split is dropped by carrying it to its place, and
// a trigger moved the same way, or put first by a click (shape-cases.mjs). The scry flight
// is a case of the split: the cards a scry looks at, from the look to the library again.
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
    stageFaults,
    workers,
} from "./capture.mjs";
import { CHECK_SECONDS, HangError, hangGuard } from "./hang.mjs";
import {
    cardsUp,
    carry as carryTo,
    heldUp,
    onTable,
    scryFlight,
} from "./shape-cases.mjs";

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
        query: "stage",
        windows: [{ width: 1600, height: 900 }],
        dock: ".stage-prompt",
        title: ".stage-prompt .prompt-title",
        captions: true,
        handClicks: true,
    },
    phone: {
        query: "phone&stage",
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
        dock: ".phone-dock, .phone-prompt-more",
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

/** What a recording asks of its seat, from its stop on (its first view, or the view its
 * board's own play led to, the `at`th): the prompt that view opens, then each click with the prompt the screen holds after it (none while the game
 * plays on or asks what the stage does not draw) and the decision that prompt is of. */
function asked(fixture) {
    const messages = fixture.messages.filter(
        (m) =>
            m.kind === "event" ||
            (m.command === "click" && m.request !== undefined),
    );
    const held = (view) => {
        const decision =
            view.status.kind === "deciding" ? view.status.decision : undefined;
        return decision && onTable(decision.prompt)
            ? { decision, prompt: decision.prompt, view }
            : { view };
    };
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
 * its buttons where each stands, and its marks. */
function expected(held, layout) {
    const { decision, prompt } = held;
    if (!prompt) return { title: "", buttons: [], marks: [], up: heldUp() };
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
        buttons: buttons.sort(byPlace),
        marks: prompt.marks
            .map(({ on, mark }) => `${mark} ${subject(on)}`)
            .sort(),
        up: heldUp(prompt),
    };
}

const subject = (on) =>
    on.kind === "object"
        ? String(on.slot)
        : on.kind === "ability"
          ? `a${on.id}`
          : on.player;

const byPlace = (a, b) =>
    `${a.where} ${a.label}`.localeCompare(`${b.where} ${b.label}`);

/** In the page: what the screen draws of a prompt now. The title, the dock's buttons where
 * each stands (`forward` in the turn button's place, `passOn` the round one, `other` stacked
 * over), every mark the page and the stage's model hold, each as `<mark> <on>`, and what is
 * held up (`heldUp`): the cards lying over everything at rest and under what flies, those in
 * a pile's box by that box, and the triggers listed. */
function drawn({ dock, title, captions }) {
    // A label as the session wrote it: a mana symbol is drawn from its `{...}`.
    const words = (element) => {
        const copy = element.cloneNode(true);
        for (const symbol of copy.querySelectorAll(
            ".mana-symbol[aria-label]",
        )) {
            symbol.textContent = symbol.getAttribute("aria-label");
        }
        for (const tip of copy.querySelectorAll(".tip, kbd")) tip.remove();
        return copy.textContent.trim();
    };
    const buttons = [...document.querySelectorAll(dock)].flatMap((part) =>
        [...part.querySelectorAll("button")].map((button) => {
            const passOn = button.classList.contains("pass-on-button");
            const forward =
                !passOn &&
                (button.closest(".turn-action") !== null ||
                    button.closest(".phone-dock") !== null);
            const caption = button
                .closest(".turn-action")
                ?.querySelector(".turn-caption");
            return {
                label: passOn
                    ? button.getAttribute("aria-label")
                    : words(button),
                enabled: !button.disabled,
                where: passOn ? "passOn" : forward ? "forward" : "other",
                caption:
                    captions && forward
                        ? caption
                            ? words(caption)
                            : ""
                        : undefined,
            };
        }),
    );
    const on = (mark) => {
        const node = mark.parentElement;
        return node.dataset.stage ?? node.dataset.life ?? "?";
    };
    const page = [...document.querySelectorAll(".stage-mark")].map((mark) => {
        const name = [...mark.classList]
            .find((c) => c.startsWith("stage-mark-"))
            .slice("stage-mark-".length);
        return `${name} ${on(mark)}`;
    });
    const stage = window.berylStage;
    const heading = document.querySelector(title);
    const nodes = [...document.querySelectorAll("[data-stage]")];
    const lifted = nodes.filter((node) => {
        const depth = Number(node.style.zIndex);
        return depth >= 850 && depth < 900;
    });
    const slotOf = (node) => Number(node.dataset.stage);
    const middle = (node) => {
        const box = node.getBoundingClientRect();
        return { x: box.left + box.width / 2, y: box.top + box.height / 2 };
    };
    const boxed = new Set();
    const piles = [...document.querySelectorAll(".stage-tray-pile")].map(
        (pile) => {
            const box = pile.getBoundingClientRect();
            const within = lifted.filter((node) => {
                const { x, y } = middle(node);
                return (
                    x >= box.left &&
                    x <= box.right &&
                    y >= box.top &&
                    y <= box.bottom
                );
            });
            for (const node of within) boxed.add(node);
            return {
                label: pile.textContent.trim(),
                cards: within
                    .sort((a, b) => middle(a).x - middle(b).x)
                    .map(slotOf),
            };
        },
    );
    const up = {
        piles,
        loose: lifted
            .filter((node) => !boxed.has(node))
            .map(slotOf)
            .sort((a, b) => a - b),
        order: [...document.querySelectorAll(".stage-order-item")].map((item) =>
            Number(item.dataset.item),
        ),
        backs: lifted.filter((node) => node.dataset.card === undefined).length,
        twice: nodes.length - new Set(nodes.map((n) => n.dataset.stage)).size,
    };
    return {
        title: heading ? words(heading) : "",
        buttons,
        marks: page.sort(),
        model: stage
            .marks()
            .map(({ mark, on }) => `${mark} ${on}`)
            .sort(),
        // A mark at rest is whole.
        coming: [...document.querySelectorAll(".stage-mark")].filter(
            (mark) => mark.style.getPropertyValue("--shown") !== "1",
        ).length,
        busy: stage.shown.busy,
        up,
    };
}

/** Holds the page to draw `want` (`expected`), once it has drawn what it was sent: the
 * screen is React's, which draws a reply a moment after the stage's model has it. */
async function holds(page, layout, want, what) {
    const read = () => page.evaluate(drawn, layout);
    const same = (got) =>
        JSON.stringify({ ...got, buttons: [...got.buttons].sort(byPlace) }) ===
        JSON.stringify({
            title: want.title,
            buttons: want.buttons,
            marks: want.marks,
            model: want.marks,
            coming: 0,
            busy: false,
            up: want.up,
        });
    try {
        await page.waitForFunction(
            ([drawn, layout, wanted]) => {
                const got = new Function(`return (${drawn})`)()(layout);
                const sorted = [...got.buttons].sort((a, b) =>
                    `${a.where} ${a.label}`.localeCompare(
                        `${b.where} ${b.label}`,
                    ),
                );
                return JSON.stringify({ ...got, buttons: sorted }) === wanted;
            },
            [
                drawn.toString(),
                layout,
                JSON.stringify({
                    title: want.title,
                    buttons: want.buttons,
                    marks: want.marks,
                    model: want.marks,
                    coming: 0,
                    busy: false,
                    up: want.up,
                }),
            ],
            { timeout: 15_000 },
        );
    } catch {
        // It never did: say how it differs.
        const got = await read();
        if (same(got)) return;
        assert.deepEqual(
            { ...got, buttons: [...got.buttons].sort(byPlace) },
            {
                title: want.title,
                buttons: want.buttons,
                marks: want.marks,
                model: want.marks,
                coming: 0,
                busy: false,
                up: want.up,
            },
            what,
        );
    }
}

/** In the page: lets what a click sent come back, and plays out all it led to. */
const settled = (limit) =>
    new Promise((resolve) => setTimeout(resolve)).then(() =>
        window.berylStage.settle(limit),
    );

/** A point of the stage's node `name` the pointer reaches: nearest its middle, where no other
 * card or control lies over it. */
const reach = (page, name) =>
    page.evaluate((name) => {
        const node =
            document.querySelector(`[data-stage="${name}"]`) ??
            document.querySelector(`[data-life="${name}"]`);
        const box = node.getBoundingClientRect();
        const at = (fx, fy) => ({
            x: box.left + box.width * fx,
            y: box.top + box.height * fy,
        });
        const shares = [0.5, 0.35, 0.65, 0.2, 0.8, 0.1, 0.9];
        for (const fy of shares) {
            for (const fx of shares) {
                const { x, y } = at(fx, fy);
                const hit = document.elementFromPoint(x, y);
                if (hit && (hit === node || node.contains(hit))) {
                    return { x, y, top: box.top };
                }
            }
        }
        throw new Error(`nothing of ${name} can be reached`);
    }, name);

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
        new URL(`/?${query}&fixture=${fixture}&at=${at}#/game`, base).href,
    );
    await page.waitForFunction(
        () => window.berylStage && document.querySelector(".stage-node"),
    );
    await page.evaluate(() => document.fonts.ready.then(() => {}));
    await page.evaluate(() => window.berylStage.settle());
    return { page, done };
}

/** Answers the prompts of `fixture` on a page of its own by the recording's clicks, each made
 * with the pointer, and holds the screen to the session's state before the first and after
 * each. With `carry`, a card of the hand is carried out of it instead of clicked. */
async function answer(context, watch, layout, fixture, carry) {
    const { opens, clicks, at } = asked(await recorded(fixture));
    const { page, done } = await opened(
        context,
        watch,
        layout.query,
        fixture,
        at,
    );
    try {
        await holds(page, layout, expected(opens, layout), "as it opens");
        if (!layout.handClicks) {
            // A touch on a card of the hand reads it: nothing is sent.
            const hand = opens.view.objects.find(
                (o) => o.zone === "hand" && o.owner === "you",
            );
            const at = await reach(page, hand.slot);
            await page.mouse.click(at.x, at.y);
            await page.waitForSelector(".inspect");
            assert.deepEqual(await clicksSent(page), [], "a touch on the hand");
            await page.mouse.click(at.x, at.y);
            await page.waitForSelector(".inspect", { state: "detached" });
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
                await page
                    .locator(layout.dock)
                    .getByRole("button", { name: label, exact: true })
                    .click();
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
            await page.evaluate(settled, SETTLE);
            await holds(page, layout, expected(after, layout), `after ${what}`);
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
        assert.deepEqual(await stageFaults(page), []);
        return clicks.map(({ before }) => before.prompt.purpose);
    } finally {
        await done();
        await page.close();
    }
}

/** In the page: plays the recording on from its first view to the next that asks its seat,
 * stepping the stage's clock, and returns what any tick before that view was shown held of a
 * prompt: the marks in the model and on the page, and the dock's buttons that could be
 * pressed. */
async function watching({ tick, dock }) {
    const stage = window.berylStage;
    stage.clock.hold();
    await window.berylFixture.invoke("choose", { request: {} });
    await new Promise((resolve) => setTimeout(resolve));
    const last = window.berylFixture.view();
    const seen = [];
    while (stage.shown.busy && stage.shown.view !== last) {
        stage.clock.advance(tick);
        if (stage.shown.view === last) break;
        const marks = [
            ...stage.marks().map(({ mark, on }) => `${mark} ${on}`),
            ...[...document.querySelectorAll(".stage-mark")].map(
                (mark) => `page ${mark.className}`,
            ),
        ];
        const buttons = [...document.querySelectorAll(dock)]
            .flatMap((part) => [...part.querySelectorAll("button")])
            .filter((button) => !button.disabled)
            .map((button) => button.textContent);
        if (marks.length || buttons.length) {
            seen.push({ at: stage.clock.now, marks, buttons });
        }
    }
    stage.settle();
    return { seen, done: stage.shown.view === last };
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
