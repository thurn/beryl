// The files a unit test in `ui/scripts` reads beyond `ui/**`, which is its family's own input
// (ignite/checks/ui.toml, `ui-unit`). Ignite reuses a check's last pass while none of its inputs
// has changed, so a file the test reads but does not declare can change under a reused pass.
//
// A test reads a data file in one of three ways, and `testInputs` finds all three:
//
// - It imports a module that imports a JSON file, perhaps several modules deep, as
//   `src/stage/timing.ts` imports `data/ui-conventions.json`. The walk follows every relative
//   static import, `export ... from` and literal dynamic `import()` from the test, as the pytest
//   adapter follows a Python test's imports (tools' `beryl_tools.ignite.python`, `test_inputs`).
// - It names the path as a string literal starting with `../`, such as `"../../data/x.json"`.
// - It calls a `read("data/...")` helper with a path from the repository root.
//
// The two string forms are looked for in every module the walk reaches, not only the test.

import { existsSync, readFileSync, statSync } from "node:fs";
import { dirname, join, relative, resolve, sep } from "node:path";

// Modules whose source the walk reads and follows further. Any other file it reaches (JSON,
// CSS, a text fixture) is only recorded.
const SOURCE = /\.(?:mjs|cjs|js|jsx|ts|tsx|mts|cts)$/;

// Specifiers of `import ... from "x"`, `export ... from "x"`, `import "x"` and `import("x")`.
const IMPORT = /(?:\bfrom\s*|\bimport\s*\(\s*|^\s*import\s*)["']([^"']+)["']/gm;

// The data paths a module names as strings, relative (`"../../data/x"`) or from the root
// (`read("data/x")`). A relative string may be joined to some other directory than the module's
// own, so it is taken to name the repository's `docs/` or `data/` however many `../` it starts
// with.
const RELATIVE_PATH = /"(?:\.\.\/)+((?:docs|data)\/[^"]+)"/g;
const ROOT_PATH = /read\("((?:docs|data)\/[^"]+)"\)/g;

// The file a relative specifier names, trying the extensions a bare `./x` may leave off.
function resolveSpecifier(from, specifier) {
    const base = resolve(dirname(from), specifier.split(/[?#]/)[0]);
    for (const candidate of [
        base,
        `${base}.ts`,
        `${base}.tsx`,
        `${base}.mjs`,
        `${base}.js`,
    ]) {
        if (existsSync(candidate) && statSync(candidate).isFile()) {
            return candidate;
        }
    }
    return null;
}

// Every path, relative to `root` and with `/` separators, that the test at `testFile` reads
// outside the directory `ui`, sorted. `ui` and `root` are absolute.
export function testInputs(testFile, ui, root) {
    const seen = new Set();
    const outside = new Set();
    const pending = [resolve(testFile)];
    const record = (file) => {
        // A path that names no file is a string the walk mistook for one, such as an example in a
        // comment or a fixture a test writes to a temporary directory.
        if (relative(ui, file).startsWith("..") && existsSync(file)) {
            outside.add(relative(root, file).split(sep).join("/"));
        }
    };
    while (pending.length) {
        const file = pending.pop();
        if (seen.has(file)) {
            continue;
        }
        seen.add(file);
        record(file);
        if (!SOURCE.test(file)) {
            continue;
        }
        const source = readFileSync(file, "utf8");
        for (const [, specifier] of source.matchAll(IMPORT)) {
            if (specifier.startsWith(".")) {
                const target = resolveSpecifier(file, specifier);
                if (target) {
                    pending.push(target);
                }
            }
        }
        for (const pattern of [RELATIVE_PATH, ROOT_PATH]) {
            for (const [, path] of source.matchAll(pattern)) {
                record(join(root, path));
            }
        }
    }
    return [...outside].sort();
}
