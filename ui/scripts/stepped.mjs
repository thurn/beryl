// The stepped capture (plan K27, Motion oracle): the verb suite's moment (ui/verbs.json) played
// on the stage under `?stage`, its clock held and stepped in ticks of a sixtieth of a second (on
// whole milliseconds), screenshotting each tick of every step of the suite's verbs. `by motion`
// measures the frames with `by video measure`, the tool Arena's numbers were taken with, and
// holds each step to its verb's own values (tools/src/beryl_tools/motion.py). Usage:
//   node scripts/stepped.mjs <base-url> <fixtures-dir> <out-dir> [--tick <ms>]
// Each seat of the moment is played twice on a page of its own: once to learn when each step
// begins and ends and where its subject (the card or ability it is about) goes, and once to
// screenshot it, which the stage plays the same to the bit because only its clock moves it.
// For each step it writes a folder `<out-dir>/<layout>/<seat>/<n>-<verb>/` of frames from a few
// ticks before the step to a few after, cropped to where its subject went and named by their
// stage time in ms as `by video measure` reads a folder (`001017.000-yours.png`); and
// `<out-dir>/<layout>/<seat>/steps.json`, every step with its act, stage times, folder and
// where in its frames its subject comes to rest. Nothing here reads the page's own time, so no
// frame depends on the machine's speed. Exits non-zero if the moment does not play out, the two
// plays differ or a page logs an error.
import { isDeepStrictEqual } from "node:util";
import { mkdir, readFile, rm, writeFile } from "node:fs/promises";
import { basename, join } from "node:path";
import { launch } from "./capture.mjs";

const args = process.argv.slice(2);
const flag = args.indexOf("--tick");
const TICK = flag >= 0 ? Number(args.splice(flag, 2)[1]) : 1000 / 60;
const [base, fixtures, out] = args;
if (!base || !fixtures || !out || !(TICK > 0)) {
    console.error(
        "usage: node scripts/stepped.mjs <base-url> <fixtures-dir> <out-dir> [--tick <ms>]",
    );
    process.exit(2);
}
const { verbs, moment } = JSON.parse(
    await readFile(new URL("../verbs.json", import.meta.url), "utf8"),
);
/** The verbs a numbered exception exempts (data/ui-conventions.json): Beryl's own, which no
 * capture holds to Arena's, so none of their steps is shot (`by motion` lists them). */
const exempt = new Set(
    JSON.parse(
        await readFile(
            new URL("../../data/ui-conventions.json", import.meta.url),
            "utf8",
        ),
    )
        .verbs.filter((verb) => "exempt" in verb)
        .map((verb) => verb.name),
);
/** The layouts captured, as verbs.mjs opens them. The phone's battlefield is laid out again
 * as the stack comes and goes, so a step's frames hold more than its subject moving: not
 * captured yet. */
const LAYOUTS = { desktop: { query: "stage", width: 1600, height: 900 } };
/** How long the stage stands idle before the seat answers, in stage ms (verbs.mjs GAP). */
const GAP = 250;
/** The most answers the moment takes (verbs.mjs ANSWERS). */
const ANSWERS = 6;
/** Ticks taken either side of a step: `by video measure` needs two still frames at each end. */
const EDGE = 3;
/** How far around where a step's subject went its frames reach, in its widths. */
const MARGIN = 0.3;

/** In the page: plays the moment as the recording's seat did, holding the stage's clock and
 * advancing it in ticks of `tick`: the seat answers once the stage has stood idle for `gap`,
 * until an ability has come onto the stack and gone, and `edge` ticks after. Before each tick
 * whose number is in `shoot` it waits on `window.berylShoot(number, time)`. Returns every step
 * the stage told of, with its stage times from the first tick, and each tick's time and the
 * box on the page of everything drawn then (a card's frame is 63 by 88). */
async function walk({ tick, gap, answers, edge, shoot }) {
    const stage = window.berylStage;
    const { clock } = stage;
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
            steps.push({ act: e.step.act, from: at, to: at });
        } else if (open.has(e.step)) {
            steps[open.get(e.step)].to = at;
        }
        lifted ||= e.step.act.verb === "emerge";
    });
    const layer = document.querySelector(".card-layer");
    const wanted = new Set(shoot);
    const ticks = [];
    let abilities = false;
    const sample = async () => {
        const table = layer.getBoundingClientRect();
        const by = table.width / layer.offsetWidth;
        const boxes = {};
        abilities = false;
        for (const name of stage.shown.resting.keys()) {
            abilities ||= typeof name === "string";
            const pose = stage.pose(name);
            if (!pose || pose.opacity === 0) continue;
            const x = table.left + pose.x * by;
            const y = table.top + pose.y * by;
            const width = pose.width * by;
            boxes[name] = [x, y, x + width, y + (width * 88) / 63];
        }
        const at = clock.now - origin;
        if (wanted.has(ticks.length)) await window.berylShoot(ticks.length, at);
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
            // Over once an ability has come onto the stack and gone, as verbs.mjs's walk is.
            finished ||= lifted && !abilities;
            if (finished) after += 1;
            idle = stage.step ? 0 : idle + took;
        }
    }
    unfollow();
    return { steps, ticks, done: after >= edge };
}

/** What a step is about, by the name the stage gives it: its card, or the ability. */
function subjectOf(act) {
    if (act.verb === "emerge") return `a${act.id}`;
    if (act.subject?.kind === "ability") return `a${act.subject.id}`;
    if (act.subject?.kind === "object") return String(act.subject.slot);
    return act.object ? String(act.object.slot) : null;
}

/** The ticks of a step's frames, and where on the page they are cropped to: around every box
 * its subject had from the tick before it to the tick after, within the window. */
function framing(step, ticks, size) {
    const first = ticks.findIndex((t) => t.at >= step.from);
    let last = ticks.findIndex((t) => t.at >= step.to);
    if (first < 0 || last < 0) return null;
    const subject = subjectOf(step.act);
    const seen = ticks
        .slice(Math.max(0, first - 1), last + 2)
        .map((t) => t.boxes[subject])
        .filter(Boolean);
    if (!subject || !seen.length) return null;
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
    // Where it comes to rest, or was last seen, as a share of the frames' width and height.
    const [rx0, ry0, rx1, ry1] = ticks[last].boxes[subject] ?? seen.at(-1);
    const rest = [
        ((rx0 + rx1) / 2 - x0) / (x1 - x0),
        ((ry0 + ry1) / 2 - y0) / (y1 - y0),
    ];
    last = Math.min(ticks.length - 1, last + EDGE);
    return {
        subject,
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

const play = (page, shoot) =>
    page.evaluate(walk, {
        tick: TICK,
        gap: GAP,
        answers: ANSWERS,
        edge: EDGE,
        shoot,
    });

for (const [layout, { query, width, height }] of Object.entries(LAYOUTS)) {
    const size = { width, height };
    for (const [seat, { whole }] of Object.entries(moment)) {
        const folder = join(out, layout, seat);
        await rm(folder, { recursive: true, force: true });
        await mkdir(folder, { recursive: true });
        const pages = [];
        try {
            const first = await open(size, query, whole);
            pages.push(first);
            const learned = await play(first.page, []);
            if (!learned.done) throw new Error("the moment did not play out");
            const captures = learned.steps
                .map((step, n) => ({ ...step, n, verb: step.act.verb }))
                .filter(
                    (step) =>
                        verbs.includes(step.verb) && !exempt.has(step.verb),
                )
                .map((step) => {
                    const framed = framing(step, learned.ticks, size);
                    return (
                        framed && {
                            ...step,
                            ...framed,
                            folder: `${step.n}-${step.verb}`,
                        }
                    );
                })
                .filter(Boolean);
            const shots = new Map();
            for (const capture of captures) {
                await mkdir(join(folder, capture.folder));
                for (let t = capture.ticks[0]; t <= capture.ticks[1]; t++) {
                    shots.set(t, [...(shots.get(t) ?? []), capture]);
                }
            }
            const second = await open(size, query, whole);
            pages.push(second);
            await second.page.exposeFunction("berylShoot", async (tick, at) => {
                const name = `${at.toFixed(3).padStart(10, "0")}-${seat}.png`;
                for (const capture of shots.get(tick)) {
                    // CSS animations run on the page's time, not the stage's (a counter's
                    // glow pulses so): they are stopped, so that only the stage moves.
                    await second.page.screenshot({
                        path: join(folder, capture.folder, name),
                        clip: capture.clip,
                        animations: "disabled",
                    });
                }
            });
            const shot = await play(second.page, [...shots.keys()]);
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
            console.log(
                `captured: ${layout}/${seat}, ${captures.length} steps, ${shots.size} ticks`,
            );
        } catch (e) {
            failed = true;
            console.error(`FAILED: ${layout}/${seat}\n${e.stack ?? e}`);
        } finally {
            for (const { context } of pages) await context.close();
        }
    }
}
await browser.close();
process.exit(failed ? 1 : 0);
