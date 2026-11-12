// The stepped capture (plan K27, Motion oracle): the verb suite's moments (ui/verbs.json) played
// on the stage under `?stage`, its clock held and stepped in ticks of a sixtieth of a second (on
// whole milliseconds), screenshotting each tick of the suite's verbs' steps. `by motion`
// measures the frames with `by video measure`, the tool Arena's numbers were taken with, and
// holds each step to its verb's own values (tools/src/beryl_tools/motion.py). Usage:
//   node scripts/stepped.mjs <base-url> <fixtures-dir> <out-dir> [--tick <ms>] [--more]
// The first moment (`moment`: a cast, its trigger and tokens) is played until the trigger is done
// and every step of the suite's verbs in it is shot. With `--more`, the moments of `more` that MORE
// names are played too, each to its recording's end, and of their steps of the verbs MORE names for
// each one of each kind is shot (`kindOf`), the first whose subject is still either side of it and
// comes to rest in the window, as the tracker needs; the others are only timed. A step of an exempt
// verb, or a life total's count, is only timed. Each seat of a moment is played twice on a page of
// its own: once to learn when each step begins and ends and where its subject (the card or ability
// it is about) goes, and once to screenshot it, which the stage plays the same to the bit because
// only its clock moves it. For each step shot it writes a folder
// `<out-dir>/<layout>/<play>/<n>-<verb>/` of frames from a few ticks before the step to a few
// after, cropped to where its subject went and named by their stage time in ms as `by video
// measure` reads a folder (`001017.000-yours.png`), its subject drawn alone, with the plate
// (`plate.png`: the first frame with no node drawn, which the tracker finds the subject against),
// where a play is the seat for the first moment and `<seat>.<moment>` for the others; and
// `<out-dir>/<layout>/<play>/steps.json`, every step taken with its act, case, turn and stage times
// and, for one shot, its folder and where in its frames its subject comes to rest. Nothing here
// reads the page's own time, so no frame depends on the machine's speed. Exits non-zero if a moment
// does not play out, the two plays differ or a page logs an error.
import { isDeepStrictEqual } from "node:util";
import { mkdir, readFile, rm, writeFile } from "node:fs/promises";
import { basename, join } from "node:path";
import { launch } from "./capture.mjs";
import { ending } from "./fixture-views.mjs";
import { install as installMarks } from "./written-marks.mjs";

const args = process.argv.slice(2);
const more = args.includes("--more");
if (more) args.splice(args.indexOf("--more"), 1);
const flag = args.indexOf("--tick");
const TICK = flag >= 0 ? Number(args.splice(flag, 2)[1]) : 1000 / 60;
const [base, fixtures, out] = args;
if (!base || !fixtures || !out || !(TICK > 0)) {
    console.error(
        "usage: node scripts/stepped.mjs <base-url> <fixtures-dir> <out-dir> [--tick <ms>] [--more]",
    );
    process.exit(2);
}
const {
    verbs,
    moment,
    more: moments,
} = JSON.parse(
    await readFile(new URL("../verbs.json", import.meta.url), "utf8"),
);
const conventions = JSON.parse(
    await readFile(
        new URL("../../data/ui-conventions.json", import.meta.url),
        "utf8",
    ),
);
/** The verbs a numbered exception exempts (data/ui-conventions.json): a tap, which turns where
 * it lies, so none of their steps is shot; each is timed as every step is. */
const exempt = new Set(
    conventions.verbs
        .filter((verb) => "exempt" in verb)
        .map((verb) => verb.name),
);
/** The verbs whose subject travels (their curve is no `none`): the tracker reads how one eases
 * only where it comes to rest in the frames. One that changes in place (a card that leaves
 * dissolves there) is read wherever its card goes after. */
const travels = new Set(
    conventions.verbs
        .filter((verb) => !["none", null].includes(verb.values.curve))
        .map((verb) => verb.name),
);
/** The layouts captured, as verbs.mjs opens them. The phone's battlefield is laid out again
 * as the stack comes and goes, so a step's frames hold more than its subject moving: not
 * captured yet. */
const LAYOUTS = { desktop: { query: "stage", width: 1600, height: 900 } };
/** The moments of `more` played besides the first with `--more`, each with the verbs whose
 * steps are taken there: cards changing zone (`zones`, `scry`), and changes on a card where it
 * lies and to players (`combat`, `place`); and in three of them the arrows and lines the first
 * moment has none of (an arrow at a target, the lines of combat), which are only timed: a
 * line is no card for the tracker to follow. A scry's flips are a library card's, which the
 * stage turns at once (SNAP), not the face-down permanent's flip its values are for. `burn`
 * has what the others lack of the verbs that move no card: a turn's banner, a coin, and a
 * streak of damage with the life it takes. */
const MORE = {
    zones: new Set(["fly", "vanish", "lift", "point", "dwell"]),
    scry: new Set(["fly", "vanish", "lift", "announce"]),
    look: new Set(["lift"]),
    combat: new Set(["tap", "strike", "count", "point", "streak"]),
    place: new Set(["tap", "count", "flip", "point"]),
    burn: new Set(["streak", "announce", "count", "set"]),
};
/** The verbs that move no card (a streak of light, a notice over the table, a wait, a change
 * with no motion): each step is timed, and none is shot for the tracker. */
const UNSEEN = new Set(["streak", "announce", "dwell", "set"]);
/** How long the stage stands idle before the seat answers, in stage ms (verbs.mjs GAP). */
const GAP = 250;
/** The most answers the first moment takes (verbs.mjs ANSWERS). */
const ANSWERS = 6;
/** Ticks taken either side of a step: `by video measure` needs two still frames at each end. */
const EDGE = 3;
/** How far around where a step's subject went its frames reach, in its widths. */
const MARGIN = 0.3;

/** In the page: plays a moment as the recording's seat did, holding the stage's clock and
 * advancing it in ticks of `tick`: the seat answers once the stage has stood idle for `gap`,
 * until an ability has come onto the stack and gone, and `edge` ticks after; or with `whole`,
 * for all of its `answers`, to the recording's end. Before each tick whose number is in
 * `shoot` (pairs of a tick's number and the subjects shot then) it draws each subject alone
 * and waits on `window.berylShoot(number, time, subject)`; for a subject of null, the plate, it
 * hides every node. Returns every step the stage told of, with its subject and its stage times from
 * the first tick, each tick's time and the box on the page of every node drawn then, and what the
 * stage shows at the end, as verbs.mjs's walk tells it (`shows`). */
async function walk({ tick, gap, answers, edge, shoot, whole, written }) {
    /** What a step is about, by the name the stage gives it: its card, or the ability; for a
     * look, the top one of the library cards it lifts (src/stage/table.ts handledAt), or none
     * where it lifts none, as for a look at a hand the seat does not see, or as it ends: the
     * lift's motion is its cards coming off (the look moment shoots it); a strike's source, and
     * the card a tally is on. None for a count of life, which is about a player. */
    const subjectOf = (act, view) => {
        if (act.verb === "emerge") return `a${act.id}`;
        if (act.verb === "strike") return String(act.source);
        if (act.verb === "lift" && act.span !== "begin") return null;
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
        const object = act.object ?? act.tally?.object;
        return object ? String(object.slot) : null;
    };
    const stage = window.berylStage;
    const { clock } = stage;
    clock.hold();
    // What is measured is the recording's steps and the marks written by hand: no prompt's.
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
            const subject = subjectOf(act, e.table.view);
            // Its case as the stage times it (src/stage/timing.ts caseOf), from the table it
            // finds: a life total's rise or fall is not told by the step alone.
            const found = window.berylCase(act, e.table.view) ?? null;
            const turn = e.span.turn - origin;
            steps.push({ act, subject, case: found, turn, from: at, to: at });
        } else if (open.has(e.step)) {
            steps[open.get(e.step)].to = at;
            open.delete(e.step);
        }
        lifted ||= e.step.act.verb === "emerge";
    });
    const layer = document.querySelector(".card-layer");
    const wanted = new Map(shoot);
    // A shot is of its subject alone, every other node hidden, so that the tracker follows
    // the card the step is about and not the cards beside it (a batch flying the same way, a
    // hand making room), nor the line that joins an ability to its source. Hiding a node
    // moves nothing: the stage's poses are as they were.
    const alone = document.head.appendChild(document.createElement("style"));
    const ticks = [];
    let abilities = false;
    // What React draws of a tick (a prompt that dims the table sinks the hand: StageBoard's
    // hold) comes in a task of its own, as it would within a frame in the app. The play that
    // shoots yields to the page at every shot, so each play yields at every tick, after React's
    // render, which a default update queues as a message on the page (React's scheduler) and
    // does not cut into slices: both plays see it drawn at the same tick.
    const settle = () =>
        new Promise((resolve) => {
            const { port1, port2 } = new MessageChannel();
            port1.onmessage = () => {
                port1.close();
                resolve();
            };
            port2.postMessage(null);
        });
    const sample = async () => {
        await settle();
        const table = layer.getBoundingClientRect();
        const by = table.width / layer.offsetWidth;
        const boxes = {};
        abilities = false;
        for (const name of stage.shown.resting.keys()) {
            abilities ||= typeof name === "string";
            const pose = stage.pose(name);
            if (!pose || pose.opacity === 0) continue;
            // As the node is drawn (src/stage/pose.ts transformOf): a card's frame, 63 by 88,
            // leaned and turned about its top-left corner, which lies at the pose's place (the
            // opponent's cards lie upside down, up and left of it); the box around it.
            const [w, h] = [pose.width, (pose.width * 88) / 63];
            const lean = Math.tan((pose.skew * Math.PI) / 180);
            const [cos, sin] = [Math.cos, Math.sin].map((f) =>
                f((pose.rotation * Math.PI) / 180),
            );
            const corners = [
                [0, 0],
                [w, 0],
                [0, h],
                [w, h],
            ].map(([x, y]) => [x + y * lean, y]);
            const xs = corners.map(([x, y]) => pose.x + x * cos - y * sin);
            const ys = corners.map(([x, y]) => pose.y + x * sin + y * cos);
            boxes[name] = [
                table.left + Math.min(...xs) * by,
                table.top + Math.min(...ys) * by,
                table.left + Math.max(...xs) * by,
                table.top + Math.max(...ys) * by,
            ];
        }
        const at = clock.now - origin;
        for (const subject of wanted.get(ticks.length) ?? []) {
            alone.textContent =
                subject === null
                    ? ".stage-node, .stage-lines { visibility: hidden; }"
                    : `.stage-node:not([data-stage="${subject}"]), .stage-lines { visibility: hidden; }`;
            await window.berylShoot(ticks.length, at, subject);
        }
        alone.textContent = "";
        ticks.push({ at, boxes });
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
        await new Promise((resolve) => setTimeout(resolve));
        let idle = 0;
        while (after < edge && idle < gap) {
            // On the whole milliseconds nearest each tick's time, so that two plays read the
            // same to the bit wherever the page's clock stood when it was held.
            const took =
                Math.round(ticks.length * tick) -
                Math.round((ticks.length - 1) * tick);
            clock.advance(took);
            await sample();
            // Over once an ability has come onto the stack and gone, as verbs.mjs's walk is;
            // a whole recording, once its last answer has played out.
            finished ||= !whole && lifted && !abilities;
            if (finished) after += 1;
            // No step plays while what a card left room for closes up after its step.
            idle = stage.step || stage.shown.busy ? 0 : idle + took;
        }
    }
    // The marks, from the script written by hand (written-marks.mjs), each beat a script of
    // its own over the table the moment leaves, once what is left of it has played out.
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
    const shown = stage.shown.view;
    // An announcement still passing over the table as the walk ends was told no end: it is
    // not timed.
    const passing = new Set(open.values());
    return {
        steps: steps.filter((_, n) => !passing.has(n)),
        ticks,
        done: whole ? !stage.shown.busy : after >= edge,
        shows: shown && [shown.turn.number, shown.turn.step, shown.status],
    };
}

/** What tells the step `n` of `steps` from others of its verb, as far as which of them are
 * shot: its case and what the case is read from (where it came from and goes, why, what it
 * is, which end of a span), whose it is for a flight (your own draw is held up and an
 * opponent's is not), and whether a like step begins beside it (draws together, or one alone). */
function kindOf(steps, n) {
    const sort = ({ act, case: found }) =>
        JSON.stringify([
            found,
            act.verb,
            act.from,
            act.object?.zone,
            act.why,
            act.subject?.kind,
            act.span,
            act.verb === "fly" ? act.object.controller : null,
        ]);
    const own = sort(steps[n]);
    const beside = [steps[n - 1], steps[n + 1]].some(
        (step) => step && sort(step) === own,
    );
    return `${own}${beside ? " together" : " alone"}`;
}

/** The ticks of a step's frames, where on the page they are cropped to (around every box its
 * subject had from the tick before it to the tick after, within the window) and where in them
 * its subject comes to rest; or, where the tracker could not read them, why (`unshot`). */
function framing(step, ticks, size, strict) {
    const first = ticks.findIndex((t) => t.at >= step.from);
    let last = ticks.findIndex((t) => t.at >= step.to);
    if (first < 0 || last < 0) return { unshot: "the walk ended first" };
    const { subject } = step;
    if (step.act.verb === "count" && step.act.tally.kind === "life") {
        return {
            unshot: "a life total's change is drawn over its orb by the screen, not the stage",
        };
    }
    const seen = ticks
        .slice(Math.max(0, first - 1), last + 2)
        .map((t) => t.boxes[subject])
        .filter(Boolean);
    if (!subject || !seen.length) {
        return { unshot: "its subject is not drawn" };
    }
    // The tracker measures a card against still frames either side of its motion, and where
    // it comes to rest in the last: with `strict`, a subject that moves again right after the
    // step (a look that ends as soon as it begins) or travels to rest mostly out of the window
    // (to the opponent's hand, above it) is not shot, where another of its kind may be.
    const still = (from, to) =>
        new Set(
            ticks
                .slice(Math.max(0, from), to + 1)
                .map((t) => JSON.stringify(t.boxes[subject] ?? null)),
        ).size === 1;
    if (strict && !still(first - EDGE, first - 1)) {
        return { unshot: "its subject moves just before it" };
    }
    if (strict && !still(last, last + EDGE)) {
        return { unshot: "its subject moves again just after it" };
    }
    // Where it comes to rest, or was last seen.
    const resting = ticks[last].boxes[subject];
    const [rx0, ry0, rx1, ry1] = resting ?? seen.at(-1);
    const [cx, cy] = [(rx0 + rx1) / 2, (ry0 + ry1) / 2];
    const outside = cx < 0 || cy < 0 || cx > size.width || cy > size.height;
    if (strict && resting && travels.has(step.act.verb) && outside) {
        return { unshot: "its subject comes to rest out of the window" };
    }
    const pad = MARGIN * Math.max(...seen.map((b) => b[2] - b[0]));
    const x0 = Math.max(
        0,
        Math.floor(Math.min(...seen.map((b) => b[0])) - pad),
    );
    const y0 = Math.max(
        0,
        Math.floor(Math.min(...seen.map((b) => b[1])) - pad),
    );
    const x1 = Math.min(
        size.width,
        Math.ceil(Math.max(...seen.map((b) => b[2])) + pad),
    );
    const y1 = Math.min(
        size.height,
        Math.ceil(Math.max(...seen.map((b) => b[3])) + pad),
    );
    // As a share of the frames' width and height.
    const rest = [(cx - x0) / (x1 - x0), (cy - y0) / (y1 - y0)];
    last = Math.min(ticks.length - 1, last + EDGE);
    return {
        ticks: [Math.max(0, first - EDGE), last],
        clip: { x: x0, y: y0, width: x1 - x0, height: y1 - y0 },
        rest,
    };
}

const browser = await launch();
let failed = false;

/** A page of `size` on the stage, whose fixtures are this run's, at `fixture`'s first view. */
async function open(size, query, fixture) {
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

/** Plays a moment in `page` (`walk`): to its `end`, or the suite's first as far as its
 * ability's going, and after that one the marks written by hand, which no recording has. */
const play = async (page, end, shoot) => {
    const written = end === undefined && verbs.includes("mark");
    if (written) await installMarks(page);
    return page.evaluate(walk, {
        tick: TICK,
        gap: GAP,
        answers: end?.answers ?? ANSWERS,
        edge: EDGE,
        shoot,
        whole: end !== undefined,
        written,
    });
};

/** What a recorded fixture of this run ends on (fixture-views.mjs `ending`). */
const endOf = async (fixture) =>
    ending(
        JSON.parse(await readFile(join(fixtures, `${fixture}.json`), "utf8")),
    );

/** The steps of `learned` (a walk's) taken from a play, each numbered by its place among them
 * and named by its folder, and framed where it is shot: of the first moment every step of the
 * suite's verbs (`moves` null); of another, every step of `moves`' verbs, of which the first
 * of each kind `taken` (shared by a seat's plays) has not had shot is shot, where the tracker
 * can read it (`framing`), and the rest only timed. A step of an exempt verb (a tap, which
 * turns where it lies) is only timed, and so is one that begins or ends an arrow, a line or
 * a mark: the tracker follows a card that travels. A step not
 * shot says why. */
function choose(learned, size, moves, taken) {
    return learned.steps.flatMap((step, n) => {
        const verb = step.act.verb;
        if (!verbs.includes(verb)) return [];
        if (moves && !moves.has(verb)) return [];
        const listed = { ...step, n, verb, folder: `${n}-${verb}` };
        if (exempt.has(verb)) {
            return [
                {
                    ...listed,
                    unshot: "it turns where it lies, which the tracker does not read",
                },
            ];
        }
        if (verb === "point" || verb === "mark") {
            return [
                {
                    ...listed,
                    unshot: "it is a line or a mark, no card for the tracker to follow",
                },
            ];
        }
        if (UNSEEN.has(verb)) {
            return [
                {
                    ...listed,
                    unshot: "it moves no card for the tracker to follow",
                },
            ];
        }
        const kind = kindOf(learned.steps, n);
        if (moves && taken.has(kind)) return [listed];
        const framed = framing(step, learned.ticks, size, moves !== null);
        if (!framed.unshot) taken.add(kind);
        return [{ ...listed, ...framed }];
    });
}

for (const [layout, { query, width, height }] of Object.entries(LAYOUTS)) {
    const size = { width, height };
    for (const [seat, { whole }] of Object.entries(moment)) {
        const plays = [
            { name: seat, fixture: whole, moves: null },
            ...Object.entries(more ? MORE : {}).map(([name, moves]) => ({
                name: `${seat}.${name}`,
                fixture: moments[name][seat],
                moves,
            })),
        ];
        const taken = new Set();
        for (const { name, fixture, moves } of plays) {
            const folder = join(out, layout, name);
            await rm(folder, { recursive: true, force: true });
            await mkdir(folder, { recursive: true });
            const pages = [];
            try {
                const end = moves ? await endOf(fixture) : undefined;
                const first = await open(size, query, fixture);
                pages.push(first);
                const learned = await play(first.page, end, []);
                if (!learned.done)
                    throw new Error("the moment did not play out");
                if (end && !isDeepStrictEqual(learned.shows, end.shows)) {
                    throw new Error(
                        "the stage did not come to the recording's end",
                    );
                }
                const captures = choose(learned, size, moves, taken);
                const shots = new Map();
                for (const capture of captures.filter((c) => c.clip)) {
                    await mkdir(join(folder, capture.folder));
                    for (let t = capture.ticks[0]; t <= capture.ticks[1]; t++) {
                        shots.set(t, [...(shots.get(t) ?? []), capture]);
                    }
                }
                const second = await open(size, query, fixture);
                pages.push(second);
                await second.page.exposeFunction(
                    "berylShoot",
                    async (tick, at, subject) => {
                        // The plate, every node hidden, at a step's first tick: what the
                        // tracker finds its subject against (video_measure.py, PLATE).
                        const file =
                            subject === null
                                ? "plate.png"
                                : `${at.toFixed(3).padStart(10, "0")}-${seat}.png`;
                        const here = shots
                            .get(tick)
                            .filter((capture) =>
                                subject === null
                                    ? capture.ticks[0] === tick
                                    : capture.subject === subject,
                            );
                        for (const capture of here) {
                            // CSS animations run on the page's time, not the stage's (a
                            // counter's glow pulses so): they are stopped, so that only the
                            // stage moves.
                            await second.page.screenshot({
                                path: join(folder, capture.folder, file),
                                clip: capture.clip,
                                animations: "disabled",
                            });
                        }
                    },
                );
                const shoot = [...shots].map(([tick, here]) => [
                    tick,
                    [
                        ...(here.some((capture) => capture.ticks[0] === tick)
                            ? [null]
                            : []),
                        ...new Set(here.map((capture) => capture.subject)),
                    ],
                ]);
                const shot = await play(second.page, end, shoot);
                if (!isDeepStrictEqual(shot, learned)) {
                    throw new Error(
                        "the stage played the moment otherwise the second time",
                    );
                }
                await writeFile(
                    join(folder, "steps.json"),
                    `${JSON.stringify({ seat, layout, tick: TICK, steps: captures }, null, 1)}\n`,
                );
                const errors = pages.flatMap((p) => p.errors);
                if (errors.length) throw new Error(errors.join("\n"));
                const framed = captures.filter((c) => c.clip).length;
                console.log(
                    `captured: ${layout}/${name}, ${framed} of ${captures.length} steps shot, ${shots.size} ticks`,
                );
            } catch (e) {
                failed = true;
                console.error(`FAILED: ${layout}/${name}\n${e.stack ?? e}`);
            } finally {
                for (const { context } of pages) await context.close();
            }
        }
    }
}
await browser.close();
process.exit(failed ? 1 : 0);
