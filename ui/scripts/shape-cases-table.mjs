// What the shape suite (shapes.mjs) needs for the shapes answered on the table itself, with
// the permanents that lie there: a cost paid with permanents (`pay`) and a declaration of
// attackers or blockers (`declare`). What the session's state says the screen draws besides
// marks and buttons (how much of a cost is left, a line from each creature declared to what
// it is aimed at), and two of the five themes the bug reports repeated, each a case played
// on the one recording of its shape: paying with permanents, and the combat line.
import assert from "node:assert/strict";
import { cross } from "./shape-cases-lines.mjs";

const named = (on) => (on.kind === "object" ? String(on.slot) : on.player);

/** How much of the cost `prompt` pays is still to pay, as the session wrote it, or how much
 * of what it divides is still to give out; nothing for a prompt of neither. */
export function leftOf(prompt) {
    const state = prompt?.body;
    if (state?.shape === "divide") return String(state.left);
    // A mana payment's title already says the full cost before the player picks anything.
    if (
        state?.shape === "pay" &&
        prompt.purpose === "mana" &&
        state.left === state.cost
    ) {
        return "";
    }
    return state?.shape === "pay" ? state.left : "";
}

/** The combat damage on its way to the player asked to block, as the blocks `state` has
 * picked leave it (by-w8gv.21.63: the old board's "the unblocked attackers' damage shows by
 * your orb as blocks are declared"): the power of each creature of `view` attacking them
 * that no blocker is yet aimed at, which the screen says by their life total and lowers with
 * each block picked. */
function damageLeft(state, view) {
    const blocked = new Set(
        state.pairs.flatMap(({ to }) =>
            to.kind === "object" ? [to.slot] : [],
        ),
    );
    const attacking = view.objects.filter(
        ({ combat, controller }) =>
            combat?.kind === "attacking" &&
            !combat.targetDeparted &&
            combat.planeswalker === null &&
            controller !== "you",
    );
    return attacking
        .filter(({ slot, combat }) => !combat.blocked && !blocked.has(slot))
        .reduce((sum, { pt }) => sum + Math.max(0, pt?.power ?? 0), 0);
}

/** The line `prompt` draws from each attacker or blocker to what it is aimed at, none to a
 * player (Arena's attacker at the player has only its chevron), and none for a prompt that
 * declares nothing or whose lines the session leaves out: which lines, and each as the
 * stage names it. With them, for a prompt
 * that declares blocks over `view`, the damage still on its way to the player (`damageLeft`),
 * and null for any other, or with no view given. */
export function declared(prompt, view) {
    const state = prompt?.body;
    if (state?.shape !== "declare") {
        return { arrow: null, lines: [], damage: null };
    }
    return {
        arrow: state.arrow,
        damage:
            state.arrow === "block" && view ? damageLeft(state, view) : null,
        lines: (state.lined ? state.pairs : [])
            .filter(({ to }) => to.kind !== "player")
            .map(({ from, to }) => `${state.arrow} ${from} ${named(to)}`)
            .sort(),
    };
}

/** The permanents `prompt` marks with `mark`, by slot. */
const marked = (prompt, mark) =>
    (prompt?.marks ?? []).flatMap((m) =>
        m.mark === mark && m.on.kind === "object" ? [m.on.slot] : [],
    );

/** In the page: how each permanent of `slots` lies and what its mark `mark` looks like: how
 * far it is turned from where it rests, in degrees, and the mark's outline (its colors, at
 * whatever size its card is drawn), ring and symbol as the page computes them. */
function looks({ slots, mark }) {
    const stage = window.berylStage;
    return slots.map((slot) => {
        const node = document.querySelector(`[data-stage="${slot}"]`);
        const drawn = node?.querySelector(`:scope > .stage-mark-${mark}`);
        const style = drawn ? getComputedStyle(drawn) : undefined;
        return {
            slot,
            turned:
                stage.pose(slot).turn - stage.shown.resting.get(slot).pose.turn,
            drawn: drawn !== null && drawn !== undefined,
            outline: style?.boxShadow.replace(/-?[\d.]+px ?/g, "") ?? "none",
            ring: style ? parseFloat(style.borderTopWidth) : 0,
            opacity: style ? Number(style.opacity) : 0,
            symbol: drawn ? getComputedStyle(drawn, "::after").content : "none",
        };
    });
}

/** Paying with permanents, one of the five themes the bug reports repeated, held on the
 * payment `after` shows once the click before it has been drawn (`before` is the prompt it
 * was made on): a crew cost, a waterbend cost and a payment plan are the recording's cases.
 * `dock` finds the layout's buttons. Held to what each report asked:
 * - by-1y4, by-uq8: a click on one permanent picks that one, a land as well as a creature,
 *   and it turns as it would tap: each one picked lies a quarter turned;
 * - by-uld: one picked carries the tap symbol;
 * - by-4hjm, by-z973: the table wears one outline at a time, and nothing that cannot pay
 *   has one: what Auto Pay would tap is outlined as anything else that may be picked, one
 *   picked has no outline, and while the pointer is over Auto Pay those it would tap alone
 *   show, in their ring. (That every mark is the session's, and the amount still to pay
 *   after what the prompt asks, is held of every state by the suite.)
 * - by-szrn, by-tyf: the payment is a prompt the player reviews, with the proposal shown on
 *   Auto Pay and a pick taken back by a click on it: one taken back stands upright again.
 * - a touch held on Auto Pay shows its proposal as the pointer over it does, and accepts
 *   nothing as it is let go.
 * - by-x45, by-hmfq: Auto Pay proposes no creature or artifact for a waterbend cost, and
 *   cannot be pressed while the lands cannot pay what is left.
 * Returns what the state showed of the theme, for the check to hold the recording whole. */
async function payment(page, dock, before, after) {
    const prompt = after.prompt;
    if (prompt?.body.shape !== "pay") return {};
    const what = `paying with permanents (${prompt.purpose}, revision ${prompt.revision})`;
    const [paying, suggested, eligible] = [
        "paying",
        "suggested",
        "eligible",
    ].map((mark) => marked(prompt, mark));
    const read = (slots, mark) => page.evaluate(looks, { slots, mark });
    // A button that comes up under the pointer has it over it: off it first.
    await page.mouse.move(2, 2);
    await page.waitForSelector('[data-pay="quiet"]');
    for (const look of await read(paying, "paying")) {
        assert.equal(look.turned, 90, `${what}: ${look.slot} lies turned`);
        assert.notEqual(look.symbol, "none", `${what}: ${look.slot}'s symbol`);
        assert.equal(look.outline, "none", `${what}: ${look.slot}'s outline`);
        assert.equal(look.ring, 0, `${what}: ${look.slot}'s ring`);
    }
    // One taken back by the click stands as it rests.
    const back =
        before.prompt.body.shape === "pay"
            ? marked(before.prompt, "paying").filter(
                  (slot) => !paying.includes(slot),
              )
            : [];
    for (const look of await read(back, "paying")) {
        assert.equal(look.turned, 0, `${what}: ${look.slot} taken back`);
        assert.equal(look.drawn, false, `${what}: ${look.slot}'s mark is gone`);
    }
    // At rest, one outline: what may be picked, whether or not Auto Pay would tap it.
    const quiet = [
        ...(await read(suggested, "suggested")),
        ...(await read(eligible, "eligible")),
    ];
    for (const look of quiet) {
        assert.equal(look.ring, 0, `${what}: ${look.slot} wears no ring`);
        assert.equal(look.outline, quiet[0].outline, `${what}: one outline`);
        assert.notEqual(
            look.outline,
            "none",
            `${what}: ${look.slot}'s outline`,
        );
        assert.equal(look.opacity, 1, `${what}: ${look.slot}'s outline shows`);
    }
    const auto = prompt.buttons.find((button) => button.id.kind === "auto");
    const lands = (slots) =>
        slots.every((slot) =>
            after.view.objects
                .find((object) => object.slot === slot)
                .types.includes("land"),
        );
    if (prompt.purpose === "waterbend") {
        assert.ok(lands(suggested), `${what}: Auto Pay proposes lands alone`);
        assert.equal(
            auto.enabled,
            suggested.length > 0,
            `${what}: Auto Pay can be pressed when lands pay what is left`,
        );
    }
    if (auto?.enabled && suggested.length > 0) {
        // The pointer over Auto Pay shows what it would tap, and nothing else outlined.
        await page
            .locator(dock)
            .getByRole("button", { name: auto.label, exact: true })
            .hover();
        await page.waitForSelector('[data-pay="suggesting"]');
        for (const look of await read(suggested, "suggested")) {
            assert.ok(look.ring > 0, `${what}: ${look.slot} in its ring`);
            assert.equal(
                look.outline,
                "none",
                `${what}: ${look.slot}, no outline`,
            );
        }
        for (const look of await read(eligible, "eligible")) {
            assert.equal(look.opacity, 0, `${what}: ${look.slot} is put out`);
        }
        await page.mouse.move(2, 2);
        await page.waitForSelector('[data-pay="quiet"]');
        // A touch held on Auto Pay shows the same, and let go it accepts nothing: the
        // click that ends a hold is no press (src/stage/PromptBar.tsx usePresses). The hold
        // is the page's own timer, and the proposal showing is what says it ran out.
        const button = page
            .locator(dock)
            .getByRole("button", { name: auto.label, exact: true });
        const sent = () =>
            page.evaluate(() => window.berylFixture.calls.length);
        const had = await sent();
        await button.dispatchEvent("pointerdown", { pointerType: "touch" });
        await page.waitForSelector('[data-pay="suggesting"]');
        await button.dispatchEvent("pointerup", { pointerType: "touch" });
        await button.dispatchEvent("click", { detail: 1 });
        await page.waitForSelector('[data-pay="quiet"]');
        assert.equal(await sent(), had, `${what}: a held touch let go`);
    }
    return {
        [prompt.purpose]: true,
        picked: paying.length > 0,
        back: back.length > 0,
        proposed: auto?.enabled && suggested.length > 0,
        refused: auto !== undefined && !auto.enabled && paying.length === 0,
    };
}

/** In the page: plays out what a click led to a tick at a time, and returns what each tick
 * showed: whether combat's damage has begun (a creature has struck, or the table is past the
 * steps that declare), how far each permanent of `slots` lies turned from upright, in degrees, and of
 * each line of `keys` whether the model holds it, where the page draws its two ends and
 * whether it draws it as the element it was first drawn as (`window.berylLinesHeld`, which the first call makes of the
 * lines then drawn). */
async function ticked({ tick, slots, keys }) {
    const stage = window.berylStage;
    await new Promise((resolve) => setTimeout(resolve));
    let struck = false;
    const unfollow = stage.follow(({ edge, step }) => {
        if (edge === "start" && step.act.verb === "strike") struck = true;
    });
    const declaring = ["declareAttackers", "declareBlockers"];
    const drawn = (key) => document.querySelector(`[data-line="${key}"]`);
    window.berylLinesHeld ??= new Map();
    const held = window.berylLinesHeld;
    for (const key of keys) if (!held.has(key)) held.set(key, drawn(key));
    const sample = () => {
        const model = new Set(stage.lines().map((line) => line.key));
        return {
            at: stage.clock.now,
            damage: struck || !declaring.includes(stage.shown.table?.turn.step),
            // A tapped permanent is dressed lying, a quarter turned as it rests.
            turns: slots.map((slot) => {
                const lying = stage.shown.resting.get(slot)?.object?.tapped;
                return (stage.pose(slot)?.turn ?? NaN) + (lying ? 90 : 0);
            }),
            lines: keys.map((key) => {
                const numbers = (
                    drawn(key)
                        ?.getAttribute("d")
                        ?.match(/-?[\d.]+/g) ?? []
                ).map(Number);
                const point = (i) => ({
                    x: numbers.at(i),
                    y: numbers.at(i + 1),
                });
                return {
                    key,
                    model: model.has(key),
                    same:
                        held.get(key) !== null && drawn(key) === held.get(key),
                    ends: [point(0), point(-2)],
                };
            }),
        };
    };
    const seen = [sample()];
    for (let i = 0; stage.shown.busy && i < 4000; i++) {
        stage.clock.advance(tick);
        seen.push(sample());
    }
    unfollow();
    return seen;
}

/** The tap that pays, the end of the theme: `page` has just sent the click that answered the
 * payment `before` shows. A permanent picked to pay lay turned from its pick, and the script
 * that taps it does not stand it up to turn it again (by-1y4: "they physically tap to show
 * their state"): at every tick from the answer on it lies as it lay. */
async function tapThatPays(page, before, tick) {
    if (before.prompt.body.shape !== "pay") return;
    const slots = marked(before.prompt, "paying");
    const seen = await page.evaluate(ticked, { tick, slots, keys: [] });
    for (const [i, slot] of slots.entries()) {
        const turns = [...new Set(seen.map((sample) => sample.turns[i]))];
        assert.deepEqual(
            turns,
            [seen[0].turns[i]],
            `${slot}, picked to pay, turned again as it tapped`,
        );
    }
}

/** Paying with permanents, watched on a page whose dock's buttons `dock` finds: `answered`
 * plays out the tap after a click that answered the payment `before` shows, `drawn` holds
 * the payment `after` shows once the screen has drawn it, and `done` holds that a recording
 * with a payment in it showed every case of the theme. */
export function payingWithPermanents(page, dock, tick) {
    const seen = new Set();
    return {
        answered: (before) => tapThatPays(page, before, tick),
        async drawn(before, after) {
            const showed = await payment(page, dock, before, after);
            for (const [what, did] of Object.entries(showed)) {
                if (did) seen.add(what);
            }
        },
        done() {
            if (seen.size === 0) return;
            const cases = ["crew", "waterbend", "mana", "picked", "back"];
            assert.deepEqual(
                [...cases, "proposed", "refused"].filter((c) => !seen.has(c)),
                [],
                "paying with permanents: cases the recording never showed",
            );
        },
    };
}

/** The combat line, one of the five themes the bug reports repeated (by-w8gv.21.22 holds the
 * script's side of it: UI Conventions, "A target that moves"), from the prompt that declares
 * it. A line is drawn as its creature is aimed (the suite holds every state's lines to the
 * session's pairs), and from then to the damage it is one line: the element the click drew,
 * never taken off the page and drawn again as the declaration is confirmed, as the other
 * player answers it or as priority passes. As the damage comes, no line of the attack
 * crosses another. One of these watches a page: `follow` plays out
 * what each click led to, and `done` says the lines it watched were all seen to the damage. */
export function combatLines(page, tick) {
    /** The lines watched, and whether each has been seen to the damage. */
    const watched = new Map();
    let declarations = 0;
    return {
        /** `page` has just sent a click on the prompt `before` shows, which `answered`
         * it or did not. */
        async follow(before, answered) {
            const { lines } = declared(before.prompt);
            if (answered && lines.length > 0) {
                declarations += 1;
                for (const key of lines) watched.set(key, false);
            }
            const keys = [...watched]
                .filter(([, damaged]) => !damaged)
                .map(([key]) => key);
            if (!answered || keys.length === 0) return;
            const seen = await page.evaluate(ticked, { tick, slots: [], keys });
            // As the damage comes the attackers stand in the order of what they attack
            // (src/game/battlefield.ts permanentRow), so no line of theirs crosses another.
            const last = seen.findLast((sample) => !sample.damage);
            const attacks = (last?.lines ?? []).filter(({ key }) =>
                key.startsWith("attack "),
            );
            for (const [i, one] of attacks.entries()) {
                for (const other of attacks.slice(i + 1)) {
                    assert.ok(
                        !cross(one.ends, other.ends),
                        `the lines ${one.key} and ${other.key} cross at ${last.at} ms`,
                    );
                }
            }
            for (const [i, key] of keys.entries()) {
                for (const sample of seen) {
                    if (sample.damage) {
                        watched.set(key, true);
                        break;
                    }
                    const what = `the line ${key} at ${sample.at} ms`;
                    assert.ok(sample.lines[i].model, `${what}: ended`);
                    assert.ok(sample.lines[i].same, `${what}: drawn again`);
                }
            }
        },
        /** Holds that a recording that declared followed an attack's lines and a block's,
         * each to its damage. */
        done() {
            const short = [...watched].filter(([, damaged]) => !damaged);
            assert.deepEqual(short, [], "lines never seen to the damage");
            if (declarations > 0) {
                assert.equal(declarations, 2, "an attack and a block declared");
            }
        },
    };
}
