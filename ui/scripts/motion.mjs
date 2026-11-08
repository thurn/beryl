// The motion audit (`by specs --motion`, tools/): plays each moment of the specs from both
// seats with motion on and checks that everything that happens in the game is animated
// clearly for both players. Usage:
//   node scripts/motion.mjs <base-url> <out.json> [card-slug...] [--moment NAME] [--minor]
// The measuring and the verdicts are audit.mjs's. Prints each finding by moment and seat,
// writes them all to <out.json>, and exits 1 if any is not ok. Minor findings (taps and
// untaps) are written but printed and counted only with --minor.
import { writeFile } from "node:fs/promises";
import { audit, collect, record } from "./audit.mjs";
import { launch, workers } from "./capture.mjs";
import { chosen, moving, openSeat, play, settled, SEATS } from "./moments.mjs";

const args = process.argv.slice(2);
const minor = args.includes("--minor");
if (minor) args.splice(args.indexOf("--minor"), 1);
const at = args.indexOf("--moment");
const moment = at >= 0 ? args.splice(at, 2)[1] : undefined;
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
const run = workers();
const results = [];
let bad = 0;
for (const spec of specs) {
    for (const m of spec.moments) {
        for (const seat of SEATS) {
            await run(async () => {
                const { context, page, errors } = await moving(browser);
                const result = {
                    card: spec.slug,
                    moment: m.name,
                    seat,
                    findings: [],
                };
                try {
                    const live = await openSeat(
                        page,
                        base,
                        spec.slug,
                        m.name,
                        seat,
                    );
                    const since = await page.evaluate(() => performance.now());
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
                result.errors = errors;
                results.push(result);
                const off = result.findings.filter(
                    (f) => f.verdict !== "ok" && (minor || !f.minor),
                );
                bad += off.length + errors.length;
                const head = `${spec.slug} / ${m.name} (${seat})`;
                if (off.length === 0 && errors.length === 0) {
                    console.log(
                        `ok       ${head}: ${result.findings.length} events`,
                    );
                    return;
                }
                console.log(head);
                for (const f of off) {
                    console.log(
                        `  ${f.verdict.padEnd(8)} ${f.view}: ${f.what}: ${f.why}`,
                    );
                }
                for (const e of errors) console.log(`  error    ${e}`);
            });
        }
    }
}
await run.drain();
await browser.close();
await writeFile(out, JSON.stringify(results, null, 1) + "\n");
console.log(`motion: ${bad} findings; every event in ${out}`);
process.exit(bad ? 1 : 0);
