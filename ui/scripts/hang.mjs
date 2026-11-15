// A hang guard for one browser check. A page that never answers (a call into a page whose main
// thread never yields) has no Playwright timeout to end it: `page.evaluate` and the fake clock's
// `runFor` wait forever. One such check once held its worker until ui-test's 600 s deadline, and
// the checks queued behind it never ran (by-b8wh). Failing the check by name instead lets the
// step's rerun narrow to it (tools/src/beryl_tools/checks.py) and the run log record it.
import { availableParallelism, loadavg } from "node:os";

/** How long one check may take, in load-adjusted seconds. The slowest that ever passed took 17:
 * the guard is for hangs, not for a slow check. */
export const CHECK_SECONDS = 60;

/** A check that ran past its guard. */
export class HangError extends Error {}

/** How much slower than a quiet machine's this one's clock runs now: the load average over the
 * core count, from 1 up to 8, as `by check` and `by ci` count their budgets (AGENTS.md). */
function slowdown() {
    return Math.min(Math.max(loadavg()[0] / availableParallelism(), 1), 8);
}

/** Settles as `work` does, or rejects with a `HangError` naming `what` once `seconds`
 * load-adjusted seconds pass first. `work` runs on after that; the caller ends it by closing
 * its browser context. */
export function hangGuard(work, seconds, what) {
    let timer;
    const hung = new Promise((_, reject) => {
        let spent = 0;
        const step = 1000;
        timer = setInterval(() => {
            spent += step / 1000 / slowdown();
            if (spent >= seconds) {
                reject(
                    new HangError(
                        `${what}: hung for ${seconds} s (load-adjusted); its page stopped answering`,
                    ),
                );
            }
        }, step);
    });
    return Promise.race([work, hung]).finally(() => clearInterval(timer));
}

/** How long, in real milliseconds, one Playwright wait (`page.goto`, `waitForFunction`,
 * `click` and the rest) may take before it fails, when the call sets no timeout of its own.
 * Playwright's default is 30 s, which only catches a hang on a machine that gives the check
 * its share of the CPU. Work started at low priority (`system.utility` in tools/, which the
 * browser sweep, the nightly and their bisections use) gets only the CPU that agents' checks
 * leave, and at a load of 50 to 70 a shot that takes 2 s at normal priority ran past 30 s.
 * Those runs set `BERYL_LOW_PRIORITY=1`, and their waits get six times as long, so that a
 * wait fails only when the page is stuck. A check at normal priority keeps 30 s, so a real
 * break still fails fast in the fast check. */
export const WAIT_MS = process.env.BERYL_LOW_PRIORITY === "1" ? 180000 : 30000;

/** A new browser context for a check, from `browser.newContext(options)`, whose waits time
 * out after `WAIT_MS`. Every check opens its contexts here (tools/tests/test_browser_waits.py
 * holds them to it), so no wait falls back to Playwright's own 30 s. */
export async function hangContext(browser, options) {
    const context = await browser.newContext(options);
    context.setDefaultTimeout(WAIT_MS);
    return context;
}
