// A recorded fixture's views, one line each, with the index a route's `&at=` takes
// (`?fixture=<name>&at=<n>` opens the page at its nth view): `by ui-shots --views <name>`
// prints them, and the driver's `spec.fixtureViews` returns them. Usage:
//   node scripts/fixture-views.mjs <recorded-fixture.json>
import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";

/** Each view of `fixture` (a recording: `{ messages }`): its `at` index, turn, step, who
 * holds priority, what the viewer is asked (or the game's status) and its events in brief. */
export function summary(fixture) {
    return fixture.messages
        .filter((m) => m.kind === "event")
        .map((m, at) => {
            const v = m.payload.view;
            const names = new Map(v.objects.map((o) => [o.slot, o.name]));
            const name = (slot) => names.get(slot) ?? `#${slot}`;
            const status =
                v.status.kind === "deciding"
                    ? `asks ${v.status.decision.kind} (${v.status.decision.prompt.purpose})`
                    : v.status.kind;
            const events = (v.events ?? [])
                .filter((e) => e.kind !== "stepBegins")
                .map((e) => {
                    if (e.kind === "zoneChange") {
                        return `${name(e.slot)} ${e.from}->${e.to}`;
                    }
                    if ("slot" in e) return `${e.kind} ${name(e.slot)}`;
                    return e.kind;
                });
            return {
                at,
                turn: `turn ${v.turn.number} ${v.turn.active} ${v.turn.step}`,
                status,
                stack: v.stack.length,
                events,
            };
        });
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
    const [file] = process.argv.slice(2);
    if (!file) {
        console.error(
            "usage: node scripts/fixture-views.mjs <recorded-fixture.json>",
        );
        process.exit(2);
    }
    for (const v of summary(JSON.parse(await readFile(file, "utf8")))) {
        const stack = v.stack ? `  stack ${v.stack}` : "";
        const events = v.events.length ? `  ${v.events.join("; ")}` : "";
        console.log(
            `at=${String(v.at).padEnd(4)} ${v.turn}  ${v.status}${stack}${events}`,
        );
    }
}
