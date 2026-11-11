// How a film reads a walk's ticks (film-walk.mjs `reel`), with no browser (`npm test`): each
// tick tells only what changed since the one before, and the reel reads where each node lay at
// any tick, so a moment that deals two whole libraries is handed back by what moves in it.
import assert from "node:assert/strict";
import { test } from "node:test";
import { reel } from "./film-walk.mjs";

const pose = (x) => ({ x, y: 0, width: 63, opacity: 1 });
const box = (x) => [x, 0, x + 63, 88];
/** A tick that tells the nodes in `drawn`, by name and place, and those `gone`. */
const tick = (at, drawn = {}, gone = []) => ({
    at,
    boxes: Object.fromEntries(
        Object.entries(drawn).map(([name, x]) => [name, box(x)]),
    ),
    poses: Object.fromEntries(
        Object.entries(drawn).map(([name, x]) => [name, pose(x)]),
    ),
    gone,
});

// A library card "7" that holds still throughout; "9" drawn from it, flying and then put away;
// an ability "a3" that comes at the third tick.
const ticks = [
    tick(0, { 7: 10, 9: 10 }),
    tick(17, { 9: 40 }),
    tick(33, { 9: 80, a3: 200 }),
    tick(50),
    tick(67, {}, ["9"]),
];

test("a node reads as it was last told, until it is gone", () => {
    const film = reel(ticks);
    assert.deepEqual(film.box("7", 4), box(10));
    assert.deepEqual(film.pose("9", 1), pose(40));
    assert.deepEqual(film.pose("9", 3), pose(80));
    assert.equal(film.pose("9", 4), null);
    assert.equal(film.box("a3", 1), null);
    assert.deepEqual(film.box("a3", 2), box(200));
    assert.equal(film.box("nothing", 2), null);
});

test("a node's changes are the ticks that tell it", () => {
    const film = reel(ticks);
    assert.deepEqual(film.names.sort(), ["7", "9", "a3"]);
    assert.deepEqual(film.changes("7"), [0]);
    assert.deepEqual(film.changes("9"), [0, 1, 2, 4]);
    assert.deepEqual(film.changes("nothing"), []);
});
