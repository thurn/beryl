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

function releaseSlot(file) {
    try {
        unlinkSync(file);
    } catch {
        // Reclaimed as stale already.
    }
}

/** Take every slot a task needs before it opens a page. Give up a partial
 * reservation before waiting so two multi-page checks cannot block each other.
 * With no slot directory configured, nothing is reserved. */
export async function takeSlot(env = process.env, pages = 1) {
    const dir = env.BERYL_BROWSER_SLOTS_DIR;
    if (!dir) return () => {};
    mkdirSync(dir, { recursive: true });
    const { total, usable } = slotCounts(env);
    if (pages < 1 || pages > total || !Number.isInteger(pages))
        throw new RangeError(
            `Cannot reserve ${pages} of ${total} browser slots`,
        );
    // A single paired check needs two slots even on a small development host.
    const limit = Math.max(usable, pages);
    for (;;) {
        const files = [];
        for (let i = 0; i < limit; i++) {
            const file = join(dir, `slot-${i}`);
            try {
                const fd = openSync(file, "wx");
                writeSync(fd, String(process.pid));
                closeSync(fd);
                files.push(file);
                if (files.length === pages)
                    return () => files.forEach(releaseSlot);
            } catch (e) {
                if (e.code !== "EEXIST") throw e;
                if (stale(file)) releaseSlot(file);
            }
        }
        files.forEach(releaseSlot);
        await new Promise((r) => setTimeout(r, 100 + Math.random() * 150));
    }
}
