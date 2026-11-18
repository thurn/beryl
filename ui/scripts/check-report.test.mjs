import assert from "node:assert/strict";
import { mkdtempSync, readFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";

import { failureOf, pageError, recordResult, sourcePlace } from "./capture.mjs";

// A page error as WebKit reports it from a module the dev server served, and an assertion
// thrown by a check script.
const PAGE_STACK =
    "layOut@http://127.0.0.1:51734/src/stage/phone.ts?t=1731900000:212:17\n" +
    "render@http://127.0.0.1:51734/node_modules/.vite/deps/react-dom.js:9:3";
const SCRIPT_STACK =
    "AssertionError: the hand rises\n" +
    "    at assert (node:assert:400:5)\n" +
    "    at body (file:///Users/me/beryl/ui/scripts/kept-table.mjs:88:9)";

test("a place in the UI's source is read from a page's stack and a script's", () => {
    assert.equal(sourcePlace(PAGE_STACK), "ui/src/stage/phone.ts:212");
    assert.equal(sourcePlace(SCRIPT_STACK), "ui/scripts/kept-table.mjs:88");
    assert.equal(sourcePlace("no place here"), null);
});

test("a page error fails its check with the file and line it was thrown from", () => {
    const error = new TypeError("cannot read properties of undefined");
    error.stack = PAGE_STACK;
    const seen = [`hand: ${pageError(error)}`];
    assert.deepEqual(failureOf(seen, null), {
        message:
            "hand: cannot read properties of undefined (at ui/src/stage/phone.ts:212)",
        at: "ui/src/stage/phone.ts:212",
    });

    const results = join(mkdtempSync(join(tmpdir(), "ignite-")), "r.jsonl");
    process.env.IGNITE_RESULTS = results;
    try {
        recordResult("ui-test:hand", "fail", 1.5, null, failureOf(seen, null));
        recordResult("ui-test:other", "pass", 1, null, failureOf(seen, null));
    } finally {
        delete process.env.IGNITE_RESULTS;
    }
    const lines = readFileSync(results, "utf8")
        .trim()
        .split("\n")
        .map((l) => JSON.parse(l));
    assert.deepEqual(lines, [
        {
            check: "ui-test:hand",
            outcome: "fail",
            message:
                "hand: cannot read properties of undefined (at ui/src/stage/phone.ts:212)",
            at: "ui/src/stage/phone.ts:212",
        },
        { check: "ui-test:other", outcome: "pass" },
    ]);
});

test("a thrown assertion fails its check at the script's line", () => {
    const error = new Error("the hand rises");
    error.stack = SCRIPT_STACK;
    assert.deepEqual(failureOf([], error), {
        message: "AssertionError: the hand rises",
        at: "ui/scripts/kept-table.mjs:88",
    });
    assert.deepEqual(failureOf([], null), {});
});
