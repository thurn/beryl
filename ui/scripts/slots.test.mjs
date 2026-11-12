import assert from "node:assert/strict";
import { mkdtempSync, readdirSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";

import { slotCounts, takeSlot } from "./slots.mjs";

test("a run that is no check takes a third of the slots", () => {
    assert.deepEqual(slotCounts({}, 18), { total: 9, usable: 3 });
    assert.deepEqual(slotCounts({ BERYL_CHECK_RESULTS: "x" }, 18), {
        total: 9,
        usable: 9,
    });
    assert.deepEqual(slotCounts({ BERYL_BROWSER_SLOTS: "3" }, 18), {
        total: 3,
        usable: 1,
    });
});

test("pages wait for a free slot and take it once released", async () => {
    const dir = mkdtempSync(join(tmpdir(), "slots-"));
    const env = { BERYL_BROWSER_SLOTS_DIR: dir, BERYL_BROWSER_SLOTS: "3" };
    const release = await takeSlot(env);
    let second = false;
    const waiting = takeSlot(env).then((r) => ((second = true), r));
    await new Promise((r) => setTimeout(r, 400));
    assert.equal(second, false);
    release();
    (await waiting)();
    assert.deepEqual(readdirSync(dir), []);
});

test("a slot whose holder exited is reclaimed", async () => {
    const dir = mkdtempSync(join(tmpdir(), "slots-"));
    // No process has pid 2^22 - 1 on macOS or Linux.
    writeFileSync(join(dir, "slot-0"), "4194303");
    const release = await takeSlot({
        BERYL_BROWSER_SLOTS_DIR: dir,
        BERYL_BROWSER_SLOTS: "3",
    });
    release();
});

test("with no slot directory nothing is taken", async () => {
    (await takeSlot({}))();
});
