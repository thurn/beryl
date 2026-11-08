// The motion audit (`by specs --motion`, tools/): plays each moment of the specs from both
// seats with motion on and checks that everything that happens in the game is animated
// clearly for both players. Usage:
//   node scripts/motion.mjs <base-url> <out.json> [card-slug...] [--moment NAME] [--minor]
//     [--viewport WxH]
// The measuring and the verdicts are audit.mjs's. Prints each finding by moment and seat,
// writes them all to <out.json>, and exits 1 if any is not ok. Minor findings (taps and
// untaps, and what moved with no event to show) are written but printed and counted only
// with --minor. A seat that fails to open or play is tried again once, in a fresh page.
import { writeFile } from "node:fs/promises";
import { audit, collect, record } from "./audit.mjs";
import { launch, workers } from "./capture.mjs";
import {
    chosen,
    moving,
    openSeat,
    option,
    play,
    retried,
    settled,
    viewport,
    warm,
    SEATS,
} from "./moments.mjs";

const args = process.argv.slice(2);
const minor = option(args, "--minor", true);
const moment = option(args, "--moment");
const size = viewport(option(args, "--viewport"));
const [base, out, ...only] = args;
if (!base || !out) {
    console.error(
        "usage: node scripts/motion.mjs <base-url> <out.json> [card-slug...] [--moment NAME] [--minor]",
    );
    process.exit(2);
}

let specs;
try {
    specs = await chosen(base, only, moment);
} catch (e) {
    console.error(`motion: ${e.message}`);
    process.exit(2);
}
const browser = await launch();
await warm(browser, base);
const run = workers();
const results = [];
let bad = 0;
for (const spec of specs) {
    for (const m of spec.moments) {
        for (const seat of SEATS) {
            await run(async () => {
                const result = {
                    card: spec.slug,
                    moment: m.name,
                    seat,
                    findings: [],
                };
                let errors = [];
                await retried(async () => {
                    const opened = await moving(browser, size);
                    errors = opened.errors;
                    const { context, page } = opened;
                    try {
                        const live = await openSeat(
                            page,
                            base,
                            spec.slug,
                            m.name,
                            seat,
                        );
                        const since = await page.evaluate(() =>
                            performance.now(),
                        );
                        await page.evaluate(record);
                        await play(live);
                        await settled(page);
                        result.findings = audit(
                            await page.evaluate(collect, since),
                        );
                    } catch (e) {
                        errors.push(String(e).split("\n")[0]);
                    } finally {
                        await context.close();
                    }
                    // A seat that never opened or played is tried once more; the page's own
                    // errors are findings.
                    if (result.findings.length === 0 && errors.length) {
                        throw new Error(errors.join("; "));
                    }
                }, 2).catch(() => undefined);
                result.errors = errors;
                results.push(result);
                const off = result.findings.filter(
                    (f) => f.verdict !== "ok" && (minor || !f.minor),
                );
                bad += off.length + errors.length;
                const head = `${spec.slug} / ${m.name} (${seat})`;
                if (off.length === 0 && errors.length === 0) {
                    console.log(
                        `ok        ${head}: ${result.findings.filter((f) => f.kind !== "ui").length} events`,
                    );
                    return;
                }
                console.log(head);
                for (const f of off) {
                    console.log(
                        `  ${f.verdict.padEnd(9)} ${f.view}: ${f.what}: ${f.why}`,
                    );
                }
                for (const e of errors) console.log(`  error     ${e}`);
            });
        }
    }
}
await run.drain();
await browser.close();
await writeFile(out, JSON.stringify(results, null, 1) + "\n");
console.log(`motion: ${bad} findings; every event in ${out}`);
process.exit(bad ? 1 : 0);
