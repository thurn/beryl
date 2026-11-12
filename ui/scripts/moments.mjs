// Opening a spec moment headless and playing it: what the film, the motion audit and the
// driver share (`by specs --film | --motion | --drive`, tools/). A moment's seat page,
// `/scenarios/<card>/<moment>/<yours|theirs>?hold`, is a fresh game that waits for its play, so
// a script starts watching before anything moves. Motion is not reduced: how things move is
// what these look at. A page is settled when its stage says so, on no timer.

/** The most to wait for a page to open or to settle. */
export const LONGEST = 30000;

/** How many times a request or a seat's opening is tried, and the wait before the first
 * retry, doubled each time: a loaded machine, or a spec server restarting after a rebuild,
 * answers late or with a proxy's 502. */
const TRIES = 4;
const BACKOFF = 1000;

/** Runs `attempt` until it resolves, up to TRIES times, waiting between tries; rethrows the
 * last failure. */
export async function retried(attempt, tries = TRIES) {
    for (let n = 1; ; n++) {
        try {
            return await attempt(n);
        } catch (e) {
            if (n >= tries) throw e;
            await new Promise((r) => setTimeout(r, BACKOFF * 2 ** (n - 1)));
        }
    }
}

/** `fetch`, retried on a network error or a 5xx answer. */
export function request(url, init) {
    return retried(async () => {
        const response = await fetch(url, init);
        if (response.status >= 500) {
            throw new Error(`${url} answered HTTP ${response.status}`);
        }
        return response;
    });
}

/** The spec server's listing: each card's name and slug, generating no spec. */
export async function listing(base) {
    return answer(await request(new URL("spec-api/specs", base)));
}

/** One card's moments, generating its spec. */
export async function moments(base, slug) {
    const response = await request(new URL(`spec-api/specs/${slug}`, base));
    return (await answer(response)).moments;
}

/** A spec server answer's JSON, or a throw saying what it answered. */
function answer(response) {
    if (!response.ok) {
        throw new Error(`the spec server answered HTTP ${response.status}`);
    }
    return response.json();
}

/** How many cards' moments are asked for at once when every card's are: each generates a
 * spec on the server, so not every card at once. */
const AT_ONCE = 8;

/** The specs for `slugs` (every one if none), each with its moments, the one `moment` names
 * if given. Throws naming a slug or moment with no spec. */
export async function chosen(base, slugs, moment) {
    const all = await listing(base);
    const unknown = slugs.filter((s) => !all.some((x) => x.slug === s));
    if (unknown.length) throw new Error(`no spec for ${unknown.join(", ")}`);
    const picked = all.filter(
        (s) => slugs.length === 0 || slugs.includes(s.slug),
    );
    const specs = [];
    for (let i = 0; i < picked.length; i += AT_ONCE) {
        const batch = picked.slice(i, i + AT_ONCE);
        const listed = await Promise.all(
            batch.map((s) => moments(base, s.slug)),
        );
        batch.forEach((s, n) =>
            specs.push({
                ...s,
                moments: listed[n].filter((m) => !moment || m.name === moment),
            }),
        );
    }
    if (moment && specs.every((s) => s.moments.length === 0)) {
        throw new Error(`no moment ${moment}`);
    }
    return specs;
}

/** What a seat's address asks for besides: the phone's layout (`by specs --phone`). */
export const LAYOUT = process.env.BERYL_PHONE ? "&phone" : "";

/** Opens a moment's seat in `page`, held, and waits until its first view shows, retried (a
 * fresh game each try). Returns the page's live game URL on the spec server, which `play`
 * posts to. */
export async function openSeat(page, base, slug, moment, seat, instance) {
    return retried(async (n) => {
        await page.goto(
            new URL(
                `${moment.startsWith("playground-") ? "playground" : "scenarios"}/${slug}/${moment}/${seat}?hold${LAYOUT}${instance ? `&instance=${instance}` : ""}`,
                base,
            ).href,
        );
        // A cold Vite compiles the whole app for the first page: its first try waits longer.
        const timeout = n === 1 ? 2 * LONGEST : LONGEST;
        await page.waitForURL(/[?&]live=/, { timeout });
        await page.waitForSelector("[data-life]", { timeout });
        return new URL(page.url()).searchParams.get("live");
    }, 3);
}

/** Plays out what each live game holds, all at once. */
export async function play(...lives) {
    await Promise.all(
        lives.map((live) => request(`${live}/play`, { method: "POST" })),
    );
}

/** Warms the UI's dev server: loads the app once, so the first moment's page does not wait for
 * Vite to compile it under a seat's timeout. */
export async function warm(browser, base) {
    const context = await browser.newContext();
    try {
        const page = await context.newPage();
        await retried(() =>
            page.goto(base, { waitUntil: "load", timeout: 4 * LONGEST }),
        );
    } finally {
        await context.close();
    }
}

/** A viewport from "WxH" ("1180x820"), or the default. */
export function viewport(text) {
    if (!text) return { width: 1600, height: 900 };
    const m = /^(\d+)x(\d+)$/.exec(text);
    if (!m) throw new Error(`a viewport is WxH, as 1180x820, not ${text}`);
    return { width: Number(m[1]), height: Number(m[2]) };
}

/** Takes `--name value` out of `args`, returning the value (undefined if absent), and
 * `--name` alone as true with `flag`. */
export function option(args, name, flag = false) {
    const at = args.indexOf(name);
    if (at < 0) return flag ? false : undefined;
    return flag ? Boolean(args.splice(at, 1)) : args.splice(at, 2)[1];
}

/** How far the page is from rest. `owed` a view: the last one heard
 * plays on (it is followed unasked, ipc.ts `playsOn`), or the page has since sent an answer
 * the session took, or has yet to reply to, and no view has come of it; a click that only
 * changed its prompt's state brings none. `shown`: the stage shows the last view heard, its
 * script played out. `busy`: the stage is playing. `settled`: none owed, the last shown and
 * nothing playing. A replayed recording has no log: there the stage's word is all. */
export function rest() {
    const stage = window.berylStage;
    const { busy } = stage.shown;
    const log = window.berylLive?.log;
    if (!log) return { owed: false, shown: true, busy, settled: !busy };
    const updateOf = (entry) =>
        entry.kind === "event" && entry.event === "game-update"
            ? entry.payload
            : entry.kind === "result" &&
                entry.command === "current_game" &&
                entry.ok
              ? entry.value
              : undefined;
    const heard = log.findLastIndex((entry) => updateOf(entry) !== undefined);
    if (heard < 0) return { owed: true, shown: false, busy, settled: false };
    const { view } = updateOf(log[heard]);
    const answers = ["choose", "click", "undo", "cancel"];
    const owed =
        view.status.kind === "thinking" ||
        view.status.kind === "playing" ||
        log.slice(heard + 1).some((sent, n) => {
            if (sent.kind !== "invoke" || !answers.includes(sent.command)) {
                return false;
            }
            const reply = log
                .slice(heard + n + 2)
                .find((r) => r.kind === "result" && r.command === sent.command);
            return (
                !reply ||
                (reply.ok &&
                    (sent.command !== "click" ||
                        reply.value?.kind === "answered"))
            );
        });
    const shown = stage.shown.view === view;
    return { owed, shown, busy, settled: !owed && shown && !busy };
}

/** Waits until `page` is at rest (`rest`), asked each frame, or `LONGEST`: an opponent who
 * never answers leaves a view owed for good, and the wait ends there. */
export async function settled(page) {
    await page
        .waitForFunction(`(${rest})().settled`, undefined, { timeout: LONGEST })
        .catch((e) => {
            if (e.name !== "TimeoutError") throw e;
        });
}

/** A browser context and page at `width`×`height` with motion on, collecting the page's
 * errors and console errors into `errors`. */
export async function moving(browser, { width = 1600, height = 900 } = {}) {
    const context = await browser.newContext({
        viewport: { width, height },
        deviceScaleFactor: 1,
    });
    const page = await context.newPage();
    const errors = [];
    page.on("pageerror", (e) => errors.push(e.message));
    page.on("console", (m) => m.type() === "error" && errors.push(m.text()));
    return { context, page, errors };
}

/** The seats, in the spec server's words. */
export const SEATS = ["yours", "theirs"];
