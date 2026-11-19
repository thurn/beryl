// `testInputs` finds the data files a unit test reads through the modules it imports, so that
// Ignite never reuses a pass of a ui-unit check after one of them changed.
import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { testInputs } from "./test-inputs.mjs";

// A repository in a temporary directory, with `files` written relative to its root.
function repository(files) {
    const root = mkdtempSync(join(tmpdir(), "test-inputs-"));
    for (const [path, text] of Object.entries(files)) {
        mkdirSync(dirname(join(root, path)), { recursive: true });
        writeFileSync(join(root, path), text);
    }
    return root;
}

test("a JSON file imported two modules away is an input", () => {
    const root = repository({
        "ui/scripts/a.test.mjs": `import { b } from "../src/b.ts";\n`,
        "ui/src/b.ts": `export { c as b } from "./deep/c";\n`,
        "ui/src/deep/c.ts":
            `import x from "../../../data/x.json" with { type: "json" };\n` +
            `export const c = x;\n`,
        "data/x.json": "{}",
    });
    assert.deepEqual(
        testInputs(join(root, "ui/scripts/a.test.mjs"), join(root, "ui"), root),
        ["data/x.json"],
    );
});

test("string paths count in every module reached; ui and missing files do not", () => {
    const root = repository({
        "ui/scripts/a.test.mjs":
            `import "./helper.mjs";\n` +
            `const m = await import("./lazy.mjs");\n` +
            `const local = await import("./local.json");\n` +
            `import "./missing.mjs";\n`,
        "ui/scripts/helper.mjs": `read("data/y.json");\n`,
        "ui/scripts/lazy.mjs": `const p = "../../docs/z.md";\n`,
        "ui/scripts/local.json": "{}",
        "ui/scripts/unused.mjs": `read("data/never.json");\n`,
        "ui/scripts/missing.mjs": `read("data/gone.json");\n`,
        "data/y.json": "{}",
        "data/never.json": "{}",
        "docs/z.md": "",
    });
    assert.deepEqual(
        testInputs(join(root, "ui/scripts/a.test.mjs"), join(root, "ui"), root),
        ["data/y.json", "docs/z.md"],
    );
});

test("an import cycle ends", () => {
    const root = repository({
        "ui/scripts/a.test.mjs": `import "./b.mjs";\n`,
        "ui/scripts/b.mjs": `import "./a.test.mjs";\nimport "../../data/w.json";\n`,
        "data/w.json": "{}",
    });
    assert.deepEqual(
        testInputs(join(root, "ui/scripts/a.test.mjs"), join(root, "ui"), root),
        ["data/w.json"],
    );
});

test("the stage tests declare the conventions the stage imports", () => {
    const ui = join(dirname(fileURLToPath(import.meta.url)), "..");
    const inputs = testInputs(
        join(ui, "scripts", "stage.test.mjs"),
        ui,
        join(ui, ".."),
    );
    assert.ok(inputs.includes("data/ui-conventions.json"), inputs.join(", "));
});
