// The pages the verb suite (verbs.mjs) reads, each opened once however many checks read it:
// every verb's check of a seat and a layout watches the same moments at the same windows and
// ticks, so the moment is played once and each check holds its own cases to the run.
import assert from "node:assert/strict";
import { coverage } from "./capture.mjs";

/** Pages opened on `queue`'s workers, one context of windows of each size from `open` shared
 * by every page at that size. `once(key, first, task)` queues `task` (given a `watch` to show
 * its page first) once under `key`, which is to say by all it depends on, and returns what it
 * holds: what the page gave (`given`, its result or what it threw, once `done`), the errors
 * it logged, what it ran if `first` (some check opens it first: `coverage`) and its seconds.
 * Every check that reads it reads those. */
export function sharedPages(queue, open) {
    const contexts = new Map();
    const pages = new Map();
    const contextOf = (size) => {
        const at = `${size.width}x${size.height}`;
        if (!contexts.has(at)) contexts.set(at, open(size));
        return contexts.get(at);
    };
    const once = (key, first, task) => {
        if (pages.has(key)) {
            const held = pages.get(key);
            // What a page ran is taken as it opens, so a later ask must come before it does.
            assert.ok(
                !first || held.first,
                `${key} is first for a later check`,
            );
            return held;
        }
        const held = { first, errors: [], covered: null, seconds: 0 };
        const watch = async (page) => {
            page.on("pageerror", (e) => held.errors.push(e.message));
            page.on(
                "console",
                (m) => m.type() === "error" && held.errors.push(m.text()),
            );
            if (!first) return async () => {};
            const finish = await coverage(page);
            return async () => {
                held.covered = await finish();
            };
        };
        held.done = new Promise((resolve) =>
            queue(async () => {
                const started = performance.now();
                try {
                    held.result = await task(watch);
                } catch (e) {
                    held.error = e;
                }
                held.seconds = (performance.now() - started) / 1000;
                resolve();
            }),
        );
        pages.set(key, held);
        return held;
    };
    return {
        once,
        contextOf,
        /** Waits for every page queued. */
        settled: () =>
            Promise.all([...pages.values()].map((held) => held.done)),
        /** Closes every context. */
        close: async () => {
            for (const context of contexts.values())
                await (await context).close();
        },
    };
}

/** What a page gave, or what it threw. */
export function given(held) {
    if (held.error) throw held.error;
    return held.result;
}
