import assert from "node:assert/strict";
import { readFileSync, readdirSync } from "node:fs";
import test from "node:test";

const read = (path) => readFileSync(new URL(path, import.meta.url), "utf8");
const commands = JSON.parse(read("../../data/table-commands.json"));
const mappings = JSON.parse(read("../../data/table-appendix-map.json"));
const appendix = read("../../docs/manual-multiplayer-cockatrice.md");
const registered = read("./kept.mjs");

function rows(markdown) {
    const result = [];
    let headers;
    for (const line of markdown.split("\n")) {
        if (!line.startsWith("|")) {
            headers = undefined;
            continue;
        }
        const cells = line
            .split(/(?<!\\)\|/u)
            .slice(1, -1)
            .map((cell) => cell.trim().replaceAll("\\|", "|"));
        if (!headers) {
            headers = cells;
            continue;
        }
        if (cells.every((cell) => /^:?-+:?$/u.test(cell))) continue;
        assert.equal(cells.length, headers.length, line);
        const label = headers[0] === "Zone" ? cells[1] : cells[0];
        const category =
            headers[0] === "Zone"
                ? cells[0]
                : headers[0] === "Cockatrice command"
                  ? "Protocol"
                  : "";
        const key = category ? `${category} / ${label}` : label;
        result.push({
            key,
            label,
            excluded: cells.at(-1).startsWith("Not carried over"),
        });
    }
    return result;
}

test("the Cockatrice appendix accounts for every carried-over row", () => {
    const ids = new Set(commands.map(({ id }) => id));
    const checks = new Set(
        readdirSync(new URL(".", import.meta.url))
            .filter(
                (name) =>
                    /^kept-table-.*\.mjs$/u.test(name) &&
                    registered.includes(`import("./${name}")`),
            )
            .flatMap((name) =>
                [...read(`./${name}`).matchAll(/check\(\s*"([^"]+)"/gu)].map(
                    ([, title]) => title,
                ),
            ),
    );
    const seen = new Set();
    for (const { key, label, excluded } of rows(appendix)) {
        assert.ok(!seen.has(key), `Duplicate appendix row: ${key}`);
        seen.add(key);
        if (excluded) {
            assert.ok(
                !Object.hasOwn(mappings, key),
                `Excluded row mapped: ${key}`,
            );
            for (const { label: command } of commands) {
                assert.notEqual(
                    command.toLowerCase(),
                    label.toLowerCase(),
                    `Excluded label: ${key}`,
                );
            }
            continue;
        }
        assert.ok(
            Object.hasOwn(mappings, key),
            `Unmapped appendix row: ${key}`,
        );
        const targets = [mappings[key]].flat();
        assert.ok(targets.length, `Empty mapping: ${key}`);
        for (const target of targets) {
            const [prefix, ...rest] = target.split(":");
            assert.ok(
                prefix === "gesture"
                    ? checks.has(rest.join(":"))
                    : ids.has(target),
                `${key} maps to missing command or browser check ${target}`,
            );
        }
    }
    for (const key of Object.keys(mappings)) {
        assert.ok(
            seen.has(key),
            `Removed appendix row is still mapped: ${key}`,
        );
    }
});
