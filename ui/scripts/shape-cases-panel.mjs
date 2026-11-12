// What the shape suite (shapes.mjs) needs for the shapes answered off the cards on the table:
// a number set, an entry of a catalog chosen and an amount divided among recipients. What the
// session's state says the screen draws of each besides marks and buttons (the number set,
// the entries with the one chosen, each recipient's share), the gesture that makes a recorded
// click of each, and what the seat across the table is watched for.
import assert from "node:assert/strict";

/** What `prompt` has set, as the session's state says it: a number prompt's value, a
 * catalog's entries with the one chosen in brackets, and each recipient's share of a
 * division; none of them for any other prompt. */
export function panelOf(prompt) {
    const state = prompt?.state ?? {};
    return {
        value: state.shape === "number" ? String(state.value) : null,
        entries:
            state.shape === "catalog"
                ? state.entries.map((entry, i) =>
                      i === state.chosen ? `[${entry}]` : entry,
                  )
                : [],
        shares:
            state.shape === "divide"
                ? state.shares
                      .map(({ on, amount }) => `${on.slot}=${amount}`)
                      .sort()
                : [],
    };
}

/** Makes the click `click`, of a number or of a catalog's entry, on the prompt as `before`
 * has it: an entry by a press of it; a number by a press of the step beside what it is now,
 * the dock's for a number prompt and the recipient's own for a share of a division. A step
 * goes to the next number, so the recording's clicks are held to go one at a time. */
export async function set(page, click, before) {
    const { state } = before.prompt;
    if (click.kind === "entry") {
        await page.locator(`.stage-entry[data-entry="${click.index}"]`).click();
        return;
    }
    const [now, steps] =
        click.on === null
            ? [state.values.indexOf(state.value), ".prompt-stepper"]
            : [
                  state.shares.find(({ on }) => on.slot === click.on.slot)
                      .amount,
                  `.stage-share[data-share="${click.on.slot}"]`,
              ];
    const to = click.on === null ? state.values.indexOf(click.n) : click.n;
    assert.equal(Math.abs(to - now), 1, `a step from ${now} to ${to}`);
    const step = page
        .locator(steps)
        .getByRole("button", { name: to > now ? "More" : "Less", exact: true });
    // Every layout's steps are where the pointer reaches them.
    await step.click();
}

/** In the page: plays the recording on from its first view to the next that asks its seat,
 * stepping the stage's clock, and returns what any tick before that view was shown held of a
 * prompt: the marks in the model and on the page, the dock's buttons that could be pressed,
 * and anything a prompt draws over the table to press. */
export async function watching({ tick, dock }) {
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
        const buttons = [
            ...[...document.querySelectorAll(dock)]
                .flatMap((part) => [...part.querySelectorAll("button")])
                .filter((button) => !button.disabled),
            ...document.querySelectorAll(".stage-entry, .stage-share"),
        ].map((button) => button.textContent);
        if (marks.length || buttons.length) {
            seen.push({ at: stage.clock.now, marks, buttons });
        }
    }
    stage.settle();
    return { seen, done: stage.shown.view === last };
}

/** The key that presses a button of the desktop's dock for a player with no control
 * in focus (src/stage/PromptBar.tsx usePromptKeys): Space for the one put forward, Shift with
 * it for the one that passes on; none for any other, or for a way to use a card, which stands
 * over the table. The focus is taken off whatever the last click left it on. */
export async function keyOf(page, prompt, click) {
    const pressed = prompt.buttons.find(
        (button) => JSON.stringify(button.id) === JSON.stringify(click.button),
    );
    const key = pressed.primary
        ? "Space"
        : pressed.id.kind === "passOn" && "Shift+Space";
    const stands = await page
        .locator(".stage-prompt")
        .getByRole("button", { name: pressed.label, exact: true })
        .count();
    if (!key || !stands) return null;
    await page.evaluate(() => document.activeElement?.blur());
    return key;
}
