// Which touches open the bug report, with no browser (`npm test`; ui/src/mobile/report-taps.ts):
// a double tap with one finger on bare felt, or with two or more fingers anywhere.
import assert from "node:assert/strict";
import { test } from "node:test";
import { ReportTaps } from "../src/mobile/report-taps.ts";

let nextId = 1;

/** A tap with `fingers` fingers landing together at `at`, held 60 ms. Returns whether it
 * finished a double tap. */
function tap(taps, at, { fingers = 1, felt = false, x = 100, y = 100 } = {}) {
    const ids = [];
    for (let i = 0; i < fingers; i++) {
        const id = nextId++;
        ids.push(id);
        taps.down(id, x + i * 40, y, at + i * 10, felt);
    }
    if (fingers > 1) assert.equal(taps.multi, true);
    let done = false;
    for (const id of ids) done = taps.up(id, at + 60) || done;
    return done;
}

test("one finger double tapped on bare felt opens the report", () => {
    const taps = new ReportTaps();
    assert.equal(tap(taps, 0, { felt: true }), false);
    assert.equal(tap(taps, 200, { felt: true }), true);
});

test("one finger double tapped on a card does not", () => {
    const taps = new ReportTaps();
    tap(taps, 0);
    assert.equal(tap(taps, 200), false);
});

test("two or three fingers double tapped anywhere open the report", () => {
    for (const fingers of [2, 3]) {
        const taps = new ReportTaps();
        assert.equal(tap(taps, 0, { fingers }), false);
        assert.equal(tap(taps, 200, { fingers }), true);
    }
});

test("the two taps can use different numbers of fingers", () => {
    const taps = new ReportTaps();
    tap(taps, 0, { felt: true });
    assert.equal(tap(taps, 200, { fingers: 2 }), true);
});

test("taps too far apart in time or space are two single taps", () => {
    const slow = new ReportTaps();
    tap(slow, 0, { fingers: 2 });
    assert.equal(tap(slow, 500, { fingers: 2 }), false);
    const apart = new ReportTaps();
    tap(apart, 0, { fingers: 2 });
    assert.equal(tap(apart, 200, { fingers: 2, x: 300 }), false);
});

test("a second finger landing late is another thumb, not a multi-finger tap", () => {
    const taps = new ReportTaps();
    taps.down(1, 100, 100, 0, false);
    taps.down(2, 300, 100, 200, false);
    assert.equal(taps.multi, false);
    taps.up(1, 250);
    taps.up(2, 260);
    assert.equal(tap(taps, 400, { fingers: 2 }), false);
});

test("a touch that moves, or is held, is not a tap", () => {
    const moved = new ReportTaps();
    tap(moved, 0, { fingers: 2 });
    moved.down(10, 100, 100, 200, false);
    moved.down(11, 140, 100, 210, false);
    moved.move(10, 100, 180);
    moved.up(10, 260);
    assert.equal(moved.up(11, 260), false);
    const held = new ReportTaps();
    tap(held, 0, { fingers: 2 });
    held.down(20, 100, 100, 200, false);
    held.down(21, 140, 100, 210, false);
    held.up(20, 700);
    assert.equal(held.up(21, 700), false);
});
