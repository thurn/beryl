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
