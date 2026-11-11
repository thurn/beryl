// What the stepped capture (stepped.mjs) and the verbs' films (verb-film.mjs) share: a moment
// of the verb suite (ui/verbs.json) played on the stage under `?stage`, its clock held and
// stepped in ticks on whole milliseconds, with each tick's time and where everything drawn
// then lay. Nothing here reads the page's own time, so nothing a play finds depends on the
// machine's speed, and two plays of one moment read the same to the bit.
import { isDeepStrictEqual } from "node:util";
import { readFile } from "node:fs/promises";
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
 * (`shows`). Before each tick whose number is in `shoot` it waits on
 * `window.berylShoot(number, time)`. Returns every step the stage told of, with its stage
 * times from the first tick and its case (src/stage/timing.ts `caseOf`), and each tick's time
 * with the box on the page of everything drawn then (a card's frame is 63 by 88, taken as it
 * lies upright) and its pose as the stage has it; and where each player's life total is
 * drawn.
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
export async function walk({ tick, gap, answers, edge, shoot, whole, paced }) {
    const stage = window.berylStage;
    const { clock } = stage;
    const live = window.berylTimers;
    const wait = live?.set ?? window.setTimeout.bind(window);
    for (let ms = 0; paced && live.pending.size && ms < 3000; ms += 20) {
        await new Promise((resolve) => wait(resolve, 20));
    }
    clock.hold();
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
            steps.push({
                act: e.step.act,
                case: window.berylCase(e.step.act, e.table.view) ?? null,
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
        abilities = false;
        for (const name of stage.shown.resting.keys()) {
            abilities ||= typeof name === "string";
            const pose = stage.pose(name);
            if (!pose || pose.opacity === 0) continue;
            const x = table.left + pose.x * by;
            const y = table.top + pose.y * by;
            const width = pose.width * by;
            boxes[name] = [x, y, x + width, y + (width * 88) / 63];
            poses[name] = pose;
        }
        const at = clock.now - origin;
        if (wanted.has(ticks.length)) await window.berylShoot(ticks.length, at);
        ticks.push({ at, boxes, poses });
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
            idle = stage.step ? 0 : idle + took;
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

/** What a step is about, by the name the stage gives it: its card, or the ability. */
export function subjectOf(act) {
    if (act.verb === "emerge") return `a${act.id}`;
    if (act.verb === "strike") return String(act.source);
    if (act.subject?.kind === "ability") return `a${act.subject.id}`;
    if (act.subject?.kind === "object") return String(act.subject.slot);
    const object = act.object ?? act.tally?.object;
    return object ? String(object.slot) : null;
}

/** The views a recorded fixture of `fixtures` shows, in order. */
async function views(fixtures, fixture) {
    const { messages } = JSON.parse(
        await readFile(join(fixtures, `${fixture}.json`), "utf8"),
    );
    return messages
        .filter((m) => m.kind === "event")
        .map((m) => m.payload.view);
}

/** What a recorded fixture ends on, as `walk` tells what the stage shows (`shows`), and how
 * many answers of its seat lead there: one for each view that waits on one (verbs.mjs
 * `ending`). */
export async function ending(fixtures, fixture) {
    const shown = await views(fixtures, fixture);
    const { turn, status } = shown.at(-1);
    return {
        shows: [turn.number, turn.step, status],
        answers:
            1 + shown.filter((view) => view.status.kind === "deciding").length,
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
    const [a, b] = [first.ticks[t].poses, second.ticks[t].poses];
    const names = Object.keys({ ...a, ...b }).filter(
        (name) => !isDeepStrictEqual(a[name], b[name]),
    );
    return `at tick ${t}, ${first.ticks[t].at} ms, ${names.join(", ")} was ${JSON.stringify(a[names[0]])}, then ${JSON.stringify(b[names[0]])}`;
}
