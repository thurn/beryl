import assert from "node:assert/strict";
import { test } from "node:test";
import { menuKey } from "../src/table/menuKeys.ts";

const childCounts = [0, 3, 0, 0];
const at = (row) => ({ active: { row } });
const inSubmenu = (child) => ({ active: { row: 1, child }, open: 1 });
const move = (selection) => ({ kind: "move", selection });

test("Up and Down traverse rows and wrap at the ends", () => {
    assert.deepEqual(menuKey("ArrowUp", at(0), childCounts), move(at(3)));
    assert.deepEqual(menuKey("ArrowDown", at(3), childCounts), move(at(0)));
    assert.deepEqual(menuKey("ArrowDown", at(1), childCounts), move(at(2)));
    assert.deepEqual(menuKey("Home", at(2), childCounts), move(at(0)));
    assert.deepEqual(menuKey("End", at(2), childCounts), move(at(3)));
});

test("Right and Return enter a submenu, and Left returns to its parent", () => {
    for (const key of ["ArrowRight", "Enter"]) {
        assert.deepEqual(menuKey(key, at(1), childCounts), move(inSubmenu(0)));
    }
    assert.deepEqual(
        menuKey("ArrowUp", inSubmenu(0), childCounts),
        move(inSubmenu(2)),
    );
    assert.deepEqual(
        menuKey("ArrowDown", inSubmenu(0), childCounts),
        move(inSubmenu(1)),
    );
    assert.deepEqual(
        menuKey("ArrowLeft", inSubmenu(1), childCounts),
        move(at(1)),
    );
    assert.equal(menuKey("ArrowRight", inSubmenu(1), childCounts), undefined);
});

test("Return chooses the focused command and Escape closes from either level", () => {
    assert.deepEqual(menuKey("Enter", at(0), childCounts), {
        kind: "choose",
        position: { row: 0 },
    });
    assert.deepEqual(menuKey("Enter", inSubmenu(1), childCounts), {
        kind: "choose",
        position: { row: 1, child: 1 },
    });
    for (const selection of [at(0), inSubmenu(0)]) {
        assert.deepEqual(menuKey("Escape", selection, childCounts), {
            kind: "close",
        });
    }
});
