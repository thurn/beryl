// What a check script reports of a failure, and how it lists its checks: the place in the UI's
// source a failure names, and the JSON lines of Ignite's `list` and results (docs/ignite.md,
// "Declaring a family" and "Reporting results"). capture.mjs re-exports all of it.

/** A place in the UI's source, `ui/<path>:<line>`, in a stack or message: a module the dev
 * server served the page (`http://127.0.0.1:5173/src/stage/x.ts?t=1:212:7`) or a script of
 * this folder (`file:///…/ui/scripts/kept-x.mjs:40:5`), or a place `pageError` already wrote
 * (`(at ui/src/stage/x.ts:212)`). */
const SOURCE_PLACE =
    /(?:https?:\/\/[^/\s]+\/|(?:^|[/\s(])ui\/)((?:src|scripts)\/[^?:\s()]+)(?:\?[^:\s()]*)?:(\d+)/;

/** The first place in the UI's source that `text` names, as `ui/<path>:<line>`; null if none. */
export function sourcePlace(text) {
    const found = SOURCE_PLACE.exec(text ?? "");
    return found ? `ui/${found[1]}:${found[2]}` : null;
}

/** A page error as a check reports it: its message, with the place in the app's source it was
 * thrown from where its stack names one. */
export function pageError(e) {
    const place = sourcePlace(e.stack);
    return place ? `${e.message} (at ${place})` : e.message;
}

/** Prints each of `names` as a check's JSON line and exits: a script's `--list` (Ignite's
 * `list`, docs/ignite.md "Declaring a family"). */
export function listChecks(names) {
    for (const check of names) console.log(JSON.stringify({ check }));
    process.exit(0);
}

/** Lists a suite's checks, `<prefix><case>/<seat>/<layout>`, for `cases` of [case, its
 * seats as an object keyed by seat], and exits (verbs.mjs and shapes.mjs `--list`). */
export function listSuite(prefix, cases, layouts) {
    listChecks(
        cases.flatMap(([name, seats]) =>
            Object.keys(seats).flatMap((seat) =>
                layouts.map((layout) => `${prefix}${name}/${seat}/${layout}`),
            ),
        ),
    );
}

/** The longest message a result carries to Ignite. */
const MESSAGE_LIMIT = 300;

/** What failed in a check, for `recordResult`: the first of `texts` (the page's errors, then
 * what the check threw, each a string or an error) as the message, and the first place in the
 * UI's source any of them names. */
export function failureOf(...texts) {
    const said = texts
        .flat()
        .filter(Boolean)
        .map((t) => (t instanceof Error ? (t.stack ?? t.message) : String(t)));
    if (said.length === 0) return {};
    const first = said[0].split("\n")[0].replace(/\s+/g, " ").trim();
    const message =
        first.length <= MESSAGE_LIMIT
            ? first
            : `${first.slice(0, MESSAGE_LIMIT - 1)}…`;
    const at = said.map(sourcePlace).find(Boolean);
    return at ? { message, at } : { message };
}
