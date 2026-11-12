// How a verb's films are taken (verb-film.mjs): a moment of the verb suite (ui/verbs.json)
// played on the stage under `?stage`, its clock held and stepped in ticks on whole
// milliseconds, with each tick's time and where everything drawn then lay. The stepped capture
// (stepped.mjs) walks a moment the same way for `by motion`'s check, where it shoots each
// step's subject alone for the tracker; a film shoots the table as a player sees it, and
// paces the page by the stage's clock (`walk`'s `paced`) so that a moment in which a choice
// opens plays the same twice. Nothing here reads the page's own time, so nothing a play finds
// depends on the machine's speed.
import { isDeepStrictEqual } from "node:util";
import { basename, join } from "node:path";

/** A tick of stage time in ms: a sixtieth of a second. */
export const TICK = 1000 / 60;
/** How long the stage stands idle before the seat answers, in stage ms (verbs.mjs GAP). */
export const GAP = 250;
/** The most answers the suite's first moment takes (verbs.mjs ANSWERS). */
export const ANSWERS = 6;
/** Ticks taken either side of a step: `by video measure` needs two still frames at each end. */
export const EDGE = 3;

/** In the page: plays the moment as the recording's seat did, holding the stage's clock and
 * advancing it in ticks of `tick`: the seat answers once the stage has stood idle for `gap`,
 * until an ability has come onto the stack and gone and `edge` ticks after, or with `whole`,
 * for all of its `answers`: to the recording's end, which the view the stage then shows tells
 * (`shows`); and with `written`, the marks of the script written by hand after it. Before each tick whose number is in `shoot` it waits on
 * `window.berylShoot(number, time)`. Returns every step the stage told of, with its subject,
 * its stage times from the first tick and its case (src/stage/timing.ts `caseOf`), and each tick's time
 * with what changed since the tick before: the box on the page and the pose as the stage has
 * it of each node drawn otherwise than it was, and the names of those no longer drawn (`reel`
 * reads them whole); and where each player's life total is drawn. Where everything lies at
 * every tick is too much to hand back for a moment that deals two whole libraries.
 *
 * With `paced`, on a page `openStage` opened so, the page's own time is the stage's while the
 * moment plays. The screen waits on the page's clock before it shows a choice (screens/
 * Game.tsx), and the stage lays the table out again for what the screen then shows (the hand
 * tucks under a choice): left to the machine's clock, that lands on another tick each play,
 * and later in a play that stops to shoot than in one that does not. So what the page set
 * before the walk is first let run out; then `setTimeout` counts the stage's milliseconds,
 * `performance.now` reads them, and the page is given turns of its own before each tick is
 * read, as it has between two frames, in which React draws without ever running out of
 * time. A moment in which a choice opens needs it to play the same twice. */
export async function walk({
    tick,
    gap,
    answers,
    edge,
    shoot,
    whole,
    paced,
    written,
}) {
    /** What a step is about, by the name the stage gives it: its card, or the ability; for a
     * look, the top one of the library cards it lifts, or none where it lifts none
     * (stepped.mjs's `subjectOf`); a strike's source, and the card a tally is on. None for a
     * count of life, which is about a player. */
    const subjectOf = (act, view) => {
        if (act.verb === "emerge") return `a${act.id}`;
        if (act.verb === "strike") return String(act.source);
        if (act.verb === "lift") {
            const { zone, side, count } = act.handling;
            const top =
                zone === "library" && count
                    ? view.players[side].library.at(-1)
                    : undefined;
            return top === undefined ? null : String(top);
        }
        if (act.subject?.kind === "ability") return `a${act.subject.id}`;
        if (act.subject?.kind === "object") return String(act.subject.slot);
        if (act.verb === "mark") return named(act.on);
        const object = act.object ?? act.tally?.object;
        return object ? String(object.slot) : null;
    };
    /** What a script's subject is on the page: a node's name, or a player's side. */
    const named = (subject) =>
        subject.kind === "object"
            ? String(subject.slot)
            : subject.kind === "ability"
              ? `a${subject.id}`
              : subject.player;
    /** The two things an arrow, a line or a streak is between, which a film of it is framed
     * on; and for what is about no thing on the table (an announcement over it, a step of the
     * turn beginning), that its film is of the whole table. */
    const endsOf = (act) => {
        if (act.verb === "point") {
            return { ends: [named(act.from), named(act.to)] };
        }
        if (act.verb === "streak") {
            return { ends: [act.from, act.to].filter(Boolean).map(named) };
        }
        return act.verb === "announce" || act.verb === "set"
            ? { table: true }
            : {};
    };
    const stage = window.berylStage;
    const { clock } = stage;
    const live = window.berylTimers;
    const wait = live?.set ?? window.setTimeout.bind(window);
    for (let ms = 0; paced && live.pending.size && ms < 3000; ms += 20) {
        await new Promise((resolve) => wait(resolve, 20));
    }
    clock.hold();
    // What is filmed is the recording's steps and the marks written by hand: no prompt's.
    window.berylPrompts.shut();
    stage.settle();
    while (!Number.isInteger(clock.now)) {
        clock.advance(Math.ceil(clock.now) - clock.now);
    }
    const origin = clock.now;
    const steps = [];
    const open = new Map();
    let lifted = false;
    const unfollow = stage.follow((e) => {
        const at = e.at - origin;
        if (e.edge === "start") {
            open.set(e.step, steps.length);
            const { act } = e.step;
            steps.push({
                act,
                subject: subjectOf(act, e.table.view),
                ...endsOf(act),
                case: window.berylCase(act, e.table.view) ?? null,
                from: at,
                to: at,
            });
        } else if (open.has(e.step)) {
            steps[open.get(e.step)].to = at;
        }
        lifted ||= e.step.act.verb === "emerge";
    });
    const layer = document.querySelector(".card-layer");
    const wanted = new Set(shoot);
    const ticks = [];
    /** Each node drawn at the last tick, by name: its box and pose then. */
    const drawn = new Map();
    const same = (a, b) => Object.keys(a).every((key) => a[key] === b[key]);
    let abilities = false;
    // The page's timers on the stage's clock: each runs in the first turn at or after its
    // time, the earliest first.
    const timers = new Map();
    const now = () => clock.now - origin;
    const page = {
        setTimeout: window.setTimeout,
        clearTimeout: window.clearTimeout,
    };
    if (paced) {
        let set = 0;
        window.setTimeout = (run, delay, ...given) => {
            const at = now() + Math.max(0, Number(delay) || 0);
            timers.set(++set, { at, run, given });
            return -set;
        };
        window.clearTimeout = (id) => {
            if (!timers.delete(-id)) live.clear(id);
        };
        const began = performance.now();
        performance.now = () => began + now();
    }
    const turns = async () => {
        for (let turn = 0; paced && turn < 8; turn++) {
            for (;;) {
                const [next] = [...timers]
                    .filter(([, timer]) => timer.at <= now())
                    .sort(([a, x], [b, y]) => x.at - y.at || a - b);
                if (!next) break;
                timers.delete(next[0]);
                next[1].run(...next[1].given);
            }
            await new Promise((resolve) => {
                const channel = new MessageChannel();
                channel.port1.onmessage = () => {
                    channel.port1.close();
                    resolve();
                };
                channel.port2.postMessage(null);
            });
        }
    };
    const sample = async () => {
        await turns();
        const table = layer.getBoundingClientRect();
        const by = table.width / layer.offsetWidth;
        const boxes = {};
        const poses = {};
        const seen = new Set();
        abilities = false;
        for (const name of stage.shown.resting.keys()) {
            abilities ||= typeof name === "string";
            const pose = stage.pose(name);
            if (!pose || pose.opacity === 0) continue;
            // As the node is drawn (src/stage/pose.ts transformOf, as stepped.mjs reads it): a
            // card's frame, 63 by 88, leaned and turned about its top-left corner, which lies
            // at the pose's place; the box around it, widened to the square it turns within
            // while its card turns about its own middle (a permanent tapping).
            const [w, h] = [pose.width, (pose.width * 88) / 63];
            const lean = Math.tan((pose.skew * Math.PI) / 180);
            const [cos, sin] = [Math.cos, Math.sin].map((f) =>
                f((pose.rotation * Math.PI) / 180),
            );
            const reach = pose.turn ? (h - w) / 2 : 0;
            const corners = [
                [-reach, 0],
                [w + reach, 0],
                [-reach, h],
                [w + reach, h],
            ].map(([x, y]) => [x + y * lean, y]);
            const xs = corners.map(([x, y]) => pose.x + x * cos - y * sin);
            const ys = corners.map(([x, y]) => pose.y + x * sin + y * cos);
            const box = [
                table.left + Math.min(...xs) * by,
                table.top + Math.min(...ys) * by,
                table.left + Math.max(...xs) * by,
                table.top + Math.max(...ys) * by,
            ];
            const key = String(name);
            seen.add(key);
            const was = drawn.get(key);
            if (was && same(was.box, box) && same(was.pose, pose)) continue;
            drawn.set(key, { box, pose: { ...pose } });
            boxes[key] = box;
            poses[key] = pose;
        }
        const gone = [...drawn.keys()].filter((key) => !seen.has(key));
        for (const key of gone) drawn.delete(key);
        const at = clock.now - origin;
        if (wanted.has(ticks.length)) await window.berylShoot(ticks.length, at);
        ticks.push({ at, boxes, poses, gone });
    };
    // Still ticks before the first answer, which begins a step at once.
    for (let still = 0; still <= edge; still++) {
        if (still)
            clock.advance(
                Math.round(still * tick) - Math.round((still - 1) * tick),
            );
        await sample();
    }
    let after = 0;
    let finished = false;
    for (let answer = 0; answer < answers && after < edge; answer++) {
        await window.berylFixture.invoke("choose", { request: {} });
        // The transport hands the views on in a microtask.
        await new Promise((resolve) => wait(resolve));
        let idle = 0;
        while (after < edge && idle < gap) {
            // On the whole milliseconds nearest each tick's time, so that two plays read the
            // same to the bit wherever the page's clock stood when it was held.
            const took =
                Math.round(ticks.length * tick) -
                Math.round((ticks.length - 1) * tick);
            clock.advance(took);
            await sample();
            // Over once an ability has come onto the stack and gone, as verbs.mjs's walk is.
            finished ||= !whole && lifted && !abilities;
            if (finished) after += 1;
            // No step plays while what a card left room for closes up after its step.
            idle = stage.step || stage.shown.busy ? 0 : idle + took;
        }
    }
    // The marks, from the script written by hand (written-marks.mjs, which the caller gave
    // the page), each beat a script of its own over the table the moment leaves, once what
    // is left of it has played out.
    if (written) {
        // What plays out unseen meanwhile is no step of this walk's.
        const seen = steps.length;
        stage.settle();
        steps.length = seen;
        const view = stage.shown.view;
        for (const beat of window.berylWrittenMarks(view).beats) {
            stage.play(view, { beats: [beat] });
            for (let idle = 0; idle < gap;) {
                const took =
                    Math.round(ticks.length * tick) -
                    Math.round((ticks.length - 1) * tick);
                clock.advance(took);
                await sample();
                idle = stage.step ? 0 : idle + took;
            }
        }
    }
    unfollow();
    if (paced) {
        Object.assign(window, page);
        delete performance.now;
    }
    const shown = stage.shown.view;
    const orbs = Object.fromEntries(
        [...document.querySelectorAll("[data-life]")].map((orb) => {
            const { left, top, right, bottom } = orb.getBoundingClientRect();
            return [orb.dataset.life, [left, top, right, bottom]];
        }),
    );
    return {
        steps,
        ticks,
        orbs,
        done: whole ? !stage.shown.busy : after >= edge,
        shows: shown && [shown.turn.number, shown.turn.step, shown.status],
    };
}

/** A page of `size` on the stage `query` asks for, whose fixtures are those recorded into
 * `fixtures`, at `fixture`'s first view, drawn at `scale` device pixels a pixel: its context,
 * the page and the errors it logs. With `paced` the page keeps count of the timers it has
 * set (`window.berylTimers`), for `walk` to pace it. */
export async function openStage(
    browser,
    { base, fixtures, size, query, fixture, scale = 1, paced = false },
) {
    const context = await browser.newContext({
        viewport: size,
        deviceScaleFactor: scale,
    });
    if (paced) {
        await context.addInitScript(() => {
            const set = window.setTimeout.bind(window);
            const clear = window.clearTimeout.bind(window);
            const pending = new Set();
            window.berylTimers = { pending, set, clear };
            window.setTimeout = (run, delay, ...given) => {
                const id = set(() => {
                    pending.delete(id);
                    run(...given);
                }, delay);
                pending.add(id);
                return id;
            };
            window.clearTimeout = (id) => {
                pending.delete(id);
                clear(id);
            };
        });
    }
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
    const errors = [];
    page.on("pageerror", (e) => errors.push(e.message));
    await page.goto(
        new URL(`/?${query}&fixture=${fixture}&at=1#/game`, base).href,
    );
    await page.waitForFunction(
        () => window.berylStage && document.querySelector(".stage-node"),
    );
    await page.evaluate(() => document.fonts.ready.then(() => {}));
    return { context, page, errors };
}

/** Where two plays of a moment first differ, in a few words. */
export function differing(first, second) {
    const told = ({ act, from, to }) => `${act.verb} ${from} to ${to}`;
    const n = first.steps.findIndex(
        (step, k) => !isDeepStrictEqual(step, second.steps[k]),
    );
    if (n >= 0 || first.steps.length !== second.steps.length) {
        const at = n >= 0 ? n : first.steps.length;
        const [a, b] = [first.steps[at], second.steps[at]];
        return `step ${at} was ${a ? told(a) : "none"}, then ${b ? told(b) : "none"}`;
    }
    const t = first.ticks.findIndex(
        (tick, k) => !isDeepStrictEqual(tick, second.ticks[k]),
    );
    if (t < 0 || !second.ticks[t]) {
        return `${first.ticks.length} ticks, then ${second.ticks.length}`;
    }
    const [a, b] = [reel(first.ticks), reel(second.ticks)];
    const named = [first.ticks[t], second.ticks[t]].flatMap((tick) => [
        ...Object.keys(tick.poses),
        ...tick.gone,
    ]);
    const names = [...new Set(named)].filter(
        (name) => !isDeepStrictEqual(a.pose(name, t), b.pose(name, t)),
    );
    return `at tick ${t}, ${first.ticks[t].at} ms, ${names.join(", ")} was ${JSON.stringify(a.pose(names[0], t))}, then ${JSON.stringify(b.pose(names[0], t))}`;
}

/** A walk's `ticks`, each of which tells only what changed since the one before, read whole:
 * `box(name, t)` and `pose(name, t)` are the box and pose of the node `name` at tick `t`, or
 * null while it is not drawn; `changes(name)`, the ticks at which either changes, in order;
 * `names`, every node drawn at some tick. */
export function reel(ticks) {
    const tracks = new Map();
    const tell = (name, t, box, pose) => {
        if (!tracks.has(name)) tracks.set(name, { at: [], box: [], pose: [] });
        const track = tracks.get(name);
        track.at.push(t);
        track.box.push(box);
        track.pose.push(pose);
    };
    ticks.forEach((tick, t) => {
        for (const [name, box] of Object.entries(tick.boxes)) {
            tell(name, t, box, tick.poses[name]);
        }
        for (const name of tick.gone) tell(name, t, null, null);
    });
    /** What `track` told of `kind` at its last change at or before tick `t`, or null. */
    const read = (kind) => (name, t) => {
        const track = tracks.get(name);
        if (!track) return null;
        let [low, high] = [0, track.at.length];
        while (low < high) {
            const middle = (low + high) >> 1;
            if (track.at[middle] <= t) low = middle + 1;
            else high = middle;
        }
        return low ? track[kind][low - 1] : null;
    };
    return {
        names: [...tracks.keys()],
        box: read("box"),
        pose: read("pose"),
        changes: (name) => tracks.get(name)?.at ?? [],
    };
}
