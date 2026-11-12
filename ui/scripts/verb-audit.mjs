// The check of the audits themselves, run by the verb suite (verbs.mjs) with the verbs that
// come to be and go: the stage's audit (stage-audit.mjs) watches the suite's first moment, a
// permanent cast, its enters trigger and the tokens it makes, on a page of its own, and must
// pass every step of it, the token that appears with no flight and the ability gone between
// two frames among them. Then the same moment is played by a stage that skips a step of the
// verb (`?skip=<verb>`, src/stage/fault.ts), and the audit must report that step. An audit
// that passed whatever it was shown would pass both.
import assert from "node:assert/strict";
import { watching } from "./stage-audit.mjs";

/** The most answers the moment takes (verbs.mjs ANSWERS). */
const ANSWERS = 6;

/** The verbs whose check audits the moment, each with the step of it the audit must pass. */
export const AUDITED = {
    appear: (f) => f.kind === "appear" && /\(created\)$/.test(f.what),
    vanish: (f) => f.kind === "vanish" && /^ability \d+ leaves/.test(f.what),
};

/** Plays `fixture` from its first view on a page of `context`, on the stage `query` asks for,
 * under the audit: an answer at a time as its seat gave them, until a step that `ends` has
 * been judged. Returns every finding. */
async function audited(context, base, query, fixture, ends, watch) {
    const page = await context.newPage();
    const done = await watch?.(page);
    try {
        await page.goto(
            new URL(`/?${query}fixture=${fixture}&at=1#/game`, base).href,
        );
        await page.waitForFunction(
            () => window.berylStage && document.querySelector(".stage-node"),
        );
        await page.evaluate(() => document.fonts.ready.then(() => {}));
        const findings = [];
        for (let n = 0; n < ANSWERS && !findings.some(ends); n++) {
            const watched = await watching(page);
            await page.evaluate(() =>
                window.berylFixture.invoke("choose", { request: {} }),
            );
            findings.push(...(await watched()));
        }
        return findings;
    } finally {
        await done?.();
        await page.close();
    }
}

/** Holds the audit to `fixture`'s moment for `verb`, one of AUDITED: it passes the moment
 * whole, and reports the step a stage that skips one of `verb` did not play. `watch` is shown
 * the page that plays the moment whole (verbs.mjs), whose errors are the check's; the faulted
 * page's are the fault's. */
export async function auditsHold({
    context,
    base,
    query,
    fixture,
    verb,
    watch,
}) {
    const told = (f) => `${f.verdict} ${f.what}: ${f.why}`;
    const whole = await audited(
        context,
        base,
        query,
        fixture,
        AUDITED[verb],
        watch,
    );
    assert.deepEqual(
        // But for taps, minor as in `by specs --motion`: a land in a pile turns under its like.
        whole.filter((f) => f.verdict !== "ok" && !f.minor).map(told),
        [],
        "the audit of the moment",
    );
    assert.ok(
        whole.some(AUDITED[verb]),
        `the audit judged no ${verb} of the moment: ${whole.map(told).join("; ")}`,
    );
    const skipped = (f) => f.kind === verb && f.verdict === "still";
    const faulted = await audited(
        context,
        base,
        `${query}skip=${verb}&`,
        fixture,
        skipped,
    );
    const unplayed = faulted.filter(skipped).map((f) => f.why);
    assert.ok(
        unplayed.includes("the stage played no such step"),
        `the audit of a stage that skips a ${verb}: ${faulted.map(told).join("; ")}`,
    );
}
