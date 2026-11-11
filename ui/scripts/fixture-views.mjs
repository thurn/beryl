// A recorded fixture's views, one line each, with the index a route's `&at=` takes
// (`?fixture=<name>&at=<n>` opens the page at its nth view): `by ui-shots --views <name>`
// prints them, and the driver's `spec.fixtureViews` returns them. Usage:
//   node scripts/fixture-views.mjs <recorded-fixture.json>
import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";

/** A script step in brief: its verb (`+` or `-` before one that begins or ends a span) and
 * what it acts on, by name: "fly Counterspell", "+point hostile-target", "set turn". The full
 * text form is `beryl_session::script`'s (`just specs --script`). */
function stepBrief(step, name) {
    const a = step.act;
    const subject = (s) =>
        s.kind === "object"
            ? name(s.slot)
            : s.kind === "ability"
              ? `a${s.id}`
              : s.player;
    const what = {
        vanish: () => subject(a.subject),
        dwell: () => subject(a.subject),
        emerge: () => `a${a.id}`,
        strike: () => name(a.source),
        streak: () => subject(a.to),
        count: () =>
            `${a.tally.kind} ${a.tally.object ? name(a.tally.object.slot) : a.tally.player}`,
        set: () => a.fact.kind,
        announce: () => a.notice.kind,
        point: () => a.arrow,
        mark: () => a.mark,
        lift: () => a.handling,
    }[a.verb];
    const on = what ? what() : a.object ? name(a.object.slot) : "";
    const span = a.span === "begin" ? "+" : a.span === "end" ? "-" : "";
    return `${span}${a.verb} ${on}`.trim();
}

/** Each view of `fixture` (a recording: `{ messages }`): its `at` index, turn, step, who
 * holds priority, what the viewer is asked (or the game's status), its events and its
 * script's steps in brief (beats apart, a beat's steps joined by `&`). */
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
            const steps = [
                ...(v.script?.reset ? ["reset"] : []),
                ...(v.script?.beats ?? []).map((beat) =>
                    beat.map((step) => stepBrief(step, name)).join(" & "),
                ),
            ];
            return {
                at,
                turn: `turn ${v.turn.number} ${v.turn.active} ${v.turn.step}`,
                status,
                stack: v.stack.length,
                events,
                steps,
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
        if (v.steps.length) console.log(`        steps ${v.steps.join(" | ")}`);
    }
}
