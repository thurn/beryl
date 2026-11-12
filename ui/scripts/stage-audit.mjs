// The motion and duplicate audits of the stage (plan K25): what `by specs --motion`, the
// driver's `probe.audit` and `spec.audit` judge by (`window.berylStage`). The script the
// session sent with each view says what was meant, step by step; the stage says when it began
// and ended each step (`stage.follow`); and the page is measured as a player sees it at every
// tick of the stage's own clock, which this holds and steps. Nothing here waits on the page's
// time: a moment is over when the last view the page heard asks its viewer something or ends
// the game, the stage has played it out, and no answer of the page's is still on its way.
//
// `watching(page)` begins an audit and returns how to end it. Each step of each script gets a
// finding, its verdict read from the script:
//   ok        the stage played the step and the page showed it, for as long as its verb takes
//   still     the stage played no such step, or nothing on the page shows it
//   instant   what the step moves jumped, in a step whose verb gives it time to travel. A verb
//             that only appears or vanishes takes what the design language gives it, which
//             may be no time at all (UI Conventions, "Tokens", "A finished ability")
//   unseen    what the step is about was off screen, covered or transparent the while
//   crowded   more than CROWDED different moves began at once in one beat
//   duplicate two painted instances of one object shared a tick, but for what the design
//             language shows twice (data/ui-conventions.json, `twice`): the cards of a prompt
//             of a shape an entry names, and the face a verb holds up beside its card
//   jump      an arrow or line whole on the table changed shape while both its ends stood
//             still, read from the stage's model (`stage.lines`). One redrawn as an end of it
//             moves is no finding: a line is "drawn every
//             moment from where its two ends are" (UI Conventions, Verbs, point), and that it
//             ends before a target leaves is the script's own check (script/check.rs)
// A finding about a tap or untap is `minor`. What the screen around the table
// animates on the page's own time (a button sliding in) is not judged here: it is no step's.
import { readFile } from "node:fs/promises";
import { duplicates, record, CROWDED, MIN_MS, MIN_PX, SEEN } from "./audit.mjs";
import { LONGEST, rest } from "./moments.mjs";

/** The stage time between two measurements of the page: two frames'. Measuring the page is
 * nearly all an audit costs, and what a tick this long could miss is shorter than anything a
 * verdict turns on (MIN_MS). */
export const TICK = 1000 / 30;

/** What the design language shows twice (`twice`): the prompt `shapes` whose cards are on
 * screen again while the prompt is open, from its entries, and the `verbs` whose step holds up
 * a second face of its card. */
export const TWICE = await readFile(
    new URL("../../data/ui-conventions.json", import.meta.url),
    "utf8",
).then((text) => {
    const { entries, verbs } = JSON.parse(text);
    return {
        shapes: new Set(entries.flatMap((e) => (e.twice ? [e.twice] : []))),
        verbs: new Set(verbs.flatMap((v) => (v.twice ? [v.name] : []))),
    };
});

/** In the page, after `record` with `stepped`: holds the stage's clock and starts keeping what
 * the stage is told to show and what it says it played, and measures the page as it stands. */
export function watch() {
    const stage = window.berylStage;
    const { clock } = stage;
    clock.hold();
    const named = (on) => (on === "you" || on === "opponent" ? null : on);
    const watching = {
        since: clock.now,
        views: [],
        edges: [],
        /** Measures the page now, at the stage's time, with what the model says besides. */
        sample() {
            window.__motion.frame(clock.now);
            const frame = window.__motion.frames.at(-1);
            const { view, busy } = stage.shown;
            const decision =
                !busy && view?.status.kind === "deciding"
                    ? view.status.decision.prompt
                    : undefined;
            const prompt = window.berylPrompts?.asked().prompt ?? decision;
            frame.opening = document.querySelector(".opening") !== null;
            frame.phone = document.querySelector(".phone-board") !== null;
            const names = new Map(
                (view?.objects ?? []).map((o) => [o.slot, o.name]),
            );
            frame.prompt = prompt
                ? {
                      shape: prompt.shape,
                      about: prompt.about,
                      names: prompt.about
                          .map((slot) => names.get(slot))
                          .filter(Boolean),
                  }
                : null;
            // Each arrow and line whole on the table, with where the model has its ends.
            frame.lines = stage
                .lines()
                .filter((line) => line.shape && !line.hidden)
                .map((line) => ({
                    key: line.key,
                    arrow: line.arrow,
                    whole: line.shown >= 1,
                    d: line.shape.d,
                    ends: [line.from, line.to]
                        .map((end) => {
                            const pose =
                                named(end) === null ? null : stage.pose(end);
                            return pose
                                ? [pose.x, pose.y, pose.width, pose.rotation]
                                      .map((n) => Math.round(n * 10) / 10)
                                      .join(",")
                                : String(end);
                        })
                        .join("|"),
                }));
        },
    };
    let last = stage.shown.view;
    watching.unsubscribe = stage.subscribe(() => {
        const { view } = stage.shown;
        if (!view || view === last) return;
        last = view;
        watching.views.push({ view, at: clock.now });
    });
    watching.unfollow = stage.follow((edge) =>
        watching.edges.push({
            edge: edge.edge,
            at: edge.at,
            from: edge.span.from,
            to: edge.span.to,
            step: edge.step,
        }),
    );
    window.__stageAudit = watching;
    watching.sample();
    // Whatever is audited may be a click, which lands as a player's would.
    window.__motion.unhit();
}

/** In the page, once what is audited has been set going: waits until the page has heard all
 * of it, then plays it out on the stage's clock in ticks of `tick` ms, measuring the page at
 * each, and gives the clock back to the page's frames. Returns the measurements, each view the
 * stage showed with its script's steps, and each step's edges. `unheard` says the page was
 * still owed a view after `longest` ms of its own time. */
export async function walk({ tick, longest }) {
    const stage = window.berylStage;
    const { clock } = stage;
    const watching = window.__stageAudit;
    const started = performance.now();
    const late = () => performance.now() - started > longest;
    // A turn of the page's event loop, for what it has heard to reach the stage and for the
    // screen to draw what the stage shows: a message, which no timer's clamp slows.
    const channel = new MessageChannel();
    const turn = () =>
        new Promise((resolve) => {
            channel.port1.onmessage = () => resolve();
            channel.port2.postMessage(0);
        });
    const pause = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
    // How far the page is from rest (moments.mjs `rest`, put on the page by `watching`).
    const rest = window.__stageAudit.rest;
    let unheard = false;
    // A replayed recording hands its views on in a microtask of the answer that asks for them.
    await pause(0);
    while (rest().owed && !(unheard = late())) await pause(10);
    for (;;) {
        if (stage.shown.busy) {
            clock.advance(tick);
            watching.sample();
            await turn();
            continue;
        }
        const { owed, shown } = rest();
        if (shown && !owed) {
            // What the screen draws once the view shows (a prompt opening over the table), and
            // what the stage then plays for it (the prompt's marks).
            for (let n = 0; n < 3; n++) await turn();
            if (stage.shown.busy) continue;
            break;
        }
        if ((unheard = late())) break;
        await pause(10);
    }
    watching.sample();
    window.__motion.unhit();
    watching.unsubscribe();
    watching.unfollow();
    clock.release();
    const steps = new Map();
    const views = watching.views.map(({ view, at }, n) => {
        const script = view.script;
        const flat = (script?.beats ?? []).flatMap((beat, b) =>
            beat.map((step) => ({ step, beat: b })),
        );
        flat.forEach(({ step }, index) => steps.set(step, [n, index]));
        return {
            at,
            turn: `turn ${view.turn.number} ${view.turn.active} ${view.turn.step}`,
            // A view with no script, or one that resets, shows at once: nothing of it plays.
            plays: script !== undefined && !script.reset,
            names: Object.fromEntries(
                view.objects.map((o) => [o.slot, o.name]),
            ),
            steps: flat.map(({ step, beat }) => ({ act: step.act, beat })),
        };
    });
    return {
        frames: window.__motion.frames,
        since: watching.since,
        views,
        // The steps of the session's scripts: a prompt's own marks are no view's.
        edges: watching.edges.flatMap(({ step, ...edge }) => {
            const found = steps.get(step);
            return found ? [{ ...edge, view: found[0], index: found[1] }] : [];
        }),
        unheard,
    };
}

/** What a step's act moves or is about on the table, as the stage names its node: a card's
 * slot, or `a` and an ability's number (src/stage/tracks.ts `moverOf`). */
export function subjectOf(act) {
    if (act.verb === "strike") return String(act.source);
    if (act.verb === "emerge") return `a${act.id}`;
    if (act.verb === "vanish") {
        if (act.subject.kind === "player") return undefined;
        return act.subject.kind === "object"
            ? String(act.subject.slot)
            : `a${act.subject.id}`;
    }
    return act.object ? String(act.object.slot) : undefined;
}

/** A step in a few words, objects by name. */
export function describe(act, names) {
    const name = (slot) => names[slot] ?? `#${slot}`;
    const of = (subject) =>
        subject === undefined
            ? "nothing"
            : subject.kind === "object"
              ? name(subject.slot)
              : subject.kind === "ability"
                ? `ability ${subject.id}`
                : subject.player;
    const card = (object) => object.name ?? name(object.slot);
    switch (act.verb) {
        case "fly":
        case "appear":
            return `${card(act.object)} ${act.from}->${act.object.zone} (${act.why})`;
        case "vanish":
            return act.object
                ? `${card(act.object)} ${act.from}->${act.object.zone} (${act.why})`
                : `${of(act.subject)} leaves the ${act.from} (${act.why})`;
        case "emerge":
            return `ability of ${act.item.name ?? name(act.item.source)}`;
        case "flip":
            return `${card(act.object)} turns over`;
        case "tap":
            return `${card(act.object)} ${act.object.tapped ? "taps" : "untaps"}`;
        case "strike":
            return `${act.amount} damage from ${name(act.source)} to ${of(act.to)}`;
        case "streak":
            return act.stream.kind === "mana"
                ? `${act.amount} ${act.stream.color} mana from ${of(act.from)} to ${of(act.to)}`
                : `${act.amount} damage from ${of(act.from)} to ${of(act.to)}`;
        case "count":
            return act.tally.kind === "life"
                ? `${act.tally.player} life ${act.tally.life}`
                : act.tally.kind === "pool"
                  ? `${act.tally.player} mana pool`
                  : `${card(act.tally.object)} ${act.tally.kind}`;
        case "set":
            return act.fact.kind === "turn"
                ? `${act.fact.active} ${act.fact.step}`
                : act.fact.kind === "library"
                  ? `${act.fact.player} library in order`
                  : act.fact.kind === "object"
                    ? `${card(act.fact.object)} changes`
                    : `${of(act.fact.subject)} changes`;
        case "showcase":
            return `${card(act.object)} shown`;
        case "dwell":
            return `wait on ${of(act.subject)}`;
        case "announce":
            return `${act.notice.kind} notice`;
        case "point":
            return `${act.arrow} line ${of(act.from)} to ${of(act.to)} ${act.span}s`;
        case "mark":
            return `${act.mark} mark on ${of(act.on)} ${act.span}s`;
        case "lift":
            return `cards held up ${act.span}`;
        default:
            return act.verb;
    }
}

/** Whether a measured instance is one a player sees in `frame`: painted, on screen, and on
 * top; or under an announcement, which passes over the cards in flight while play goes on
 * (UI Conventions, Verbs, announce); or under its like in a pile, a card of its name lying over
 * it in its zone, which shows it as the pile does (UI Conventions, "Lands unstacked until
 * seven"). While the opening's screen is up (src/screens/Opening.tsx) it is that screen that
 * shows the hands dealt, the mulligans and the cards put back, over a table that plays them
 * beneath it or holds its hand out of sight: there every instance counts as shown. */
function seen(card, frame) {
    if (card === undefined) return false;
    if (frame.opening) return true;
    if (card.o <= 0.3 || !card.on) return false;
    if (card.top || card.cover === ".stage-notice") return true;
    const over = frame.nodes.get(card.cover);
    return (
        over !== undefined && over.name === card.name && over.zone === card.zone
    );
}

/** What kept a measured instance from being seen in `frame`, for a finding's words. */
const hiddenBy = (card, frame) =>
    !card.on
        ? "off screen"
        : card.o <= 0.3
          ? "transparent"
          : card.cover === undefined
            ? "covered"
            : `under ${frame.nodes.get(card.cover)?.name ?? card.cover}`;

/** What mostly kept a step's subject from being seen along `path`, for a finding's words. */
function hidden(path) {
    const why = path
        .filter(({ card, frame }) => !seen(card, frame))
        .map(({ card, frame }) => hiddenBy(card, frame));
    const most = [...new Set(why)].sort(
        (a, b) =>
            why.filter((w) => w === b).length -
            why.filter((w) => w === a).length,
    )[0];
    return most ? `, mostly ${most}` : "";
}

/** How far apart two measurements of a node are: its middle, or its size, which a turn or a
 * change of zone changes. */
const apart = (a, b) =>
    Math.max(
        Math.hypot(a.x - b.x, a.y - b.y),
        Math.abs(a.w - b.w),
        Math.abs(a.h - b.h),
    );

/** How the page showed one step of a view's script, given its edges: `verdict` and `why`.
 * `taken(name, from, to)` says whether another step about `name` began in that time. */
function judgeStep(act, start, end, frames, taken) {
    if (!start) {
        return { verdict: "still", why: "the stage played no such step" };
    }
    const { from, to } = start;
    const took = Math.round(to - from);
    const name = subjectOf(act);
    const played = { verdict: "ok", why: `played over ${took} ms` };
    if (!end && to <= frames.at(-1).t) {
        return { verdict: "still", why: "the stage never ended the step" };
    }
    // What moves no card, or lays its cards out for a prompt: the verb suite holds what each
    // draws to the model (verbs.mjs); here, that the stage played it.
    if (
        name === undefined ||
        ["streak", "count", "set", "showcase", "dwell", "lift"].includes(
            act.verb,
        )
    ) {
        return played;
    }
    // The phone draws no exile pile yet: an exiled card rests unseen under its owner's
    // graveyard (UI Conventions, "The phone"), so a card's way into or out of exile there has
    // no place of its own to be seen at. One a permanent holds is tucked under it, as on the
    // desktop, and is judged as any.
    const exiled = (object) =>
        object?.zone === "exile" &&
        object.exiledBy == null &&
        object.exiledWith == null;
    if (
        frames.at(-1).phone &&
        (exiled(act.object) ||
            (act.from === "exile" && ["fly", "vanish"].includes(act.verb)))
    ) {
        return played;
    }
    // A step the stage gives no time is a cut by design: nothing of it travels.
    if (took < TICK && act.verb !== "vanish" && act.verb !== "appear") {
        return { verdict: "ok", why: "played in no time" };
    }
    // The frame before the step began, each one in it, and the first with it over.
    const first = frames.findIndex((f) => f.t >= from);
    const before = frames[first - 1]?.nodes.get(name);
    const during = frames.filter((f) => f.t >= from && f.t <= to);
    const over = frames.find((f) => f.t >= to);
    const after = over?.nodes.get(name);
    const path = [...new Set([frames[first - 1], ...during, over])]
        .filter(Boolean)
        .map((f) => ({ t: f.t, card: f.nodes.get(name), frame: f }))
        .filter((at) => at.card);
    if (act.verb === "vanish") {
        if (!before) {
            return { verdict: "still", why: "nothing of it was there to go" };
        }
        if (
            !seen(before, frames[first - 1]) &&
            !during.some((f) => seen(f.nodes.get(name), f))
        ) {
            return { verdict: "unseen", why: "it went where no player saw it" };
        }
        // A card is at its new place as it is gone, unless its next step has it already (a
        // card exiled and returned at once); an ability, or a token that is no more, has no
        // node.
        const gone = act.object
            ? (after && after.zone === act.object.zone) ||
              taken(name, to, over?.t ?? to)
            : !after || after.o === 0;
        return gone
            ? { verdict: "ok", why: `gone over ${took} ms` }
            : { verdict: "still", why: "it is still where it was" };
    }
    if (path.length === 0) {
        return { verdict: "still", why: "no node on the page shows it" };
    }
    // An ability whose source is no more has nothing to lift out of: it appears on the stack
    // (UI Conventions, "An ability with no source on the battlefield").
    const source = act.verb === "emerge" && act.item.source;
    const sourceless =
        act.verb === "emerge" &&
        (source == null || !frames[first - 1]?.nodes.has(String(source)));
    if (act.verb === "appear" || sourceless) {
        if (!after) {
            return { verdict: "still", why: "nothing came to be at its place" };
        }
        // Read once it is whole: for as long as a move must take to be seen.
        const whole = frames.filter((f) => f.t >= to && f.t <= to + MIN_MS);
        if (whole.some((f) => seen(f.nodes.get(name), f))) {
            return { verdict: "ok", why: `appeared over ${took} ms` };
        }
        const last = whole.findLast((f) => f.nodes.has(name));
        return {
            verdict: "unseen",
            why: `it came to be ${last ? hiddenBy(last.nodes.get(name), last) : "nowhere on the page"}`,
        };
    }
    const visible = path.filter(({ card, frame }) => seen(card, frame)).length;
    const unseen = visible < path.length * SEEN && {
        verdict: "unseen",
        why: `visible in ${visible} of ${path.length} frames${hidden(path)}`,
    };
    if (act.verb === "flip") return unseen || played;
    // How far it went from where it was, and over how long it kept changing.
    const origin = path[0].card;
    const reach = Math.max(...path.map(({ card }) => apart(card, origin)));
    const changes = path.flatMap(({ t, card }, n) =>
        n > 0 && apart(card, path[n - 1].card) >= 1 ? [t] : [],
    );
    const moving =
        changes.length === 0
            ? 0
            : changes.at(-1) - path.findLast((at) => at.t < changes[0]).t;
    if (reach < MIN_PX) {
        if (act.verb === "tap") {
            return { verdict: "still", why: "it did not turn" };
        }
        // A card put elsewhere in the pile it lies in has nowhere to travel.
        return act.verb === "fly" && act.from === act.object.zone
            ? { verdict: "ok", why: "it stays where it lies" }
            : { verdict: "still", why: "it did not move" };
    }
    if (unseen) return unseen;
    // A jump, where the verb gives it time: it changed for under half its step, or under what
    // counts as animated, by more than a tick. An eased move's last ticks are within a pixel.
    if (took >= MIN_MS && moving < Math.min(MIN_MS, took / 2) - TICK) {
        return {
            verdict: "instant",
            why: `moved ${Math.round(reach)}px in ${Math.round(moving)} ms of its ${took}`,
        };
    }
    return {
        verdict: "ok",
        why: `moved ${Math.round(reach)}px over ${Math.round(moving)} ms`,
    };
}

/** The verbs that move a card or an ability from one place to another. */
const MOVES = new Set(["fly", "appear", "vanish", "emerge", "flip", "strike"]);

/** The beats of a view in which more than CROWDED different moves began within MIN_MS of one
 * another: cards making the same move together read as one. */
function crowds(view, starts) {
    const found = [];
    const beats = new Map();
    view.steps.forEach((step, index) => {
        const start = starts.get(index);
        if (!start || !MOVES.has(step.act.verb)) return;
        const beat = beats.get(step.beat) ?? [];
        beat.push({ act: step.act, at: start.from });
        beats.set(step.beat, beat);
    });
    for (const beat of beats.values()) {
        beat.sort((a, b) => a.at - b.at);
        for (let n = 0; n < beat.length;) {
            const group = beat.filter(
                (s, i) => i >= n && s.at - beat[n].at < MIN_MS,
            );
            const moves = new Map(
                group.map(({ act }) => [
                    `${act.verb}:${act.from}>${act.object?.zone}:${act.why}`,
                    act,
                ]),
            );
            if (moves.size > CROWDED) {
                found.push({
                    what: [...moves.values()]
                        .map((act) => describe(act, view.names))
                        .join("; "),
                    verdict: "crowded",
                    why: `${moves.size} moves began within ${MIN_MS} ms`,
                    start: beat[n].at,
                });
            }
            n += group.length;
        }
    }
    return found;
}

/** Whether two paths differ by a move the eye would catch: some number of theirs by MIN_PX,
 * or how many they have. */
function reshaped(was, now) {
    const numbers = (d) => (d.match(/-?\d+(\.\d+)?/g) ?? []).map(Number);
    const [a, b] = [numbers(was), numbers(now)];
    return (
        a.length !== b.length || a.some((n, i) => Math.abs(n - b[i]) >= MIN_PX)
    );
}

/** Each arrow or line that changed shape between two ticks while whole in both and with both
 * its ends where they were: a `jump`. */
function lineFindings(frames) {
    const runs = new Map();
    for (let i = 1; i < frames.length; i++) {
        const before = new Map(
            (frames[i - 1].lines ?? []).map((line) => [line.key, line]),
        );
        for (const now of frames[i].lines ?? []) {
            const was = before.get(now.key);
            if (!was || !was.whole || !now.whole) continue;
            if (was.ends !== now.ends || !reshaped(was.d, now.d)) continue;
            const run = runs.get(now.key) ?? {
                line: `${now.arrow} line ${now.key}`,
                start: frames[i].t,
                count: 0,
            };
            run.count++;
            runs.set(now.key, run);
        }
    }
    return [...runs.values()].map(({ line, start, count }) => ({
        kind: "arrow",
        what: line,
        verdict: "jump",
        why: `changed shape ${count} time${count > 1 ? "s" : ""} with its ends still`,
        start,
    }));
}

/** Each finding of one seat's run under the stage (`walk`'s return): per view, each step of
 * its script and any crowd, then each duplicate and each arrow that jumped. `twice` is what
 * may show again (`TWICE`). Each finding has `at`, stage ms from the run's
 * start to when its step began. */
export function judge(
    { frames: measured, views, edges, since },
    twice = TWICE,
) {
    const frames = measured.map((frame) => ({
        ...frame,
        nodes: new Map(
            frame.instances
                .filter((card) => card.node !== undefined)
                .map((card) => [card.node, card]),
        ),
    }));
    const findings = [];
    // When each step about a node began, to tell a step whose subject the next has taken up.
    const began = edges.flatMap((edge) => {
        const name =
            edge.edge === "start" &&
            subjectOf(views[edge.view].steps[edge.index].act);
        return name ? [{ name, edge }] : [];
    });
    views.forEach((view, n) => {
        if (!view.plays) return;
        const own = edges.filter((edge) => edge.view === n);
        const edge = (index, which) =>
            own.find((e) => e.index === index && e.edge === which);
        const starts = new Map();
        view.steps.forEach(({ act }, index) => {
            const start = edge(index, "start");
            if (start) starts.set(index, start);
            findings.push({
                view: view.turn,
                kind: act.verb,
                minor: act.verb === "tap",
                what: describe(act, view.names),
                ...judgeStep(
                    act,
                    start,
                    edge(index, "end"),
                    frames,
                    (name, from, to) =>
                        began.some(
                            (other) =>
                                other.name === name &&
                                other.edge !== start &&
                                other.edge.from >= from &&
                                other.edge.from <= to,
                        ),
                ),
                start: start?.from,
            });
        });
        for (const crowd of crowds(view, starts)) {
            findings.push({ view: view.turn, ...crowd });
        }
    });
    // A face a verb holds up is a node of the stage's own, named for no slot, that shows a
    // card (src/stage/layout.ts `Name`): it outlasts the script that held it up, so it is
    // known by what it is, not by this run's steps.
    const held = (card) =>
        twice.verbs.size > 0 &&
        card.node !== undefined &&
        card.slot == null &&
        card.name != null;
    const allows = (frame, card) =>
        held(card) ||
        (frame.prompt != null &&
            twice.shapes.has(frame.prompt.shape) &&
            (frame.prompt.about.includes(Number(card.slot)) ||
                (card.slot == null && frame.prompt.names.includes(card.name))));
    for (const found of [
        ...duplicates(frames, allows),
        ...lineFindings(frames),
    ]) {
        const view = views.find((v) => v.at >= found.start) ?? views.at(-1);
        findings.push({ view: view?.turn ?? "", ...found });
    }
    return findings.map(({ start, ...finding }) => ({
        ...finding,
        ...(start === undefined ? {} : { at: Math.round(start - since) }),
    }));
}

/** Begins an audit of `page`'s motion and returns how to end it, once whatever is audited has
 * been set going (a moment played, a click): that resolves to its findings. A page still owed
 * a view after the longest wait throws. */
export async function watching(page) {
    await page.evaluate(record);
    await page.evaluate(watch);
    await page.evaluate(`window.__stageAudit.rest = ${rest}`);
    return async () => {
        const run = await page.evaluate(walk, { tick: TICK, longest: LONGEST });
        if (run.unheard) {
            throw new Error(
                `the page was still owed a view after ${LONGEST} ms`,
            );
        }
        return judge(run);
    };
}
