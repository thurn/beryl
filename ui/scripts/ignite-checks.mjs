// The UI checks that need no browser, in the form Ignite runs them (docs/ignite.md, "Declaring a
// family" and "Reporting results"):
//
//   node ui/scripts/ignite-checks.mjs <family> --list
//   node ui/scripts/ignite-checks.mjs <family> [check ...]
//
// `--list` prints one JSON line per check: {"check": "<family>:<name>", "inputs": [paths]}.
// `inputs` holds only what the check reads beyond the family's own inputs. A run executes the
// checks named (all of the family's when none is named; a name may omit the "<family>:" prefix)
// and appends one JSON line per check to the file named by IGNITE_RESULTS, or prints it when that
// is unset: {"check": ..., "outcome": "pass" | "fail", "message": ...}. The commands run in `ui/`
// and need `ui/node_modules`, which the `ui-deps` build product provides.
//
// Families: `ui-format` (prettier), `ui-types` (tsc) and `ui-unit` (one check per
// `scripts/*.test.mjs`, which `npm test` runs together).

import { spawnSync } from "node:child_process";
import { appendFileSync, readdirSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const ui = join(dirname(fileURLToPath(import.meta.url)), "..");
const bin = (name) => join(ui, "node_modules", ".bin", name);

// Files a unit test reads beyond the family's inputs (`ui/**`), found from its source.
function extraInputs(file) {
    const source = readFileSync(join(ui, "scripts", file), "utf8");
    const paths = [...source.matchAll(/"(?:\.\.\/)+((?:docs|data)\/[^"]+)"/g)];
    return [...paths, ...source.matchAll(/read\("(data\/[^"]+)"\)/g)].map(
        (m) => m[1],
    );
}

function unitFiles() {
    return readdirSync(join(ui, "scripts"))
        .filter((name) => name.endsWith(".test.mjs"))
        .sort();
}

const families = {
    "ui-format": {
        checks: () => [{ name: "prettier", inputs: [] }],
        command: () => [bin("prettier"), ["--check", "."]],
    },
    "ui-types": {
        checks: () => [{ name: "tsc", inputs: [] }],
        command: () => [bin("tsc"), ["-b"]],
    },
    "ui-unit": {
        checks: () =>
            unitFiles().map((file) => ({
                name: file.replace(/\.test\.mjs$/, ""),
                inputs: extraInputs(file),
            })),
        command: (name) => [
            process.execPath,
            ["--test", join("scripts", `${name}.test.mjs`)],
        ],
    },
};

const [family, ...rest] = process.argv.slice(2);
const spec = families[family];
if (!spec) {
    console.error(
        `usage: ignite-checks.mjs <${Object.keys(families).join("|")}> --list | [check ...]`,
    );
    process.exit(1);
}
const all = spec.checks();

if (rest[0] === "--list") {
    for (const { name, inputs } of all) {
        console.log(JSON.stringify({ check: `${family}:${name}`, inputs }));
    }
    process.exit(0);
}

const wanted = rest.length
    ? rest.map((name) => name.replace(`${family}:`, ""))
    : all.map((c) => c.name);
const unknown = wanted.filter((name) => !all.some((c) => c.name === name));
if (unknown.length) {
    console.error(`${family} has no check named ${unknown.join(", ")}`);
    process.exit(1);
}

let failed = false;
for (const name of wanted) {
    const [program, args] = spec.command(name);
    const run = spawnSync(program, args, { cwd: ui, encoding: "utf8" });
    const pass = run.status === 0;
    failed ||= !pass;
    const output =
        `${run.stdout ?? ""}${run.stderr ?? ""}${run.error ?? ""}`.trim();
    const line = JSON.stringify({
        check: `${family}:${name}`,
        outcome: pass ? "pass" : "fail",
        message: pass ? "" : output.split("\n").slice(-20).join("\n"),
    });
    if (process.env.IGNITE_RESULTS) {
        appendFileSync(process.env.IGNITE_RESULTS, `${line}\n`);
    } else {
        console.log(line);
    }
}
process.exit(failed ? 1 : 0);
