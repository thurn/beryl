import assert from "node:assert/strict";
import { test } from "node:test";
import { match } from "../src/table/filter.ts";

const commands = [
    { label: "Stop checking cards" },
    { label: "Look at top cards…" },
    { label: "Top-card preview" },
    { label: "Tap" },
    { label: "Desktop cards" },
];

test("an empty or whitespace-only query keeps every command in input order", () => {
    assert.deepEqual(match("", commands), commands);
    assert.deepEqual(match(" \t ", commands), commands);
});

test("all words must match the label, regardless of case, order or spacing", () => {
    assert.deepEqual(match("  CARD\tLOOK ", commands), [commands[1]]);
    assert.deepEqual(match("top exile", commands), []);
});

test("word prefixes rank above interior matches and keep input order at each rank", () => {
    assert.deepEqual(match("top", commands), [
        commands[1],
        commands[2],
        commands[0],
        commands[4],
    ]);
    assert.deepEqual(match("top pre", commands), [commands[2]]);
});
