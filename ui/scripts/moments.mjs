// Opening a spec moment headless and playing it: what the film, the motion audit and the
// driver share (`by specs --film | --motion | --drive`, tools/). A moment's seat page,
// `/specs/<card>/<moment>/<yours|theirs>?hold`, is a fresh game that waits for its play, so
// a script starts watching before anything moves. Motion is not reduced: how things move is
// what these look at.

/** How long the page must hear no view and queue none before it counts as settled, how long
 * the last view's animation may take after that, and the most to wait for either. */
export const QUIET = 1500;
export const LAST_VIEW = 1200;
export const LONGEST = 30000;

/** The spec server's listing: each card's slug and its moments. */
export async function listing(base) {
    const response = await fetch(new URL("spec-api/specs", base));
    if (!response.ok) {
        throw new Error(`the spec server answered HTTP ${response.status}`);
    }
    return response.json();
}

/** The specs for `slugs` (every one if none), each with its moments, the one `moment` names
 * if given. Throws naming a slug or moment with no spec. */
export async function chosen(base, slugs, moment) {
    const all = await listing(base);
    const unknown = slugs.filter((s) => !all.some((x) => x.slug === s));
    if (unknown.length) throw new Error(`no spec for ${unknown.join(", ")}`);
    const specs = all
        .filter((s) => slugs.length === 0 || slugs.includes(s.slug))
        .map((s) => ({
            ...s,
            moments: s.moments.filter((m) => !moment || m.name === moment),
        }));
    if (moment && specs.every((s) => s.moments.length === 0)) {
        throw new Error(`no moment ${moment}`);
    }
    return specs;
}

/** Opens a moment's seat in `page`, held, and waits until its first view shows. Returns the
 * page's live game URL on the spec server, which `play` posts to. */
export async function openSeat(page, base, slug, moment, seat) {
    await page.goto(new URL(`specs/${slug}/${moment}/${seat}?hold`, base).href);
    await page.waitForURL(/[?&]live=/, { timeout: LONGEST });
    await page.waitForSelector("[data-life]", { timeout: LONGEST });
    return new URL(page.url()).searchParams.get("live");
}

/** Plays out what each live game holds, all at once. */
export async function play(...lives) {
    await Promise.all(
        lives.map((live) => fetch(`${live}/play`, { method: "POST" })),
    );
}

/** Waits until `page` has heard no view for `QUIET` ms and queues none, then `LAST_VIEW` more
 * for its animation; or `LONGEST`. */
export async function settled(page) {
    const started = await page.evaluate(() => performance.now());
    await page.waitForFunction(
        ([quiet, longest, started]) => {
            const log = window.berylLive?.log ?? [];
            const last = log.at(-1)?.at ?? 0;
            const now = performance.now();
            return (
                (now - last > quiet && !window.berylBehind) ||
                now - started > longest
            );
        },
        [QUIET, LONGEST, started],
        { timeout: LONGEST + 5000, polling: 100 },
    );
    await page.waitForTimeout(LAST_VIEW);
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
