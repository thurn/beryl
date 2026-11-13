// The kept checks of a script in play (kept.mjs): a key and a press while a script plays, Undo
// over a prompt that holds the table, the inspector of a card held up, and a reloaded page.
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { join } from "node:path";
import {
    FIXTURE,
    check,
    clicked,
    clicksSent,
    fixtures,
    lastDecision,
    load,
    scry,
    views,
    writeFixture,
} from "./kept-harness.mjs";

/** Whether view `v` waits for the human: the views of the game playing on without them
 * follow one another unasked. */
const awaits = (v) =>
    v.status.kind !== "thinking" && v.status.kind !== "playing";

// The opponent's turn played back: `opponent-turn` ends on the session's halt at the human's
// precombat main (End Turn), the first since a turn in which the opponent plays a land and casts
// spells, and which stops the human nowhere: they hold nothing to cast there. The page starts at
// the human's last decision before that turn, a look at the top of their library.
const turnViews = await load("opponent-turn");

const mainHalt = turnViews.length - 1;

const lookAt = turnViews.findLastIndex((v, i) => i < mainHalt && awaits(v));

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

const jace = await lastDecision("choice-jace");

// The end of a game (`game-over`, under full control): Beza, the Bounding Spring attacks a
// human at 3 life with nothing to block, and its damage ends the game once they pass. The page
// starts at that last pass.
const overFixture = JSON.parse(
    await readFile(join(fixtures, "game-over.json"), "utf8"),
);

const overViews = await load("game-over");

const overEvents = overFixture.messages.filter((m) => m.kind === "event");

const lastPass = overViews.findLastIndex((v) => v.status.kind === "deciding");

const lifeShown = async (page) =>
    Number(
        (
            await page
                .locator('[data-life="you"] .life-orb-total')
                .textContent()
        ).replace("\u2212", "-"),
    );

// The stage's own checks (by-w8gv.21.64): what a key or a press does while a script plays, a
// button over a prompt that holds the table, the pointer over a card a prompt holds up, and a
// page that takes its game up. Each draws the stage whatever the run. A recording made
// before the session held a prompt's state has no reply to a click, which is what the stage
// sends: these answer any.
const anyClick = {
    kind: "response",
    command: "click",
    value: { kind: "answered" },
};

async function clickable(from, name) {
    const { messages } = JSON.parse(
        await readFile(join(fixtures, `${from}.json`), "utf8"),
    );
    await writeFixture(name, { messages: [...messages, anyClick] });
    return name;
}

/** Whether the stage comes to be behind the session (a script is playing, or views wait
 * to), or with `not` to have caught up, within a few frames of the page's clock: the page
 * draws what it was sent on its own timers, which a paused clock holds, and a script takes
 * far longer than these frames to play. */
async function behind(page, not = false) {
    const is = () => page.evaluate(() => window.berylBehind);
    for (let frames = 0; frames < 6 && (await is()) === not; frames++) {
        await page.clock.runFor(16);
    }
    return is();
}

/** The button of `view`'s prompt that the dock puts forward, as a click names it. */
const forward = (view) => ({
    kind: "button",
    button: view.status.decision.prompt.buttons.find((b) => b.primary).id,
});

// Why no audit: what a key does while a script plays: a state between two views, which no screen
// settles on.
await check(
    "Space while a script plays shows its end at once and sends nothing",
    async (page) => {
        await page.clock.runFor(2000);
        // From here on time passes only when the check says so: no script can play on.
        await page.clock.pauseAt(Date.now() + 60_000);
        const answer = turnViews[lookAt].status.decision.prompt.buttons.find(
            (button) => button.primary,
        );
        await page
            .locator(".stage-prompt")
            .getByRole("button", { name: answer.label, exact: true })
            .click();
        assert.ok(await behind(page), "the turn plays");
        const shows = () =>
            page.evaluate(() => {
                const { view } = window.berylStage.shown;
                return [view.turn.number, view.turn.step, view.status.kind];
            });
        const { turn, status } = turnViews[mainHalt];
        const halt = [turn.number, turn.step, status.kind];
        assert.notDeepEqual(await shows(), halt, "the turn is still playing");
        await page.keyboard.press("Space");
        assert.equal(await behind(page, true), false, "the turn is over");
        assert.deepEqual(await shows(), halt, "the turn's last view at once");
        assert.equal((await clicksSent(page)).length, 1, "Space sent nothing");
        // The next press answers what that view asks.
        await page.keyboard.press("Space");
        assert.deepEqual(
            (await clicked(page, 2))[1],
            forward(turnViews[mainHalt]),
            "the halt is answered",
        );
    },
    {
        fixture: await clickable("opponent-turn", "opponent-turn-clicks"),
        at: lookAt + 1,
        ready: ".stage-prompt button",
    },
);

// Why no audit: what a press does while a script plays: a state between two views, which no screen
// settles on.
await check(
    "a land pressed while a script plays on to it is played",
    async (page) => {
        await page.clock.runFor(2000);
        await page.clock.pauseAt(Date.now() + 60_000);
        const land =
            views[replayed.next].status.decision.candidates[replayed.land]
                .option.slot;
        await page.keyboard.press("Space");
        assert.ok(await behind(page), "a script plays");
        assert.deepEqual(
            await clicksSent(page),
            [forward(views[replayed.at])],
            "passed",
        );
        // Pressed while the script plays, which ends; let go on the view it played on to,
        // where the land may be played.
        const box = await page.locator(`[data-stage="${land}"]`).boundingBox();
        await page.mouse.click(box.x + box.width / 2, box.y + 40);
        assert.deepEqual(
            (await clicked(page, 2))[1],
            { kind: "on", on: { kind: "object", slot: land } },
            "the land is played",
        );
        // It was a click on the view the script played on to, which the press showed.
        const on = await page.evaluate(
            () =>
                window.berylFixture.calls.findLast((c) => c.command === "click")
                    .args.request.decision.seq,
        );
        assert.equal(on, views[replayed.next].status.decision.id.seq);
    },
    {
        fixture: await clickable(FIXTURE, `${FIXTURE}-clicks`),
        at: replayed.at + 1,
        ready: ".stage-prompt button",
    },
);

// Why no audit: that a button outside a prompt is reached while the prompt is open, and the command
// it sends.
await check(
    "Undo is pressed over a prompt that holds the table",
    async (page) => {
        await page.clock.runFor(1000);
        assert.ok(
            await page.locator(".stage-tray-pile").first().isVisible(),
            "the prompt holds its cards up",
        );
        await page.getByRole("button", { name: "Undo", exact: true }).click();
        const sent = await page.evaluate(
            () =>
                window.berylFixture.calls.filter((c) => c.command === "undo")
                    .length,
        );
        assert.equal(sent, 1);
    },
    {
        fixture: "choice-jace",
        at: jace.views.length,
        ready: ".stage-tray-pile",
    },
);

// Why no audit: what the pointer's inspector shows over each card of a tray: a state only the
// pointer enters.
await check(
    "the pointer over a card a prompt holds up inspects it",
    async (page) => {
        await page.clock.runFor(2000);
        const view = scry.views.at(-1);
        const held = view.status.decision.prompt.body.piles.flatMap(
            (pile) => pile.cards,
        );
        assert.ok(held.length > 0, "the scry holds cards up");
        for (const slot of held) {
            const { name } = view.objects.find((o) => o.slot === slot);
            const box = await page
                .locator(`[data-stage="${slot}"]`)
                .boundingBox();
            // By its right edge, which a card lying over its neighbor leaves showing.
            await page.mouse.move(
                box.x + box.width - 12,
                box.y + box.height / 2,
            );
            await page.clock.runFor(300);
            await page
                .getByLabel(`${name}, inspected`)
                .waitFor({ state: "visible" });
        }
        await page.mouse.move(1, 1);
        await page.clock.runFor(300);
        assert.equal(
            await page.locator(".inspect").count(),
            0,
            "the inspector goes with the pointer",
        );
    },
    {
        fixture: "choice-scry",
        at: scry.views.length,
        ready: ".stage-tray-pile",
    },
);

// A reloaded page asks for the game in progress (`current_game`) and takes it up: the
// fixture holds no view but that answer, the human's last pass of `game-over`.
await writeFixture("game-over-resumed-clicks", {
    messages: [
        ...overFixture.messages.filter((m) => m.kind === "response"),
        {
            kind: "response",
            command: "current_game",
            value: overEvents[lastPass].payload,
        },
        anyClick,
    ],
});

// Why no audit: a game taken up from the session's answer at launch, with no view sent: what a page
// does as it boots.
await check(
    "a reloaded page takes up the game in progress on the stage",
    async (page) => {
        assert.equal(await lifeShown(page), 3);
        assert.equal(await page.locator(".start-panel").count(), 0);
        await page.clock.runFor(1000);
        await page.keyboard.press("Space");
        assert.deepEqual(
            await clicked(page, 1),
            [forward(overViews[lastPass])],
            "the open prompt is answered",
        );
    },
    {
        fixture: "game-over-resumed-clicks",
        at: 0,
        ready: ".stage-prompt button",
    },
);
