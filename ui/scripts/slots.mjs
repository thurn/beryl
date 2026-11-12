// The machine's browser pages, shared by every capture run on it. Headless WebKit's page and GPU
// processes are started by launchd at an interactive priority that no clamp on the run reaches
// (`taskpolicy -c utility` lowers Playwright itself, not them), so three or four runs at once
// took most of the cores from every build and check on the machine. A page now holds one of a
// fixed number of slots, files in `BERYL_BROWSER_SLOTS_DIR` (shots.browser_env sets it) that
// name the process holding them; a run waits for a free one.
import {
    closeSync,
    mkdirSync,
    openSync,
    readFileSync,
    statSync,
    unlinkSync,
    writeSync,
} from "node:fs";
import { availableParallelism } from "node:os";
import { join } from "node:path";

/** How many pages the machine runs at once (`BERYL_BROWSER_SLOTS`, else half the cores: a
 * WebKit page keeps about a core busy), and how many of them a run may take: a check (`by check`
 * or `by ci`, which set `BERYL_CHECK_RESULTS`) all of them, and any other run (an agent's own
 * `just specs --motion`, say) a third, shared with every other such run, so that what a push
 * or slow CI waits on is not queued behind it. Slow CI alone runs about ten pages at once. */
export function slotCounts(env = process.env, cores = availableParallelism()) {
    const total = Math.max(
        3,
        Number(env.BERYL_BROWSER_SLOTS) || Math.floor(cores / 2),
    );
    const usable = env.BERYL_CHECK_RESULTS
        ? total
        : Math.max(1, Math.floor(total / 3));
    return { total, usable };
}

/** Whether the slot file's holder is gone: its process exited (a step killed at the deadline
 * leaves its slots behind), or it never wrote its pid and the file is older than a few seconds. */
function stale(file) {
    let text;
    try {
        text = readFileSync(file, "utf8").trim();
    } catch {
        return false; // Released meanwhile: the next pass takes it.
    }
    const pid = Number(text);
    if (!pid) {
        try {
            return Date.now() - statSync(file).mtimeMs > 10_000;
        } catch {
            return false;
        }
    }
    try {
        process.kill(pid, 0);
        return false;
    } catch (e) {
        return e.code === "ESRCH";
    }
}

/** Takes a free slot, waiting while there is none, and resolves to the function that releases
 * it; with no `BERYL_BROWSER_SLOTS_DIR` (a script run by hand), takes nothing. Two runs that
 * reclaim the same stale slot at once can let one more page run than the count: harmless, and
 * gone when either releases it. */
export async function takeSlot(env = process.env) {
    const dir = env.BERYL_BROWSER_SLOTS_DIR;
    if (!dir) return () => {};
    mkdirSync(dir, { recursive: true });
    const { usable } = slotCounts(env);
    for (;;) {
        for (let i = 0; i < usable; i++) {
            const file = join(dir, `slot-${i}`);
            try {
                const fd = openSync(file, "wx");
                writeSync(fd, String(process.pid));
                closeSync(fd);
                return () => {
                    try {
                        unlinkSync(file);
                    } catch {
                        // Reclaimed as stale already.
                    }
                };
            } catch (e) {
                if (e.code !== "EEXIST") throw e;
                if (stale(file)) {
                    try {
                        unlinkSync(file);
                    } catch {
                        // Another run reclaimed it first.
                    }
                }
            }
        }
        await new Promise((r) => setTimeout(r, 100 + Math.random() * 150));
    }
}
