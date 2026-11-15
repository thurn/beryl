import assert from "node:assert/strict";
import { test } from "node:test";
import { counterSlots } from "../src/table/cardState.ts";

test("creatures use stat counters and planeswalkers use loyalty first", () => {
    assert.deepEqual(counterSlots(["creature"], "a time counter"), [
        "+1/+1",
        "-1/-1",
        "counter",
    ]);
    assert.deepEqual(
        counterSlots(["planeswalker"], "a loyalty counter and a lore counter"),
        ["loyalty", "lore", "counter"],
    );
});

test("other permanents use the distinct named counters in printed order", () => {
    assert.deepEqual(
        counterSlots(
            ["artifact"],
            "a counter, two charge counters, a charge counter, then a stun counter.",
        ),
        ["charge", "stun", "counter"],
    );
    assert.deepEqual(
        counterSlots(["artifact"], "a −1/−1 counter and a lore counter"),
        ["-1/-1", "lore", "counter"],
    );
    assert.deepEqual(counterSlots(["land"]), ["charge", "time", "counter"]);
});
