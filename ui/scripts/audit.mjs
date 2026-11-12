// What the stage's motion audit (stage-audit.mjs) and the driver (drive.mjs) share: `record`
// runs in the page and measures every painted card each time the stage's clock is stepped;
// `duplicates` finds two painted instances of the same slot in one frame (only hover
// inspection may show another copy); `timeline` lists when each finding first showed, and
// `ordered` asserts their order: the driver's `spec.ordered`, and any test with a recording.

/** The shortest change that counts as animated, in ms, and the least move that counts. */
export const MIN_MS = 150;
export const MIN_PX = 4;
/** How much of a change must be visible to the player. */
export const SEEN = 0.5;
/** More events than this in one view are too many to follow at once. Taps and steps count
 * for nothing: they come with what they pay for. */
export const CROWDED = 3;

/** Sets up recording in the page. Run with `page.evaluate(record)`. It records no frame of
 * its own: whoever steps the stage's clock takes each with `window.__motion.frame(at)`,
 * stamped with the stage time given, and calls `window.__motion.unhit()` once done measuring
 * (stage-audit.mjs). */
export function record() {
    // The stage's nodes say their name (`data-stage`, src/stage/dom.ts): an ability's has no
    // slot.
    const CARD_SELECTOR =
        "[data-slot], [data-viewer-slot], [data-risen], [data-prompt-slot], " +
        "[data-shown-slot], [data-ghost-slot], [data-ceasing-slot], [data-card], " +
        "[data-stage]";
    const slotOf = ({ dataset: d }) =>
        d.slot ??
        d.viewerSlot ??
        d.risen ??
        d.promptSlot ??
        d.shownSlot ??
        d.ghostSlot ??
        d.ceasingSlot;
    // A DOM node keeps its own ID when it moves or changes faces. A different node
    // can take over the same object, and a recycled slot can get a new node.
    const ids = new WeakMap();
    let nextId = 0;
    const id = (e) => {
        if (!ids.has(e)) ids.set(e, ++nextId);
        return ids.get(e);
    };
    // An element's opacity through its ancestors', each worked out once a frame: the cards
    // of a layer share theirs.
    let opacities = new Map();
    const through = (x) => {
        if (!x || x === document.body) return 1;
        let o = opacities.get(x);
        if (o === undefined) {
            const own = Number(getComputedStyle(x).opacity);
            o = own === 0 ? 0 : own * through(x.parentElement);
            opacities.set(x, o);
        }
        return o;
    };
    const opacity = (e) => Math.round(through(e) * 100) / 100;
    // Hit testing follows paint order only where everything takes the pointer: a card in
    // flight, or the layer it flies in, may not. Only while a frame is measured, so a live
    // click between frames lands as it would for a player.
    const hits = document.createElement("style");
    hits.textContent = "* { pointer-events: auto !important; }";
    // Whether an element paints anything a player would see over what lies below it.
    const paints = (x) => {
        // An SVG's root is a canvas for its shapes, which paint where they are hit: an arrows
        // layer over the whole table covers nothing between its arrows.
        if (x.tagName === "svg") return false;
        if (x instanceof SVGGraphicsElement) return true;
        // So is a canvas of the stage's light (src/stage/glow.ts), as large as the table and
        // clear but where something is lit, and light lies on a card without hiding it.
        if (x.classList.contains("stage-lights")) return false;
        if (/^(IMG|CANVAS|VIDEO)$/.test(x.tagName)) return true;
        const style = getComputedStyle(x);
        if (style.backgroundImage !== "none") return true;
        const alpha = style.backgroundColor.match(/[\d.]+/g);
        if (alpha && (alpha.length < 4 ? 1 : Number(alpha[3])) > 0.1)
            return true;
        return [...x.childNodes].some(
            (n) => n.nodeType === Node.TEXT_NODE && n.textContent.trim(),
        );
    };
    // What lies at a point, topmost first, and whether an element hides what is under it:
    // each asked of the page once a frame, since cards in a pile are tested at the same
    // points against the same cards over them.
    let lying = new Map();
    let hiding = new Map();
    const under = (cx, cy) => {
        const key = `${cx},${cy}`;
        let found = lying.get(key);
        if (!found)
            lying.set(key, (found = document.elementsFromPoint(cx, cy)));
        return found;
    };
    const hides = (x) => {
        let found = hiding.get(x);
        if (found === undefined) {
            hiding.set(x, (found = paints(x) && opacity(x) > 0.3));
        }
        return found;
    };
    // Whether `e` is what a player sees at (cx, cy): the topmost painted element there is
    // in it.
    const onTop = (e, cx, cy) => {
        for (const x of under(cx, cy)) {
            if (x === e || e.contains(x)) return true;
            if (hides(x)) return false;
        }
        return false;
    };
    // What a player sees at (cx, cy) in place of `e`: the stage's node that lies over it
    // there, by its name; an announcement passing over the table (`.stage-notice`); or how
    // the element that does is classed.
    const coverOf = (e, cx, cy) => {
        for (const x of under(cx, cy)) {
            if (x === e || e.contains(x)) return undefined;
            if (!hides(x)) continue;
            const node = x.closest("[data-stage]");
            if (node) return node.dataset.stage;
            if (x.closest(".stage-notice, .stage-notice-scry")) {
                return ".stage-notice";
            }
            return `.${String(x.className.baseVal ?? x.className).split(" ")[0]}`;
        }
        return undefined;
    };
    const frames = [];
    const recording = { frames };
    window.__motion = recording;
    const frame = (at) => {
        if (!hits.isConnected) document.head.append(hits);
        opacities = new Map();
        lying = new Map();
        hiding = new Map();
        const instances = [];
        const inside = (x, y) =>
            x >= 0 && y >= 0 && x < innerWidth && y < innerHeight;
        for (const e of document.querySelectorAll(CARD_SELECTOR)) {
            if (e.closest(".inspect")) continue;
            const slot = slotOf(e);
            // A stack indicator or mode may reuse a card's art. It has its own
            // identity. A real slot inside either is still audited.
            if (slot == null && e.closest("[data-stack-key], [data-mode]"))
                continue;
            const parent = e.parentElement?.closest(CARD_SELECTOR);
            if (parent && (slot == null || slot === slotOf(parent))) continue;
            // Borrowing hides the actual face, which can move inside a stationary
            // hit target. A raised hand card has a separate visual above that target.
            const face =
                e.querySelector(
                    ":scope > .shown-card, .card-body, .pile-card-rim",
                ) ?? e;
            const r = face.getBoundingClientRect();
            const style = getComputedStyle(face);
            const o = style.visibility === "visible" ? opacity(face) : 0;
            const points = [
                [0.5, 0.5],
                [0.2, 0.2],
                [0.8, 0.2],
                [0.2, 0.8],
                [0.8, 0.8],
            ]
                .map(([fx, fy]) => [
                    r.left + r.width * fx,
                    r.top + r.height * fy,
                ])
                .filter(([x, y]) => inside(x, y));
            const top =
                o > 0.3 &&
                r.width > 1 &&
                r.height > 1 &&
                points.some(([x, y]) => onTop(face, x, y));
            const flip = o > 0.3 && face.querySelector(".card-flip");
            const surface = flip && flip.firstElementChild;
            const transform = flip && getComputedStyle(flip).transform;
            const turn =
                transform && transform !== "none"
                    ? new DOMMatrix(transform).m11
                    : 1;
            instances.push({
                id: id(e),
                stage: id(e.closest(".stage") ?? document.body),
                node: e.dataset.stage,
                slot,
                tokenLife: e.dataset.tokenLife,
                ceasing: e.hasAttribute("data-ceasing-slot"),
                where:
                    e.dataset.zone ?? e.classList[0] ?? e.tagName.toLowerCase(),
                zone: e.dataset.zone,
                librarySide: e.dataset.libraryPile,
                name: e.dataset.card ?? e.getAttribute("aria-label") ?? null,
                x: Math.round(r.left + r.width / 2),
                y: Math.round(r.top + r.height / 2),
                w: Math.round(r.width),
                h: Math.round(r.height),
                o,
                on: points.length > 0,
                top,
                // What lies over a card that is not on top (stage-audit.mjs).
                ...(!top &&
                    o > 0.3 &&
                    points.length > 0 && {
                        cover: coverOf(face, ...points[0]),
                    }),
                ...(flip && {
                    turn,
                    back:
                        !!surface?.matches(".card-back, .card-face-down") ||
                        !!surface?.querySelector(".card-face-down"),
                }),
                ...(e.hasAttribute("data-dissolving") && {
                    mask: getComputedStyle(e).maskPosition,
                }),
            });
        }
        // The rule stays in from tick to tick, each change of it restyling the whole page:
        // whoever steps takes it out before anything is clicked (`unhit`).
        frames.push({ t: at, instances });
    };
    recording.frame = frame;
    recording.unhit = () => hits.remove();
}

/** Report visible duplicates even if the overlap lasts only a single frame.
 * Group by stage and slot, never by name when both instances have slots. An instance that
 * `allows(frame, instance)` may show again in that frame: under the stage, what the design
 * language says is shown twice (stage-audit.mjs). */
export function duplicates(frames, allows = () => false) {
    const reported = new Set();
    const out = [];
    for (const frame of frames) {
        const { t, instances } = frame;
        const bySlot = new Map();
        const byName = new Map();
        for (const card of instances.filter((c) => c.top)) {
            if (allows(frame, card)) continue;
            if (card.slot != null) {
                const key = `${card.stage}:${card.slot}`;
                const list = bySlot.get(key) ?? [];
                list.push(card);
                bySlot.set(key, list);
            }
            if (card.name) {
                const key = `${card.stage}:${card.name}`;
                const list = byName.get(key) ?? [];
                list.push(card);
                byName.set(key, list);
            }
        }
        const report = (key, cards) => {
            if (reported.has(key)) return;
            reported.add(key);
            const locations = cards.map((c) => `${c.where} #${c.id}`);
            const name = cards.find((c) => c.name)?.name ?? "Face-down card";
            const slot = cards[0].slot;
            out.push({
                kind: "duplicate",
                what: `${name}${slot == null ? "" : ` (slot ${slot})`} shown twice`,
                verdict: "duplicate",
                why: locations.join(" + "),
                start: t,
            });
        };
        for (const [key, cards] of bySlot) {
            // A token can leave a dissolving visual behind when its slot is
            // reused. Those two lifetimes represent distinct game objects.
            for (const [i, a] of cards.entries()) {
                const b = cards
                    .slice(i + 1)
                    .find(
                        (b) =>
                            a.tokenLife == null ||
                            b.tokenLife == null ||
                            a.tokenLife === b.tokenLife,
                    );
                if (b) report(`slot:${key}`, [a, b]);
            }
        }
        for (const [key, cards] of byName) {
            // Legacy unlabelled card faces, such as a result illustration,
            // have no slot. A name match is the only evidence for those.
            if (cards.length > 1 && cards.some((c) => c.slot == null))
                report(`name:${key}`, cards);
        }
    }
    return out;
}

/** What a seat's run showed, in the order it first showed it: each finding with an `at`, as
 * `{ at, text }`, its text its `what` and `why` ("Winds of Abandon hand->stack (cast): moved
 * 300px over 500 ms"). */
export function timeline(findings) {
    return findings
        .filter((f) => f.at !== undefined && f.verdict !== "still")
        .sort((a, b) => a.at - b.at)
        .map((f) => ({ at: f.at, text: `${f.what}: ${f.why}` }));
}

/** Whether `entries` (a `timeline`) showed what `patterns` match in that order: each pattern,
 * a substring (any case) or a RegExp, takes the first entry it matches that no earlier
 * pattern took, and each must show no earlier than the one before it. Returns `ok`, each
 * pattern's match (`at` and `text`, or null), and why not, if not. */
export function ordered(entries, patterns) {
    const used = new Set();
    const matched = patterns.map((p) => {
        const test =
            p instanceof RegExp
                ? (t) => p.test(t)
                : (t) => t.toLowerCase().includes(String(p).toLowerCase());
        const i = entries.findIndex((e, n) => !used.has(n) && test(e.text));
        if (i < 0) return { pattern: String(p), at: null, text: null };
        used.add(i);
        return { pattern: String(p), ...entries[i] };
    });
    const missing = matched.find((m) => m.at === null);
    if (missing) {
        return { ok: false, matched, why: `nothing showed ${missing.pattern}` };
    }
    const early = matched.findIndex(
        (m, i) => i > 0 && m.at < matched[i - 1].at,
    );
    if (early > 0) {
        const [a, b] = [matched[early - 1], matched[early]];
        return {
            ok: false,
            matched,
            why: `${b.pattern} showed at ${b.at} ms, before ${a.pattern} at ${a.at} ms`,
        };
    }
    return { ok: true, matched };
}
