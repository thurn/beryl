// The web interface's timing of every step of every recording, for `just timing-parity`, to
// hold beside the timing the session sent with it:
// `node scripts/timings.mjs <pack.json>` reads the recordings `beryl_cli step-timings` wrote,
// replays each view's script over the stage's table (src/stage/table.ts `apply`) and prints
// one JSON line per step with `timingOf`, `caseOf` and `effectsOf` (src/stage/timing.ts),
// written in the field names of the session's `StepTiming`, in the form that command prints
// its own lines. No browser: Node runs the stage's TypeScript as it is.
import { readFileSync } from "node:fs";
import { easeIn, easeInOut, easeOut } from "motion";
import { apply, tabled, tableOf } from "../src/stage/table.ts";
import { SNAP } from "../src/stage/stage.ts";
import { caseOf, effectsOf, timingOf } from "../src/stage/timing.ts";

/** A curve's name in `StepTiming`. */
function curveOf(ease) {
    if (ease === SNAP.ease) return "linear";
    if (ease === easeIn) return "easeIn";
    if (ease === easeOut) return "easeOut";
    if (ease === easeInOut) return "easeInOut";
    throw new Error(`a curve no data names: ${ease}`);
}

/** The script subject a stage name stands for (src/stage/spans.ts `namedBy`, reversed). */
function subjectOf(name) {
    if (typeof name === "number") return { kind: "object", slot: name };
    if (name === "you" || name === "opponent") {
        return { kind: "player", player: name };
    }
    if (/^a\d+$/.test(name))
        return { kind: "ability", id: Number(name.slice(1)) };
    throw new Error(`a name no subject has: ${name}`);
}

/** `record` without the fields that are undefined, as serde leaves out a `None`. */
const defined = (record) =>
    Object.fromEntries(
        Object.entries(record).filter(([, value]) => value !== undefined),
    );

function timed(act, view, before, beside) {
    const timing = timingOf(act, view, before, beside);
    const effects = effectsOf(act, view).map((effect) =>
        defined({
            name: effect.name,
            when: effect.when,
            on: effect.on === undefined ? undefined : subjectOf(effect.on),
            into: effect.into,
            duration: effect.duration,
            delay: effect.delay,
            curve: curveOf(effect.ease),
        }),
    );
    return defined({
        case: caseOf(act, view, beside),
        duration: timing.duration,
        delay: timing.delay,
        stagger: timing.stagger,
        curve: curveOf(timing.ease),
        each: timing.each,
        with: timing.with ?? false,
        after: (timing.after ?? []).map(subjectOf),
        hold: timing.hold,
        leads: timing.leads ?? false,
        passes: timing.passes ?? false,
        alone: timing.alone ?? false,
        opens: timing.opens ?? false,
        ahead: timing.ahead ?? false,
        closes: timing.closes,
        effects,
    });
}

const [packPath] = process.argv.slice(2);
if (!packPath) {
    console.error("usage: node scripts/timings.mjs <pack.json>");
    process.exit(2);
}
const pack = JSON.parse(readFileSync(packPath, "utf8"));
const lines = [];
for (const { name, seat, views } of pack) {
    // The table as the stage holds it between views: the last script's replay, with the
    // view's chrome (src/stage/stage.ts), which keeps the numbers the scripts gave the
    // abilities on its stack.
    let carried;
    views.forEach((view, n) => {
        const script = view.script;
        // The stage starts over from a view with no script, numbering no ability.
        if (!script) {
            carried = undefined;
            return;
        }
        // With a reset the script starts over from its own view's table; with none, it plays
        // over the table the scripts before it left.
        let table = script.reset ? tableOf(view, script.reset) : carried;
        carried = undefined;
        if (!table) return;
        // The session sends one timing with each step (`Script.timing`), which the lines
        // `beryl_cli step-timings` prints are: a script without one for each step is wrong.
        const steps = script.beats.reduce((n, beat) => n + beat.length, 0);
        if ((script.timing ?? []).length !== steps) {
            throw new Error(
                `${name} ${seat} view ${n + 1}: ${steps} steps and ` +
                    `${(script.timing ?? []).length} timings sent`,
            );
        }
        let applied = true;
        let k = 0;
        for (const beat of script.beats) {
            const beside = beat.map((step) => step.act);
            beat.forEach((step, i) => {
                const timing = timed(
                    step.act,
                    table.view,
                    beat[i - 1]?.act,
                    beside,
                );
                k += 1;
                lines.push(
                    JSON.stringify({
                        fixture: name,
                        seat,
                        view: n + 1,
                        step: k,
                        timing,
                    }),
                );
                // A step that cannot be applied leaves the table as it was, as the session's
                // `time_script` does, and no later view is timed until one starts over.
                try {
                    table = apply(table, step.act, step.cause);
                } catch {
                    applied = false;
                }
            });
        }
        if (applied) carried = { ...table, view: tabled(view) };
    });
}
process.stdout.write(lines.length ? lines.join("\n") + "\n" : "");
