// The list of verbs is complete (`npm test`, no browser): every verb of the design language
// (data/ui-conventions.json) has values to be played with or the exception that exempts it,
// a track on the stage that the verb suite checks from both seats in both layouts
// (ui/verbs.json, verbs.mjs: `verb:<verb>/<seat>/<layout>`), and a script written by hand that
// holds what the session builds for it (beryl_session's tests) or, for a verb no builder
// writes, that its check plays. A verb the stage does not play yet says so, with the bead that
// will play it. A new verb, or one whose case, snapshot or check is taken away, fails here.
import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
import { test } from "node:test";
import { CASES as ZONES } from "./verb-cases.mjs";
import { CASES as PILES } from "./verb-cases-piles.mjs";
import { CASES as PLACE } from "./verb-cases-place.mjs";
import { CASES as REST } from "./verb-cases-rest.mjs";
import { CASES as SPANS } from "./verb-cases-spans.mjs";

const repo = new URL("../../", import.meta.url);
const read = (path) => readFileSync(new URL(path, repo), "utf8");
const data = JSON.parse(read("data/ui-conventions.json"));
const suite = JSON.parse(read("ui/verbs.json"));
const names = data.verbs.map((verb) => verb.name);

/** The tests of the scripts the session builds, each a hand-written expectation whose doc
 * comment names the verbs it holds. */
const SNAPSHOTS = [
    "rust/crates/beryl_session/tests/script_build.rs",
    "rust/crates/beryl_session/tests/script_spans.rs",
    "rust/crates/beryl_session/tests/script_piles.rs",
];

/** The verbs the doc comment of some test of `source` names, in backticks. */
function snapshotted(source) {
    const found = new Set();
    for (const [, comment] of source.matchAll(
        /((?:^\/\/\/.*\n)+)#\[test\]/gm,
    )) {
        for (const [, word] of comment.matchAll(/`([a-z]+)`/g)) found.add(word);
    }
    return found;
}

test("the verb suite's lists name the design language's verbs, each once", () => {
    const pending = Object.keys(suite.pending);
    assert.deepEqual(
        [...suite.verbs, ...pending].sort(),
        [...names].sort(),
        "ui/verbs.json's verbs and pending against data/ui-conventions.json",
    );
    for (const [verb, bead] of Object.entries(suite.pending)) {
        assert.match(bead, /^by-[a-z0-9.]+$/, `${verb} is pending on a bead`);
    }
});

test("every verb has values to play it with, or an exemption", () => {
    for (const verb of data.verbs) {
        const timed = Number.isInteger(verb.values?.duration);
        assert.ok(
            timed || Number.isInteger(verb.exempt),
            `${verb.name} has no duration and no exemption`,
        );
    }
});

test("every verb the stage plays has a case, and its four checks", () => {
    const cases = [ZONES, PLACE, PILES, SPANS, REST];
    for (const verb of suite.verbs) {
        assert.ok(
            cases.some((own) => typeof own[verb] === "function"),
            `${verb} has no case in the verb suite`,
        );
    }
    // verbs.mjs makes one check a verb, seat and layout from these lists.
    assert.deepEqual(Object.keys(suite.moment).sort(), ["theirs", "yours"]);
    assert.deepEqual([...suite.layouts].sort(), ["desktop", "phone"]);
    for (const [verb, moments] of Object.entries(suite.watches)) {
        assert.ok(suite.verbs.includes(verb), `${verb} watches and is no verb`);
        for (const moment of moments) {
            const seats = Object.keys(suite.more[moment] ?? {}).sort();
            assert.deepEqual(seats, ["theirs", "yours"], `${verb}: ${moment}`);
        }
    }
});

test("every verb the stage plays has a script written by hand", () => {
    const built = new Set(
        SNAPSHOTS.flatMap((path) => [...snapshotted(read(path))]),
    );
    for (const verb of suite.verbs) {
        const written = suite.written[verb];
        assert.ok(
            built.has(verb) || (written && existsSync(new URL(written, repo))),
            `${verb} has no hand-written script among ${SNAPSHOTS.join(", ")}`,
        );
    }
    for (const verb of Object.keys(suite.written)) {
        assert.ok(!built.has(verb), `${verb} is built, and needs no written`);
    }
});
