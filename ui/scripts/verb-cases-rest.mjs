// The verb suite's cases for the verbs that move no card: `streak`, mana into what
// it pays for and damage onto what it hits; `announce`, the turn's banner, a scry's pill and a
// coin; `dwell`, the opponent's spell or ability left to be read; and `set`, the table
// changing with no motion. A run's `ticks` hold, for every tick the clock took, each streak
// the stage drew then with the element that drew it and how far through each announcement the
// table was written to be (verb-rest.mjs, which holds the page to the model there); its
// samples at an announcement's middle hold what the page showed of it. What is asked here is
// what only a whole run shows. Nothing here touches a page.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { complete, effectsOn } from "./verb-run.mjs";

const { verbs } = JSON.parse(
    readFileSync(
        new URL("../../data/ui-conventions.json", import.meta.url),
        "utf8",
    ),
);
/** The design language's duration of `verb`'s `kind`, in stage ms. */
function durationOf(verb, kind) {
    const { values, cases = {} } = verbs.find((v) => v.name === verb);
    return cases[kind]?.duration ?? values.duration;
}

/** What a streak is made of, as the stage names its look (src/stage/streaks.ts). */
const lookOf = ({ stream }) =>
    stream.kind === "mana" ? stream.color : "damage";

/** What the stage names a script's subject (src/stage/spans.ts `namedBy`). */
const named = (subject) =>
    subject.kind === "object"
        ? subject.slot
        : subject.kind === "ability"
          ? `a${subject.id}`
          : subject.player;

/** Every step of `verb` that `run` was told of, from its edges: when it began and ended, and
 * the steps that began while it played. */
function played(run, verb) {
    const starts = run.edges.filter((e) => e.edge === "start");
    return starts
        .filter((e) => e.verb === verb)
        .map((e) => ({
            act: e.act,
            from: e.at,
            to: e.at + e.span,
            under: starts.filter((o) => o.at > e.at && o.at < e.at + e.span),
        }));
}

/** The sample `run` took at the stage time `at`, if it took one. */
const sampleAt = (run, at) => run.samples.find((sample) => sample.at === at);

/** Every run a check was given, by its moment's name, each with every sample it took. */
const runsOf = (runs) =>
    Object.entries(runs)
        .filter(([, run]) => run?.edges)
        .map(([name, run]) => [name, run.samples ? run : complete(run)]);

/** Nothing the stage holds was anywhere else at `after` than at `before`. */
function still(before, after, what) {
    for (const [name, pose] of Object.entries(before.poses)) {
        if (!(name in after.poses)) continue;
        assert.deepEqual(after.poses[name], pose, `${what}: ${name} moved`);
    }
}

export const CASES = {
    // A streak is one comet from the step that draws it to its landing: the stage holds it
    // at every tick of its step and at none after, one element draws it, its head goes on
    // toward what it lands on and is there at the last, and what it lands on flashes as it
    // lands. The streaks of one payment leave together. Mana from a Treasure sacrificed for
    // it leaves where the Treasure lay, and damage at a player takes their life as it lands.
    streak(runs) {
        const seen = { mana: 0, damage: 0, gone: 0 };
        for (const [name, run] of runsOf(runs)) {
            const streaks = played(run, "streak");
            // Every streak tick is some step's.
            for (const tick of run.ticks) {
                for (const held of tick.streaks) {
                    const mine = streaks.some(
                        (s) => s.from <= tick.at && tick.at < s.to,
                    );
                    assert.ok(mine, `${name}: a streak at ${tick.at} ms`);
                }
            }
            for (const [i, { act, from, to }] of streaks.entries()) {
                const what = `${name}: the streak at ${from} ms`;
                const kind = act.stream.kind;
                assert.equal(to - from, durationOf("streak", kind), what);
                seen[kind] += 1;
                // Those of one beat, one after another in the script, leave together.
                const beside = streaks[i - 1];
                if (beside && beside.to > from && kind === "mana") {
                    assert.equal(beside.from, from, `${what} leaves alone`);
                }
                const look = lookOf(act);
                const on = named(act.to);
                // The stage holds its streaks in the order they began: among those of its
                // beat alike in look and target, it is the one of its place.
                const twins = streaks.filter(
                    (s) =>
                        s.from === from &&
                        lookOf(s.act) === look &&
                        named(s.act.to) === on,
                );
                const nth = twins.findIndex((s) => s.act === act);
                const during = run.ticks.filter(
                    (tick) => from < tick.at && tick.at < to,
                );
                assert.ok(during.length > 1, `${what} was not seen`);
                const held = during.map((tick) => {
                    const found = tick.streaks.filter(
                        (s) => s.look === look && s.to === on,
                    )[nth];
                    assert.ok(found, `${what} is not held at ${tick.at} ms`);
                    return found;
                });
                assert.equal(
                    new Set(held.map((s) => s.element)).size,
                    1,
                    `${what} was drawn by more than one element`,
                );
                for (const [k, streak] of held.entries()) {
                    const through = (during[k].at - from) / (to - from);
                    assert.ok(
                        streak.through > 0 && streak.through <= through + 1e-9,
                        `${what} is ${streak.through} through at ${through} of its step`,
                    );
                    assert.ok(
                        streak.through > (held[k - 1]?.through ?? 0),
                        `${what} does not go on`,
                    );
                }
                const after = run.ticks.find((tick) => tick.at >= to);
                const left = after?.streaks.find(
                    (s) => s.element === held[0].element,
                );
                assert.equal(left, undefined, `${what} outlives its step`);
                if (!act.from) {
                    seen.gone += 1;
                    assert.ok(
                        held.every((s) => !s.drawn),
                        `${what} is drawn from nowhere`,
                    );
                } else {
                    assert.ok(
                        held.every((s) => s.drawn),
                        `${what} is not drawn`,
                    );
                    // Its head closes on what it lands on, and is nearly there at the last.
                    const away = held.map((s) =>
                        Math.hypot(
                            s.head.x - s.target.x,
                            s.head.y - s.target.y,
                        ),
                    );
                    for (const [k, far] of away.entries()) {
                        assert.ok(
                            k === 0 || far < away[k - 1],
                            `${what} turns back at ${during[k].at} ms`,
                        );
                    }
                    const last = held.at(-1);
                    const short = (1 - last.through) * away[0] * 1.5;
                    assert.ok(
                        away.at(-1) <= short + 0.25 * last.target.reach,
                        `${what} ends ${away.at(-1)} from what it lands on`,
                    );
                }
                // What it lands on flashes as it lands; mana's source glows as it leaves.
                const landed = sampleAt(run, to);
                assert.ok(landed, `${what}: its landing was not sampled`);
                assert.ok(
                    effectsOn(landed, on).includes(
                        kind === "mana" ? "flash over" : "impact over",
                    ),
                    `${what}: ${on} shows ${effectsOn(landed, on)} as it lands`,
                );
                if (kind === "damage" && act.to.kind === "player") {
                    // The life it takes counts as it lands, and the total flares.
                    const [count] = played(run, "count").filter(
                        (c) =>
                            c.from === to &&
                            c.act.tally.kind === "life" &&
                            c.act.tally.player === on,
                    );
                    assert.ok(count, `${what}: no life counts as it lands`);
                    assert.equal(landed.life[on], count.act.tally.life, what);
                    assert.ok(
                        effectsOn(landed, on).includes("loss over"),
                        `${what}: the total does not flare`,
                    );
                }
            }
        }
        assert.ok(seen.mana > 0, "no moment streaks mana");
        // Mana Crypt's lost coin deals its controller three (the board verbs-burn).
        assert.equal(seen.damage, 1, "the streaks of damage");
        assert.equal(seen.gone, 0, "a streak with no source");
        // The Treasure sacrificed for Oust (the board verbs-combat): its mana leaves where
        // it lay, as it goes.
        const [paid] = played(runs.combat, "streak").filter(
            ({ act }) => act.from?.kind === "object",
        );
        assert.ok(paid, "the Treasure's mana names no source");
        const leaves = runs.combat.edges.find(
            (e) =>
                e.edge === "start" &&
                e.verb === "vanish" &&
                e.act.subject.kind === "object" &&
                e.act.subject.slot === paid.act.from.slot,
        );
        assert.ok(
            leaves &&
                leaves.at <= paid.from &&
                paid.from < leaves.at + leaves.span,
            "the Treasure's mana does not leave as the Treasure goes",
        );
    },

    // The turn's banner and a scry's pill pass over the table: each is shown for as long as
    // its step, play goes on under it, and the stage writes how far through it is at every
    // tick, from which the page draws it with Arena's words. A coin is waited for.
    announce(runs) {
        const seen = { turn: 0, scry: 0, coin: 0 };
        const said = { turn: 0, scry: 0, coin: 0 };
        for (const [name, run] of runsOf(runs)) {
            for (const { act, from, to, under } of played(run, "announce")) {
                const { kind } = act.notice;
                const what = `${name}: the ${kind} announced at ${from} ms`;
                seen[kind] += 1;
                const length = durationOf(
                    "announce",
                    { turn: "turn banner", scry: "scry pill", coin: "coin" }[
                        kind
                    ],
                );
                assert.equal(to - from, length, what);
                // Written on the table at every tick of it, as far through as the clock is.
                const during = run.ticks.filter(
                    (tick) => from < tick.at && tick.at < to,
                );
                for (const tick of during) {
                    const written = tick.notices.find((n) => n.kind === kind);
                    assert.ok(
                        written &&
                            Math.abs(
                                written.through - (tick.at - from) / length,
                            ) < 1e-9,
                        `${what} is written ${JSON.stringify(written)} at ${tick.at} ms`,
                    );
                }
                const over = run.ticks.filter((tick) => tick.at >= to);
                const next = played(run, "announce").find(
                    (other) =>
                        other.act.notice.kind === kind && other.from >= to,
                );
                for (const tick of over) {
                    if (next && tick.at >= next.from) break;
                    assert.ok(
                        !tick.notices.some((n) => n.kind === kind),
                        `${what} is still up at ${tick.at} ms`,
                    );
                }
                if (kind === "coin") {
                    assert.deepEqual(
                        under.map((e) => e.verb),
                        [],
                        `${what}: play went on before it landed`,
                    );
                }
                // What the page shows of it half way: its words, whole.
                const shown = sampleAt(run, (from + to) / 2)?.notices?.find(
                    (n) => n.kind === kind,
                );
                if (!shown) continue;
                said[kind] += 1;
                assert.ok(
                    shown.opacity > 0.99,
                    `${what} shows ${shown.opacity}`,
                );
                if (kind === "turn") {
                    const whose = act.notice.active;
                    assert.equal(shown.side, whose, what);
                    assert.deepEqual(
                        shown.words,
                        [whose === "you" ? "Your Turn" : "Opponent's Turn"],
                        what,
                    );
                    assert.equal(shown.down, 0, `${what} has begun to go`);
                } else if (kind === "scry") {
                    const { top, bottom } = act.notice;
                    assert.deepEqual(
                        shown.words,
                        [`Scry: ${top} top, ${bottom} bottom.`],
                        what,
                    );
                } else {
                    const who =
                        act.notice.player === "you" ? "You" : "Opponent";
                    const won = act.notice.won ? "won" : "lost";
                    assert.equal(
                        shown.words[1],
                        `${who} ${won} the flip`,
                        what,
                    );
                }
            }
        }
        // The board verbs-burn: a turn begins and Mana Crypt flips its coin; verbs-scry: a
        // scry of two.
        for (const kind of ["turn", "scry", "coin"]) {
            assert.ok(seen[kind] > 0, `no moment announces a ${kind}`);
            assert.ok(said[kind] > 0, `no ${kind} was read on the page`);
        }
        // Play goes on under a turn's banner: the turn's first steps begin while it is up.
        const [banner] = played(runs.burn, "announce").filter(
            ({ act }) => act.notice.kind === "turn",
        );
        assert.ok(
            banner.under.length > 0,
            "nothing played under the turn's banner",
        );
    },

    // The opponent's spell or ability is left to be read, for the design language's time,
    // while nothing on the table moves; yours never is.
    dwell(runs) {
        let seen = 0;
        for (const [name, run] of runsOf(runs)) {
            for (const { act, from, to } of played(run, "dwell")) {
                const what = `${name}: the dwell at ${from} ms`;
                seen += 1;
                assert.equal(to - from, durationOf("dwell"), what);
                // What is read is the opponent's, and lies on the stack throughout.
                const on = named(act.subject);
                const put = run.edges.findLast(
                    (e) =>
                        e.edge === "start" &&
                        e.at <= from &&
                        ((e.verb === "fly" &&
                            e.act.object.slot === on &&
                            e.act.object.zone === "stack") ||
                            (e.verb === "emerge" && `a${e.act.id}` === on)),
                );
                assert.ok(put, `${what}: nothing put ${on} on the stack`);
                const whose =
                    put.verb === "fly"
                        ? put.act.object.controller
                        : put.act.item.controller;
                assert.equal(whose, "opponent", `${what} is on your own`);
                // A sample at its end has the next beat begun: half way is the last of it.
                const [first, last] = [from, (from + to) / 2].map((at) =>
                    sampleAt(run, at),
                );
                assert.ok(first && last, `${what} was not sampled`);
                assert.equal(first.zones[on], "stack", what);
                assert.equal(last.zones[on], "stack", what);
                still(first, last, what);
                // And no step that moves a card begins meanwhile.
                const moving = run.edges.filter(
                    (e) =>
                        e.edge === "start" &&
                        e.at >= from &&
                        e.at < to &&
                        !["dwell", "announce", "point", "mark"].includes(
                            e.verb,
                        ),
                );
                assert.deepEqual(
                    moving.map((e) => e.verb),
                    [],
                    `${what}: play went on`,
                );
            }
        }
        // Each seat watches the other's spells in some moment (the boards verbs-zones and
        // verbs-combat: the opponent's Oust, and their abilities from the seat across).
        assert.ok(seen > 0, "no moment leaves a spell to be read");
    },

    // The table changes and nothing moves: a step of the turn begins in the design
    // language's moment, and a library is shuffled in none.
    set(runs) {
        let seen = 0;
        for (const [name, run] of runsOf(runs)) {
            for (const { act, from, to } of played(run, "set")) {
                const what = `${name}: the set of ${act.fact.kind} at ${from} ms`;
                if (act.fact.kind === "turn") {
                    seen += 1;
                    assert.equal(
                        to - from,
                        durationOf("set", "step begins"),
                        what,
                    );
                } else if (act.fact.kind === "library") {
                    assert.equal(to - from, 0, what);
                } else {
                    assert.equal(to - from, durationOf("set"), what);
                }
            }
            // A step's beginning alone moves no node: over each one that no other step
            // shares its time with, every pose is as it was.
            const others = run.edges.filter(
                (e) => e.edge === "start" && e.verb !== "set" && e.span > 0,
            );
            for (const { act, from, to } of played(run, "set")) {
                if (act.fact.kind !== "turn" || to === from) continue;
                const alone = !others.some(
                    (o) => o.at < to && o.at + o.span > from,
                );
                const [first, last] = [from, (from + to) / 2].map((at) =>
                    sampleAt(run, at),
                );
                if (alone && first && last) {
                    still(first, last, `${name}: the step at ${from} ms`);
                }
            }
        }
        assert.ok(seen > 0, "no moment begins a step");
    },
};
