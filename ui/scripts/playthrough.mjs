// The fixture play-through (`by ui-play`, tools/): every recorded fixture played on the stage
// from its first view to its last, on the desktop's layout and the phone's, under the generic
// audits. Usage:
//   node scripts/playthrough.mjs <base-url> <fixtures-dir> [--minor] [name...]
// A name is a fixture's, or `play:<fixture>/<layout>`, the name each play is logged by. The
// stage's audit (stage-audit.mjs) judges every step of every view's script as the recording
// plays on, an answer at a time, on the stage's own clock; then the screen the play ends on is
// held to the recording's last view (capture.mjs' screenFaults and stageFaults). Prints each
// play's findings and exits 1 if any is not ok or a page logs an error. Minor findings (taps
// and untaps) are printed and counted only with --minor, as `by specs --motion` has them.
//
// A recording is played on by its transport's `choose` (src/ipc.ts), whatever its seat
// answered with: what a prompt draws and takes is the shape suite's (shapes.mjs), and this
// holds what the table plays between the prompts, in every game the fixtures record.
import { readFile } from "node:fs/promises";
import { basename, join } from "node:path";
import {
    launch,
    recordResult,
    screenFaults,
    settle,
    stageFaults,
    toldErrors,
    workers,
} from "./capture.mjs";
import { moving, option, retried } from "./moments.mjs";
import { watching } from "./stage-audit.mjs";

/** The layouts a recording is played in: the query its page opens with and its window. */
const LAYOUTS = {
    desktop: { query: "stage", size: { width: 1600, height: 900 } },
    phone: { query: "phone&stage", size: { width: 393, height: 759 } },
};

const args = process.argv.slice(2);
const minor = option(args, "--minor", true);
const [base, fixtures, ...only] = args;
if (!base || !fixtures) {
    console.error(
        "usage: node scripts/playthrough.mjs <base-url> <fixtures-dir> [--minor] [name...]",
    );
    process.exit(2);
}

const { fixtures: defined } = JSON.parse(
    await readFile(new URL("../fixtures.json", import.meta.url), "utf8"),
);
const plays = defined.flatMap(({ name }) =>
    Object.keys(LAYOUTS).map((layout) => ({
        fixture: name,
        layout,
        check: `play:${name}/${layout}`,
    })),
);
const chosen = plays.filter(
    (p) =>
        only.length === 0 || only.includes(p.fixture) || only.includes(p.check),
);
const unknown = only.filter(
    (name) => !plays.some((p) => p.fixture === name || p.check === name),
);
if (unknown.length) {
    console.error(`ui-play: no fixture named ${unknown.join(", ")}`);
    process.exit(2);
}

/** Serves `context`'s pages the recordings in the fixtures folder, each with an answer to
 * `choose` if it recorded none (a board that ends at its seat's first decision, or one
 * answered by clicks), so that every recording plays on by it. */
async function served(context) {
    await context.route("**/fixtures/*.json", async (route) => {
        const name = decodeURIComponent(
            basename(new URL(route.request().url()).pathname),
        );
        const recording = JSON.parse(
            await readFile(join(fixtures, name), "utf8"),
        );
        const answered = recording.messages.some(
            (m) => m.kind === "response" && m.command === "choose",
        );
        if (!answered) {
            recording.messages.push({
                kind: "response",
                command: "choose",
                value: null,
            });
        }
        await route.fulfill({ json: recording });
    });
}

/** Plays `fixture` to its end on `page` in `layout`. Returns the audit's findings, and the
 * faults of the screen it ends on. */
async function played(page, fixture, layout) {
    await page.goto(
        new URL(
            `/?${LAYOUTS[layout].query}&fixture=${fixture}&at=1#/game`,
            base,
        ).href,
    );
    await page.waitForFunction(() => window.berylStage && window.berylFixture);
    await page.evaluate(() => document.fonts.ready.then(() => {}));
    const findings = [];
    for (let more = true; more;) {
        const watched = await watching(page);
        // The transport hands the views an answer releases on in a microtask; a recording
        // that has played out releases none.
        more = await page.evaluate(async () => {
            const replay = window.berylFixture;
            const last = replay.view();
            await replay.invoke("choose", { request: {} });
            return replay.view() !== last;
        });
        findings.push(...(await watched()));
    }
    await settle(page);
    const faults = [
        ...(await toldErrors(page)).map(
            (e) => `the screen tells of an error: ${e}`,
        ),
        ...(await screenFaults(page)).map((fault) => `screen: ${fault}`),
        ...(await stageFaults(page)).map((fault) => `stage: ${fault}`),
    ];
    return { findings, faults };
}

const browser = await launch();
const run = workers();
let bad = 0;
for (const { fixture, layout, check } of chosen) {
    await run(async () => {
        const started = performance.now();
        let result = { findings: [], faults: [] };
        let errors = [];
        // A page that never opened or played is tried once more; its own errors are findings.
        await retried(async () => {
            const opened = await moving(browser, LAYOUTS[layout].size);
            errors = opened.errors;
            try {
                await served(opened.context);
                result = await played(opened.page, fixture, layout);
            } catch (e) {
                errors.push(String(e).split("\n")[0]);
            } finally {
                await opened.context.close();
            }
            if (result.findings.length === 0 && errors.length) {
                throw new Error(errors.join("; "));
            }
        }, 2).catch(() => undefined);
        const off = result.findings.filter(
            (f) => f.verdict !== "ok" && (minor || !f.minor),
        );
        const failed = off.length + result.faults.length + errors.length;
        bad += failed;
        recordResult(
            check,
            failed ? "fail" : "pass",
            (performance.now() - started) / 1000,
        );
        if (!failed) {
            console.log(`ok        ${check}: ${result.findings.length} steps`);
            return;
        }
        console.log(check);
        for (const f of off) {
            console.log(
                `  ${f.verdict.padEnd(9)} ${f.view}: ${f.what}: ${f.why}`,
            );
        }
        for (const fault of result.faults) console.log(`  fault     ${fault}`);
        for (const e of errors) console.log(`  error     ${e}`);
    });
}
await run.drain();
await browser.close();
console.log(`ui-play: ${bad} findings in ${chosen.length} plays`);
process.exit(bad ? 1 : 0);
