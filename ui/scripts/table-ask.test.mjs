import assert from "node:assert/strict";
import { test } from "node:test";
import { clamp, parse, parseStats } from "../src/table/ask.ts";

test("digits replace the starting value and signed entries change the current value", () => {
    assert.equal(parse("3"), 3);
    assert.equal(parse("3", 20), 3);
    assert.equal(parse("+3", 20), 23);
    assert.equal(parse("-5", 20), 15);
    assert.equal(parse("−5", 20), 15);
    assert.equal(parse("+3", 0), 3);
    assert.equal(parse("-5"), -5);
    assert.equal(parse(" 0 ", 20), 0);
});

test("only safe, whole numbers can be submitted", () => {
    for (const entry of [
        "",
        "+",
        "-",
        "−",
        "1.5",
        "1e3",
        "1 2",
        "abc",
        "3/3",
    ]) {
        assert.equal(parse(entry, 20), undefined, entry);
    }
    assert.equal(parse("9007199254740992"), undefined);
    assert.equal(parse("+1", Number.MAX_SAFE_INTEGER), undefined);
});

test("power and toughness can each be replaced or changed", () => {
    const current = { power: 2, toughness: 5 };
    assert.deepEqual(parseStats("3/3", current), { power: 3, toughness: 3 });
    assert.deepEqual(parseStats("+1/+1", current), { power: 3, toughness: 6 });
    assert.deepEqual(parseStats("−1 / -2", current), {
        power: 1,
        toughness: 3,
    });
    assert.deepEqual(parseStats("+1/−1", current), { power: 3, toughness: 4 });
    assert.deepEqual(parseStats("-1/+1", current), { power: 1, toughness: 6 });
    assert.deepEqual(parseStats("0/+1", current), { power: 0, toughness: 6 });
    for (const entry of ["3", "3/", "/3", "3/3/3", "+1/false"]) {
        assert.equal(parseStats(entry, current), undefined, entry);
    }
});

test("bounds apply to the result of a replacement or a change", () => {
    assert.equal(clamp(parse("0", 20), 1, 25), 1);
    assert.equal(clamp(parse("+10", 20), 1, 25), 25);
    assert.equal(clamp(parse("-10", 20), 1, 25), 10);
});
