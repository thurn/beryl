// The web interface's timeline of a recording: when the stage began and ended each step of
// each script, as one seat sees it. Beryl 3D prints the same for the same recordings
// (`just b3d timeline`), and its timeline parity check compares the two (docs/beryl-3d.md,
// "Timeline parity"). Usage:
//   node scripts/timeline.mjs <directory>
// The directory holds one recording per seat, `<name>.<seat>.json`, where a `/` in the
// recording's name is written `__`: a recorded fixture, `{ messages }`, as `beryl_cli`'s
// fixture recorder writes it (or the bare array of its messages). Every file is played in
// name order, and one line of JSON is printed for each step, in the order the steps begin:
//   {"recording":"verbs-zones","seat":"you","view":3,"beat":0,"step":1,"verb":"fly",
//    "case":"own spell cast","start":0,"end":370,"effects":["settle@end"]}
// `view` counts the recording's views from 1. `beat` and `step` place a step the session sent
// in the script it sent, from 0. `start` and `end` are milliseconds from the start of the
// script the stage played the step in. A step the stage made itself while a prompt was open
// (prompt.ts: its marks, its arrows and the cards it holds up) has `"own": true`; its `beat`
// and `step` are its place in the script the stage played it in.
//
// The stage plays on a clock this steps itself, as Beryl 3D's test driver does (its
// `REPLY_DELAY` and `plays_on`): the first view is given at stage time 0, with every view
// after it that the game plays on through (one that neither asks the viewer something nor
// ends the game); once the stage has played all of that out, the prompt's own steps
// included, the next views are given 400 ms later. A step that ends a span waits for how long
// the span has stood, so the two clients' clocks must agree for their timelines to.
import { existsSync, readFileSync } from "node:fs";
import { readdir, readFile } from "node:fs/promises";
import { registerHooks } from "node:module";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import ts from "typescript";

// The desktop layout reaches modules written for Vite: imports with no extension, TypeScript
// that Node's type stripping cannot run (parameter properties), and a component's constants
// (`CARD_ASPECT` in components/Card.tsx). These hooks serve the files of src/ to Node: an
// import with no extension finds its `.ts` or `.tsx` file, and every `.ts` file is compiled
// by TypeScript. A `.tsx` file is a React component, whose imports reach the app's IPC and
// the browser, so it is served as its exported constants alone, those whose values are
// literals: an import of anything else from one fails as the module links, by name.
const SOURCE = new URL("../src/", import.meta.url).href;

/** Whether `statement` is exported. */
const isExported = (statement) =>
    ts
        .getModifiers(statement)
        ?.some((m) => m.kind === ts.SyntaxKind.ExportKeyword) ?? false;

/** The exported constants of the TSX module `text` whose values name nothing. */
function literalExports(file, text) {
    const source = ts.createSourceFile(file, text, ts.ScriptTarget.ESNext);
    const names = (node) =>
        ts.isIdentifier(node) || ts.forEachChild(node, names) === true;
    const kept = source.statements.flatMap((statement) => {
        if (!isExported(statement) || !ts.isVariableStatement(statement)) {
            return [];
        }
        if (!(statement.declarationList.flags & ts.NodeFlags.Const)) return [];
        return statement.declarationList.declarations.flatMap(
            ({ name, initializer }) =>
                ts.isIdentifier(name) && initializer && !names(initializer)
                    ? [
                          `export const ${name.text} = ${initializer.getText(source)};`,
                      ]
                    : [],
        );
    });
    return kept.join("\n");
}

/** The modules of src/ that read the browser as they load. */
const BROWSER = ["freeze.ts"];

/** A module with the exports of the TypeScript module `text`, each a function that throws if
 * called: for a module the stage's modules import from but never call while playing. */
function unavailable(file, text) {
    const source = ts.createSourceFile(file, text, ts.ScriptTarget.ESNext);
    const exported = source.statements.flatMap((statement) => {
        if (!isExported(statement)) return [];
        if (ts.isVariableStatement(statement)) {
            return statement.declarationList.declarations.map(
                (d) => d.name.text,
            );
        }
        if (
            ts.isFunctionDeclaration(statement) ||
            ts.isClassDeclaration(statement)
        ) {
            return [statement.name.text];
        }
        return [];
    });
    const thrower = (name) =>
        `export const ${name} = () => { throw new Error(${JSON.stringify(`${name} needs a browser`)}); };`;
    return exported.map(thrower).join("\n");
}

registerHooks({
    resolve(specifier, context, next) {
        const from = context.parentURL ?? "";
        if (from.startsWith(SOURCE) && /^\.\.?\//.test(specifier)) {
            const url = new URL(specifier, from);
            if (!/\.[a-z]+$/.test(url.pathname)) {
                for (const extension of [".ts", ".tsx"]) {
                    const file = new URL(url.href + extension);
                    if (existsSync(fileURLToPath(file))) {
                        return next(file.href, context);
                    }
                }
            }
        }
        return next(specifier, context);
    },
    load(url, context, next) {
        if (!url.startsWith(SOURCE) || !/\.tsx?$/.test(url)) {
            return next(url, context);
        }
        const file = fileURLToPath(url);
        const text = readFileSync(file, "utf8");
        const source = BROWSER.some((path) => url === SOURCE + path)
            ? unavailable(file, text)
            : url.endsWith(".tsx")
              ? literalExports(file, text)
              : ts.transpileModule(text, {
                    fileName: file,
                    compilerOptions: {
                        module: ts.ModuleKind.ESNext,
                        target: ts.ScriptTarget.ESNext,
                    },
                }).outputText;
        return { format: "module", source, shortCircuit: true };
    },
});

const { cardRecords } = await import("../src/cardRecords.ts");
const { desktop } = await import("../src/stage/desktop.ts");
const { Feed } = await import("../src/stage/feed.ts");
const { Prompts } = await import("../src/stage/prompt.ts");
const { ScriptStage } = await import("../src/stage/stage.ts");
const { ownTimings, played } = await import("../src/stage/timing.ts");

/** How long after the stage comes to rest the next views are given (Beryl 3D's driver,
 * `REPLY_DELAY`). */
const REPLY_DELAY = 400;

/** Whether the game plays on through `update` to the view after it (Beryl 3D's driver,
 * `plays_on`): it asks the viewer nothing and does not end the game. */
const playsOn = (update) =>
    update.view.status.kind === "thinking" ||
    update.view.status.kind === "playing";

/** `x` rounded to 3 decimal places, with no negative zero. */
const rounded = (x) => Math.round(x * 1000) / 1000 + 0;

/** An effect of a step's timing as a line names it: "impact@start+0.376". */
const effectName = ({ name, when, into }) =>
    `${name}@${when}${into === undefined ? "" : `+${into.toFixed(3)}`}`;

/** The lines of one recording's timeline, as `seat` sees it. */
function timeline(recording, seat, messages) {
    const timings = messages.find(
        (m) => m.kind === "response" && m.command === "stage_timings",
    );
    // A recording that never opens a prompt on the table needs no table of own timings.
    ownTimings(timings?.value ?? { entries: [] });
    // The layout reads cards' records (a land with a mana ability lies in its own row), which
    // the recording holds for every card it shows.
    const records = messages.find(
        (m) => m.kind === "response" && m.command === "card_records",
    )?.value;
    if (records) {
        cardRecords.keep(
            [...Object.keys(records.cards), ...Object.keys(records.tokens)],
            records,
        );
    }
    const updates = messages
        .filter((m) => m.kind === "event" && m.event === "game-update")
        .map((m) => m.payload);

    const stage = new ScriptStage({
        layout: desktop,
        timings: (step) => played(step).timing,
        effects: (step) => played(step).effects,
    });
    /** Where each step given to the stage came from, by the step. */
    const placed = new WeakMap();
    /** Each script the stage was given: when its first beat began, once a step has said. */
    const scripts = [];
    /** The number of the view given last, from 1. */
    let viewNumber = 0;
    /** The script the view given last came with, as the session sent it. */
    let sentScript;
    // Every script reaches the stage through `play`, the session's with the prompt's own
    // steps put among them (prompt.ts `paid`, `lowered`) and the prompt's own alone. Each
    // step is placed as it goes in: one the session sent carries its timing (`played`) and
    // is placed in the session's script by its order among the sent steps, which the
    // prompt's own steps do not change.
    const prompted = {
        play(view, script) {
            if (script) {
                const at = scripts.push({ start: Infinity }) - 1;
                const sent = sentScript?.beats.flatMap((beat, b) =>
                    beat.map((_, s) => [b, s]),
                );
                let k = 0;
                script.beats.forEach((beat, b) =>
                    beat.forEach((step, s) => {
                        const own = !step.played;
                        const [beatAt, stepAt] = own
                            ? [b, s]
                            : (sent?.[k++] ?? [b, s]);
                        placed.set(step, {
                            view: viewNumber,
                            beat: beatAt,
                            step: stepAt,
                            own,
                            script: at,
                        });
                    }),
                );
            }
            stage.play(view, script);
        },
        skip: () => stage.skip(),
        trade: (a, b) => stage.trade(a, b),
        declare: (picked) => stage.declare(picked),
        subscribe: (listener) => stage.subscribe(listener),
        get shown() {
            return stage.shown;
        },
    };
    const prompts = new Prompts(prompted, () => new Promise(() => {}));
    const feed = new Feed(prompts);

    /** The lines, in the order their steps began, with the step's start and end in stage
     * time until the recording has played. */
    const lines = [];
    const open = new Map();
    stage.follow((edge) => {
        const from = placed.get(edge.step);
        if (!from) throw new Error(`${recording}: a step no script placed`);
        const script = scripts[from.script];
        script.start = Math.min(script.start, edge.span.turn);
        if (from.own && !timings) {
            throw new Error(
                `${recording}: the stage made a step of its own, and the recording has no ` +
                    "stage_timings response to time it by",
            );
        }
        if (edge.edge === "start") {
            const { case: named, effects } = played(edge.step);
            const line = {
                recording,
                seat,
                view: from.view,
                beat: from.beat,
                step: from.step,
                verb: edge.step.act.verb,
                case: named ?? null,
                start: edge.at,
                end: edge.span.to,
                effects: effects.map(effectName),
                own: from.own,
                script,
            };
            lines.push(line);
            open.set(edge.step, line);
        } else {
            const line = open.get(edge.step);
            if (line) line.end = edge.at;
            open.delete(edge.step);
        }
    });

    let given = 0;
    while (given < updates.length) {
        // The next view, and every view after it that the game plays on through.
        for (;;) {
            const update = updates[given++];
            viewNumber = given;
            sentScript = update.view.script;
            feed.arrive(update, false);
            if (!playsOn(update) || given >= updates.length) break;
        }
        stage.settle(10 * 60_000);
        if (given < updates.length) stage.clock.advance(REPLY_DELAY);
    }
    // The keys in the order the format gives them.
    return lines.map((line) => ({
        recording: line.recording,
        seat: line.seat,
        view: line.view,
        beat: line.beat,
        step: line.step,
        verb: line.verb,
        case: line.case,
        start: rounded(line.start - line.script.start),
        end: rounded(line.end - line.script.start),
        effects: line.effects,
        ...(line.own ? { own: true } : {}),
    }));
}

const [dir] = process.argv.slice(2);
if (!dir) {
    console.error("usage: node scripts/timeline.mjs <directory>");
    process.exit(2);
}
const files = (await readdir(dir))
    .filter((name) => name.endsWith(".json"))
    .sort();
for (const file of files) {
    const match = /^(.+)\.([^.]+)\.json$/.exec(file);
    if (!match) {
        console.error(`${file}: not named <recording>.<seat>.json`);
        process.exit(2);
    }
    const [, name, seat] = match;
    const read = JSON.parse(await readFile(join(dir, file), "utf8"));
    const messages = Array.isArray(read) ? read : read.messages;
    const out = timeline(name.replaceAll("__", "/"), seat, messages);
    process.stdout.write(
        out.map((line) => JSON.stringify(line) + "\n").join(""),
    );
}
