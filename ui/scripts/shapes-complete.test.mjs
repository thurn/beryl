// The list of shapes is complete (`npm test`, no browser): every prompt shape the session
// can send (`ShapeName`, src/generated/protocol.ts) has a recording in the shape suite
// (ui/shapes.json) from the seat that is asked and from the seat across the table, which
// shapes.mjs checks in both layouts (`shape:<shape>/<seat>/<layout>`). A new shape, or one
// whose recording is taken away, fails here.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

const read = (path) =>
    readFileSync(new URL(`../${path}`, import.meta.url), "utf8");
const suite = JSON.parse(read("shapes.json"));
const fixtures = JSON.parse(read("fixtures.json"));

test("every shape the session sends has its four checks", () => {
    const [, names] =
        /^export type ShapeName = (.*);$/m.exec(
            read("src/generated/protocol.ts"),
        ) ?? [];
    assert.ok(names, "src/generated/protocol.ts declares ShapeName");
    const shapes = [...names.matchAll(/"(\w+)"/g)].map(([, name]) => name);
    assert.ok(shapes.length > 0);
    assert.deepEqual([...suite.layouts].sort(), ["desktop", "phone"]);
    for (const shape of shapes) {
        assert.deepEqual(
            Object.keys(suite.shapes[shape] ?? {}).sort(),
            ["theirs", "yours"],
            `${shape} in ui/shapes.json`,
        );
    }
});

test("every recording the shape suite names is a fixture", () => {
    const known = new Set(fixtures.fixtures.map((fixture) => fixture.name));
    for (const [shape, seats] of Object.entries(suite.shapes)) {
        for (const [seat, fixture] of Object.entries(seats)) {
            assert.ok(known.has(fixture), `${shape}/${seat}: ${fixture}`);
        }
    }
});
