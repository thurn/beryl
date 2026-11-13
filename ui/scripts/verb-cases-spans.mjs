// The verb suite's cases for what a script begins and ends (plan K23): `point`, the arrows
// and lines of the moments it watches, and `mark`, played from the script verbs.mjs writes by
// hand over the table its first moment leaves. A run's `ticks` hold, for every tick the clock
// took, each line and mark the stage held then with the number of the element that drew it
// (verbs.mjs `spans`); the page is held to the model there, each line's ends to the edges of
// the two things it is between as the model has them, so a run with no fault has drawn every
// line where its ends were at every tick. What is asked here is what only a whole run shows:
// that a span is one element from the step that begins it to the step that ends it, and the
// two themes the reports kept coming back to. Nothing here touches a page.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

/** How long after an ability lifts its line to its source begins, in stage ms
 * (data/ui-conventions.json, `point`'s "source line" case): Arena's starts as the copy nears
 * the stack. */
const SOURCE_LINE_DELAY = JSON.parse(
    readFileSync(
        new URL("../../data/ui-conventions.json", import.meta.url),
        "utf8",
    ),
).verbs.find((verb) => verb.name === "point").cases["source line"].delay;

/** What the stage names a script's subject (src/stage/spans.ts `namedBy`). */
const named = (subject) =>
    subject.kind === "object"
        ? subject.slot
        : subject.kind === "ability"
          ? `a${subject.id}`
          : subject.player;

/** What names a span among those the stage holds (src/stage/spans.ts). */
const keyOf = (act) =>
    act.verb === "point"
        ? `${act.arrow} ${named(act.from)} ${named(act.to)}`
        : `${act.mark} ${named(act.on)}`;

/** The spans of `verb` that `run`'s steps began or ended, in the order they ended, those
 * still open last: each from the stage time its beginning step began to the time the step
 * that ended it was over, and `whole` from when that beginning step was over. One the first view
 * came with has no beginning told, and one the run leaves open no end. */
function lifetimes(run, verb) {
    const open = new Map();
    const over = [];
    for (const { edge, at, act, span } of run.edges) {
        if (act.verb !== verb || edge !== "start") continue;
        const key = keyOf(act);
        if (act.span === "begin") {
            open.set(key, { key, act, from: at, whole: at + span });
        } else {
            // It is over as the step that ends it is (src/stage/spans.ts).
            const life = open.get(key) ?? { key, act, from: -Infinity };
            open.delete(key);
            over.push({ whole: life.from, ...life, to: at + span });
        }
    }
    return [
        ...over,
        ...[...open.values()].map((l) => ({ ...l, to: Infinity })),
    ];
}

/** What `tick` holds of the span `life`, a line or a mark. */
const heldAt = (tick, { key, act }) =>
    act.verb === "point"
        ? tick.lines.find((line) => line.key === key)
        : tick.marks.find(
              (mark) => mark.mark === act.mark && mark.on === named(act.on),
          );

/** The span `life` of `run` is one thing for as long as it lasts: the stage holds it at every
 * tick from its beginning to its end, drawn by one element of the page, and at no tick
 * between one of its lifetimes and the next (`lives`, every lifetime of its key). Returns
 * what each tick of it held. */
function continuous(run, life, lives) {
    const elements = new Set();
    const held = [];
    for (const tick of run.ticks) {
        const found = heldAt(tick, life);
        // A tick taken at the very time of its beginning or its end may be from before the
        // step played: the clock stood there between two scripts.
        if (life.from < tick.at && tick.at < life.to) {
            assert.ok(found, `${life.key} is not held at ${tick.at} ms`);
            elements.add(found.element);
            held.push({ at: tick.at, ...found });
        } else if (
            !lives.some((other) => other.from <= tick.at && tick.at <= other.to)
        ) {
            assert.equal(found, undefined, `${life.key} at ${tick.at} ms`);
        }
    }
    assert.ok(
        elements.size <= 1 && !elements.has(null),
        `${life.key} was drawn by ${elements.size} elements from ${life.from} to ${life.to} ms`,
    );
    return held;
}

/** Every span of `verb` in `run`, each held to `continuous`, with what its ticks held. */
function spansOf(run, verb) {
    const lives = lifetimes(run, verb);
    return lives.map((life) => ({
        ...life,
        held: continuous(
            run,
            life,
            lives.filter((other) => other.key === life.key),
        ),
    }));
}

const arrowAt = (arrow) =>
    arrow === "friendly-target" || arrow === "hostile-target";

/** Every line of `run` is drawn whenever it could be: once it has come in whole, at every
 * tick but those of a prompt, which hides the line to a source, and those in which a creature
 * at one of its ends strikes (it closes on what it strikes, and a line between two things
 * that meet has nothing to draw); and the line to a source from its last tick back to where
 * its ability came clear of its source. Returns the run's lines. */
function drawn(run) {
    const lines = spansOf(run, "point");
    const strikes = [];
    for (const { edge, at, act, span } of run.edges) {
        if (act.verb !== "strike" || edge !== "start") continue;
        const ends = [act.source, named(act.to)];
        strikes.push({ ends, from: at, to: at + span });
    }
    const striking = (line, at) =>
        strikes.some(
            (strike) =>
                strike.from <= at &&
                at <= strike.to &&
                [line.from, line.to].some((end) => strike.ends.includes(end)),
        );
    for (const { key, act, whole, held } of lines) {
        const after = held.filter(
            (tick) =>
                tick.at >= whole && !tick.hidden && !striking(tick, tick.at),
        );
        if (act.arrow !== "source") {
            const blank = after.find((tick) => !tick.drawn);
            assert.equal(blank, undefined, `${key} is not drawn`);
            continue;
        }
        // An ability that lifts out of its source has no line to it until it is clear of it,
        // and from then always.
        const first = after.findIndex((tick) => tick.drawn);
        if (after.length) assert.ok(first >= 0, `${key} was never drawn`);
        const blank = after.slice(first).find((tick) => !tick.drawn);
        assert.equal(blank, undefined, `${key} went and came back`);
    }
    return lines;
}

/** The theme of six reports (by-yfw, by-ad8, by-8uw, by-2wsl, by-gfjy, by-pk2; UI Conventions,
 * "A target that moves"): an arrow left on a card that moved, drawn at it in its graveyard or
 * hand, or not at its target at all. The moment `zones` aims three spells and abilities at
 * things that then leave: a spell countered, a permanent exiled, a spell returned to its
 * hand. Stepping the clock from each arrow's beginning to its end, one element draws it at
 * every tick (`continuous`), with its ends where the model has its two cards (the run has no
 * fault); the card it points at lies in one zone and, where the table is not laid out again
 * under it, in one place, for as long; and once the arrow has ended nothing points at that
 * card again, wherever it went. */
function arrowsOnCardsThatMoved(run, reflows) {
    const arrows = drawn(run).filter(({ act }) => arrowAt(act.arrow));
    assert.ok(arrows.length >= 3, `${arrows.length} arrows at targets`);
    const left = [];
    for (const { key, act, whole, to, held } of arrows) {
        assert.ok(to < Infinity, `${key} never ended`);
        const target = named(act.to);
        const zones = new Set(held.map((tick) => tick.zones[1]));
        assert.equal(zones.size, 1, `${key}: its target lay in ${[...zones]}`);
        const stood = held.filter((tick) => tick.at >= whole);
        if (!reflows) {
            for (const tick of stood) {
                assert.deepEqual(
                    tick.ends[1],
                    stood[0].ends[1],
                    `${key}: its target moved under it at ${tick.at} ms`,
                );
            }
        }
        for (const tick of run.ticks.filter((tick) => tick.at >= to)) {
            const at = tick.lines.find(
                (line) => line.to === target && arrowAt(line.arrow),
            );
            assert.equal(at, undefined, `${key} was drawn again at ${tick.at}`);
        }
        // What it pointed at then went: a step moves it once the arrow has ended, never before.
        const moves = run.edges.find(
            (edge) =>
                edge.edge === "start" &&
                edge.at >= whole &&
                (edge.act.verb === "vanish" || edge.act.verb === "fly") &&
                (edge.act.object?.slot ?? edge.act.subject?.slot) === target,
        );
        if (moves) {
            assert.ok(moves.at >= to, `${key} was still up as its target left`);
            left.push(target);
        }
    }
    assert.ok(left.length >= 3, `${left.length} targets left`);
}

/** The theme of five reports (by-y3sp, by-kq8i, by-m0wf, by-mm03, by-l26; UI Conventions,
 * "A creature that enters attacking"): the line of combat that went away and came back as
 * the game moved through its steps, or showed late. Over a whole combat (`combat`: three
 * attackers declared one by one, one at a planeswalker, a block and combat damage) and one a
 * trigger resolves in the middle of (`piles`: an attack trigger's token enters attacking),
 * stepping the clock from each line's beginning to its end one element draws it at every
 * tick, across every script between, with its ends where the model has the creatures as
 * they turn and their rows make room (the runs have no fault). Each attacker's line is
 * begun once, at one thing, and is whole within a tenth of a second of the step that begins
 * it. */
function combatLinesThatFlickered(runs) {
    // Only an attacker at a planeswalker has a line (Arena, UrGalw_f35w 48:56.6): one of
    // `combat`'s three. `piles`' token attacks the player and has none.
    for (const [moment, most] of [
        ["combat", 1],
        ["piles", 0],
    ]) {
        const lines = drawn(runs[moment]).filter(
            ({ act }) => act.arrow === "attack" || act.arrow === "block",
        );
        const attacks = lines.filter(({ act }) => act.arrow === "attack");
        assert.ok(
            attacks.length >= most,
            `${moment}: ${attacks.length} attack lines`,
        );
        const attackers = attacks.map(({ act }) => named(act.from));
        assert.equal(
            new Set(attackers).size,
            attackers.length,
            `${moment}: an attacker's line was begun again: ${attackers}`,
        );
        for (const { key, from, whole, held } of lines) {
            if (from === -Infinity) continue;
            assert.ok(
                whole - from <= 100,
                `${moment}: ${key} took ${whole - from} ms`,
            );
            assert.ok(held.length > 0, `${moment}: ${key} was never seen`);
        }
    }
    const blocks = spansOf(runs.combat, "point").filter(
        ({ act }) => act.arrow === "block",
    );
    assert.ok(blocks.length >= 1, "no block line");
}

/** The line from an ability to its source (UI Conventions, "A line from an ability to its
 * source"): it begins the source line's delay after the ability leaves its source, once the
 * ability has landed on the stack, as Arena's does, comes in whole, and stays for as long as
 * the ability is on the stack. */
function sourceLine(run) {
    const lift = run.edges.find(
        (edge) => edge.verb === "emerge" && edge.edge === "start",
    );
    assert.ok(lift, "no ability lifted");
    const landed = run.edges.find(
        (edge) => edge.verb === "emerge" && edge.edge === "end",
    );
    const line = drawn(run).find(
        ({ act }) =>
            act.arrow === "source" && named(act.from) === `a${lift.act.id}`,
    );
    assert.ok(line, "the lifted ability has no line to its source");
    assert.equal(
        line.from,
        lift.at + SOURCE_LINE_DELAY,
        "the line begins its delay after the ability lifts",
    );
    // Arena's appears whole in one frame once the copy rests on the stack (`2VW6ZzmS8U4
    // 32:15.94`, `0VsYGs88NXA 19:39.67`).
    assert.ok(
        line.from >= landed.at,
        "the line begins before the ability lands",
    );
    assert.ok(
        line.held.some((tick) => tick.at >= line.whole && tick.drawn),
        "no line joins the ability to its source once it has come in",
    );
    const gone = run.edges.find(
        (edge) =>
            edge.verb === "vanish" &&
            edge.act.subject.kind === "ability" &&
            edge.act.subject.id === lift.act.id,
    );
    assert.ok(gone, "the ability never finished");
    assert.ok(line.to <= gone.at, "the line outlived its ability");
    assert.ok(line.to >= landed.at, "the line ended before the ability landed");
}

/** Each of the six marks, played from the script written by hand: on what it marks from the
 * step that begins it to the step that ends it, by one element; its light up by the end of
 * its beginning step; and drawn one way wherever it is, a card, a permanent or a player's
 * life total, and no two alike. What is picked to pay turns a quarter and turns back, and
 * nothing else on the table moves. */
function marks(run) {
    const { written } = run;
    assert.ok(written, "no marks were written");
    const played = spansOf(run, "mark");
    const looks = new Map();
    for (const { key, act, whole, to, held } of played) {
        assert.ok(to < Infinity, `${key} never ended`);
        const up = held.filter((tick) => tick.at >= whole);
        assert.ok(up.length > 0, `${key} was never seen whole`);
        for (const tick of up) {
            assert.equal(tick.shown, 1, `${key} at ${tick.at} ms`);
            assert.ok(tick.look, `${key} has no look`);
            looks.set(
                act.mark,
                (looks.get(act.mark) ?? new Set()).add(tick.look),
            );
        }
    }
    assert.deepEqual(
        [...looks.keys()].sort(),
        [
            "eligible",
            "friendly-target",
            "hostile-target",
            "paying",
            "selected",
            "suggested",
        ],
        "the marks played",
    );
    for (const [mark, drawnAs] of looks) {
        assert.equal(drawnAs.size, 1, `${mark} has ${drawnAs.size} looks`);
    }
    const each = [...looks.values()].map(([look]) => look);
    assert.equal(new Set(each).size, each.length, "two marks look alike");
    // The light of what may be picked comes up on all of it at once, over a moment.
    const eligible = played.filter(({ act }) => act.mark === "eligible");
    assert.equal(eligible.length, written.eligible.length);
    assert.equal(new Set(eligible.map((life) => life.from)).size, 1);
    assert.ok(
        eligible[0].held.some((tick) => tick.shown > 0 && tick.shown < 1),
        "the eligible outline came up in no time",
    );
    // A player's life total is marked as a card is.
    for (const [mark, on] of written.targets) {
        assert.ok(
            played.some(({ act }) => act.mark === mark && named(act.on) === on),
            `${mark} on ${on}`,
        );
    }
    // What pays turns as it would tap, a quarter, while it is marked, and no longer.
    const paying = played.find(({ act }) => act.mark === "paying");
    const quarter = written.turns ? 90 : 0;
    for (const tick of paying.held.filter((t) => t.at >= paying.whole)) {
        assert.equal(tick.turn, quarter, `paying at ${tick.at} ms`);
    }
    assert.deepEqual(
        written.after,
        written.before,
        "the marks moved something",
    );
    assert.deepEqual(run.ticks.at(-1).marks, [], "a mark was left");
}

export const CASES = {
    point(runs) {
        sourceLine(runs.whole);
        for (const moment of ["zones", "combat", "place", "piles"]) {
            drawn(runs[moment]);
        }
        arrowsOnCardsThatMoved(runs.zones, runs.reflows);
        combatLinesThatFlickered(runs);
    },
    mark(runs) {
        marks(runs.whole);
    },
};
