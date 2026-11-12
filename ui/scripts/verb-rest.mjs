// What the verb suite reads of the stage's streaks and announcements, in the page (verbs.mjs
// `walk`; src/stage/streaks.ts, src/stage/overlay.ts, src/stage/dom.ts): at each tick, every
// streak the stage draws with the element that draws it, and how far through each announcement
// the stage has written the table to be; and, once React has drawn what the stage told it,
// what the page shows of each announcement. The page is held to the model here: one path a
// streak, with the model's shape and the look of what it is made of, and none besides. It
// lives apart from `walk` only for that script's length.

/** In the page: what reads the streaks and announcements of `stage` at each tick, for a walk
 * that began at stage time `origin` on a `clock` it holds and draws in `layer`, and tells its
 * `faults`. */
export function restSampler({ stage, clock, origin, layer, faults }) {
    /** A number that names an element for as long as it is on the page. */
    const numbered = new WeakMap();
    let made = 0;
    const numberOf = (element) => {
        if (!numbered.has(element)) numbered.set(element, ++made);
        return numbered.get(element);
    };
    /** The middle, in stage pixels, of what the stage names `on`, worked out apart from the
     * stage's own streaks: a card as its pose says it lies, or a player's life total as the
     * page draws it. */
    const middleOf = (on) => {
        if (on === "you" || on === "opponent") {
            const table = layer.getBoundingClientRect();
            const by = table.width / layer.offsetWidth;
            const orb = document
                .querySelector(`[data-life="${on}"]`)
                .getBoundingClientRect();
            return {
                x: (orb.left + orb.width / 2 - table.left) / by,
                y: (orb.top + orb.height / 2 - table.top) / by,
                reach: orb.height / by,
            };
        }
        const pose = stage.pose(on);
        const resting = stage.shown.resting.get(on);
        if (!pose || !resting) return null;
        const lies =
            resting.dress.zone === "battlefield" && resting.object?.tapped;
        const long = (pose.width * 88) / 63;
        const [width, height] = lies ? [long, pose.width] : [pose.width, long];
        const lean = Math.tan((pose.skew * Math.PI) / 180) * (height / 2);
        return {
            x: pose.x + width / 2 + lean,
            y: pose.y + height / 2,
            reach: Math.max(width, height),
        };
    };
    return () => {
        const at = clock.now - origin;
        const fault = (why) => faults.push(`at ${at} ms ${why}`);
        const held = stage.streaks();
        const paths = [...layer.querySelectorAll(".stage-streak")];
        if (paths.length !== held.length) {
            fault(`the page has ${paths.length} streaks for ${held.length}`);
        }
        const streaks = held.map(({ key, look, to, through, shape }) => {
            const drawn = paths.filter((path) => path.dataset.streak === key);
            const d = drawn[0]?.getAttribute("d") ?? null;
            if (drawn.length !== 1) {
                fault(`the streak ${key} has ${drawn.length} elements`);
            } else if (d !== (shape?.d ?? "")) {
                fault(`the streak ${key} is drawn as ${d}`);
            } else if (!drawn[0].matches(`.stage-streak-${look}`)) {
                fault(`the streak ${key} is drawn as ${drawn[0].className}`);
            } else if (shape && through > 0.1) {
                // And the browser paints it, once it has left: a path of some size, filled.
                const box = drawn[0].getBBox();
                const style = getComputedStyle(drawn[0]);
                if (!(box.width + box.height > 0) || style.fill === "none") {
                    fault(`the streak ${key} paints nothing`);
                }
            }
            return {
                key,
                look,
                to,
                through,
                drawn: shape !== null,
                element: drawn[0] ? numberOf(drawn[0]) : null,
                head: shape?.head ?? null,
                tail: shape?.tail ?? null,
                target: middleOf(to),
            };
        });
        const style = layer.parentElement.style;
        const notices = stage.shown.notices.map(({ act }) => {
            const kind = act.notice.kind;
            const written = style.getPropertyValue(`--notice-${kind}`);
            return { kind, through: written === "" ? null : Number(written) };
        });
        // Nothing is left written for an announcement that is over.
        for (const kind of ["turn", "scry", "coin"]) {
            const stale =
                style.getPropertyValue(`--notice-${kind}`) !== "" &&
                !notices.some((notice) => notice.kind === kind);
            if (stale) fault(`the ${kind} announcement is still written`);
        }
        return { streaks, notices };
    };
}

/** In the page: what it shows of each announcement, once React has drawn what the stage told
 * it: its kind, its words, and how the browser draws it from the number the stage wrote. */
export async function noticed() {
    await new Promise((resolve) => setTimeout(resolve));
    const read = (kind, element, words) => {
        if (!element) return [];
        const style = getComputedStyle(element);
        return [
            {
                kind,
                words: words.map((w) => w?.textContent ?? null),
                opacity: Number(style.opacity),
                down: parseFloat(style.translate.split(" ")[1] ?? 0),
                side: element.dataset.turn ?? null,
            },
        ];
    };
    const turn = document.querySelector(".turn-banner.stage-notice");
    const scry = document.querySelector(".stage-notice-scry");
    const coin = document.querySelector(".step-banner.stage-notice");
    return [
        ...read("turn", turn, [turn?.querySelector(".turn-banner-title")]),
        ...read("scry", scry, [scry]),
        ...read("coin", coin, [
            coin?.querySelector(".step-banner-title"),
            coin?.querySelector(".step-banner-detail"),
        ]),
    ];
}

/** Gives `page` the sampler (`window.berylRestSampler`) and the reader of what it shows
 * (`window.berylNoticed`), for a walk played in it. */
export const install = (page) =>
    page.evaluate(
        `window.berylRestSampler = ${restSampler}; window.berylNoticed = ${noticed}`,
    );
