// The motion audit's measuring and judging, shared by `by specs --motion` (motion.mjs) and
// the driver (`probe.audit`, drive.mjs): `record` runs in the page and keeps every animation
// frame's cards and life totals; `collect` returns them with the views the page showed
// (`window.berylShown`, game/playback.ts); `audit` matches each event to the elements that
// should show it, over the time its view held the screen, and says how it showed:
//   ok        they moved, faded or animated for long enough, where the player could see it
//   instant   they changed (appeared, vanished, moved, turned) within MIN_MS: a jump
//   unseen    they changed, but off screen, covered or transparent the while
//   still     nothing showed it: no element changed, or none shows it
//   crowded   a view showed more than CROWDED events at once, too many to follow
//   duplicate the same card showed twice at once for DUPLICATE_MS or longer: a second copy of
//             a card (one slot drawn twice, or a copy with no slot beside it), outside a
//             prompt, which shows copies of the cards it offers on purpose
//   jump      an arrow took a new path while both its ends stood still: rerouted round
//             something that moved elsewhere, which the eye reads as the arrow jumping
//   stretch   an arrow was drawn while one of its ends moved, following a card in flight
//             rather than waiting for it to land
// Beside the events, `ui` findings judge what moved with no event to show (a hand rising, a
// chooser opening, a button sliding in): the motion a live click sets going. A finding about a
// tap or untap is `minor`: mana bookkeeping, which a land in a group shows under its
// neighbours; so are `ui` findings when no live action set them going (`audit`'s `live`).
// `timeline` lists when each thing first showed, and `ordered` asserts their order: the
// driver's `spec.ordered`, and any test with a recording.

/** The shortest change that counts as animated, in ms, and the least move that counts. */
const MIN_MS = 150;
const MIN_PX = 4;
/** How much of a change must be visible to the player. */
const SEEN = 0.5;
/** How long before a view's logged time its window starts. */
const LEAD = 60;
/** More events than this in one view are too many to follow at once. Taps and steps count
 * for nothing: they come with what they pay for. */
const CROWDED = 3;
/** Events that need no animation of their own. */
const QUIET_EVENTS = new Set([
    "stepBegins",
    "coinFlip",
    "win",
    "lose",
    "resolutionPayment",
]);
/** Events that come with another and count for nothing in a crowd. */
const MINOR_EVENTS = new Set(["tap", "untap", "draw", "cast", "mana"]);
/** How long a card may show twice before it counts as a duplicate: a hand-off between two
 * elements of one card (a prompt's card settling back onto the table) crosses briefly. */
const DUPLICATE_MS = 250;
/** How long after a view's window a card drawn in it may still arrive in the hand: a draw's
 * beat can hold it back past the next view. */
const DRAW_GRACE = 1500;

/** Starts recording every frame in the page. Run with `page.evaluate(record, ui?)`: `ui`
 * selects the elements other than cards whose motion is judged, by default what a click opens
 * or moves (a chooser's frames, a prompt's title and buttons, the dock's buttons). */
export function record(ui) {
    const UI_SELECTOR =
        ui ??
        ".dock button, .prompt-title, .ability-choice, .choice-card, .chooser-close, .chooser-corner";
    // Where copies of a card are shown on purpose: the cards a prompt offers, the inspector.
    const ALLOWED = "[role=dialog], .prompt, .ability-chooser, .inspector";
    const opacity = (e) => {
        let o = 1;
        for (let x = e; x && x !== document.body; x = x.parentElement) {
            o *= Number(getComputedStyle(x).opacity);
            if (o === 0) break;
        }
        return Math.round(o * 100) / 100;
    };
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
    // Whether `e` is what a player sees at (cx, cy): the topmost painted element there is
    // in it.
    const onTop = (e, cx, cy) => {
        for (const x of document.elementsFromPoint(cx, cy)) {
            if (x === e || e.contains(x)) return true;
            if (paints(x) && opacity(x) > 0.3) return false;
        }
        return false;
    };
    const badges = (e) => [
        ...e.querySelectorAll(
            ".card-counters, .card-pt-counter, .card-loyalty",
        ),
    ];
    const frames = [];
    window.__motion = { frames, stop: false };
    const frame = () => {
        document.head.append(hits);
        const cards = {};
        for (const e of document.querySelectorAll("[data-slot]")) {
            const r = e.getBoundingClientRect();
            if (r.width === 0 && r.height === 0) continue;
            const slot = e.dataset.slot;
            // A slot shown twice (a source and its ability) keeps its largest.
            if (
                cards[slot] &&
                cards[slot].w * cards[slot].h >= r.width * r.height
            )
                continue;
            const cx = r.left + r.width / 2;
            const cy = r.top + r.height / 2;
            // Seen where any of five points across it is on screen and on top: a land under
            // its neighbours in a group, or the opponent's hand peeking in at the edge, shows
            // a part.
            const inside = (x, y) =>
                x >= 0 && y >= 0 && x < innerWidth && y < innerHeight;
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
            const on = points.length > 0;
            const top = points.some(([x, y]) => onTop(e, x, y));
            cards[slot] = {
                zone: e.dataset.zone,
                name: e.dataset.card ?? null,
                x: Math.round(cx),
                y: Math.round(cy),
                w: Math.round(r.width),
                h: Math.round(r.height),
                o: opacity(e),
                on,
                top,
                anim: e.getAnimations({ subtree: true }).length > 0,
                // Its counters and loyalty, as their badges read, and whether they animate.
                badge: badges(e)
                    .map((b) => b.textContent.trim())
                    .join(" "),
                badgeAnim: badges(e).some(
                    (b) => b.getAnimations({ subtree: true }).length > 0,
                ),
            };
        }
        // Each card shown more than once at a time: its name, and where each copy is. Twins
        // (two slots of one name) are two cards; one slot drawn twice, or a copy with no slot
        // beside it, is a duplicate.
        const copies = {};
        for (const e of document.querySelectorAll("[data-card]")) {
            const name = e.dataset.card;
            if (!name || e.closest(ALLOWED)) continue;
            // The outermost element of a card: its own parts may name it too.
            if (e.parentElement?.closest(`[data-card="${CSS.escape(name)}"]`))
                continue;
            const r = e.getBoundingClientRect();
            if (r.width * r.height < 1500 || opacity(e) <= 0.3) continue;
            const visible = [
                [0.5, 0.5],
                [0.25, 0.25],
                [0.75, 0.75],
            ].some(([fx, fy]) => {
                const x = r.left + r.width * fx;
                const y = r.top + r.height * fy;
                return (
                    x >= 0 &&
                    y >= 0 &&
                    x < innerWidth &&
                    y < innerHeight &&
                    onTop(e, x, y)
                );
            });
            if (!visible) continue;
            (copies[name] ??= []).push({
                slot: e.dataset.slot ?? null,
                where:
                    e.dataset.zone ??
                    (e.className && typeof e.className === "string"
                        ? e.className.split(" ")[0]
                        : e.tagName.toLowerCase()),
            });
        }
        const dupes = {};
        for (const [name, list] of Object.entries(copies)) {
            const slots = list.filter((c) => c.slot !== null);
            const distinct = new Set(slots.map((c) => c.slot)).size;
            if (list.length > Math.max(distinct, 1) || slots.length > distinct)
                dupes[name] = list.map((c) => c.where).join(" + ");
        }
        // The other elements a click opens or moves, by their kind and place among those
        // alike.
        const uis = {};
        const counts = {};
        for (const e of document.querySelectorAll(UI_SELECTOR)) {
            const r = e.getBoundingClientRect();
            if (r.width === 0 && r.height === 0) continue;
            const kind =
                typeof e.className === "string" && e.className
                    ? e.className.split(" ")[0]
                    : e.tagName.toLowerCase();
            const label = (
                e.getAttribute("aria-label") ??
                e.dataset.card ??
                e.textContent ??
                ""
            )
                .trim()
                .slice(0, 30);
            // By kind and place, not label: a button whose text changes is the same button.
            counts[kind] = (counts[kind] ?? 0) + 1;
            const key = `${kind} #${counts[kind]}`;
            const cx = r.left + r.width / 2;
            const cy = r.top + r.height / 2;
            uis[key] = {
                label,
                x: Math.round(cx),
                y: Math.round(cy),
                w: Math.round(r.width),
                h: Math.round(r.height),
                o: opacity(e),
                on: cx >= 0 && cy >= 0 && cx < innerWidth && cy < innerHeight,
                top: onTop(e, cx, cy),
                anim: e.getAnimations({ subtree: true }).length > 0,
            };
        }
        const lives = {};
        for (const e of document.querySelectorAll("[data-life]")) {
            const r = e.getBoundingClientRect();
            lives[e.dataset.life] = {
                text: e.textContent.trim(),
                x: Math.round(r.left),
                y: Math.round(r.top),
                w: Math.round(r.width),
                anim: e.getAnimations({ subtree: true }).length > 0,
            };
        }
        // The stack's items that are no card of their own (abilities), by place from the
        // bottom: an item's text changes as it settles.
        const stack = {};
        [...document.querySelectorAll(".stack-item")].forEach((e, n) => {
            const r = e.getBoundingClientRect();
            // By place and name: an item that resolves and another put in its place are two.
            // An ability says its own (the item, not its source's face, which an emblem's
            // shares with its planeswalker), and keeps it while one leaving beneath it shifts
            // the order.
            const i =
                e.closest("[data-stack-key]")?.dataset.stackKey ??
                `${n}:${e.querySelector(".card-name, .card-scan")?.textContent || e.querySelector(".card-scan")?.getAttribute("alt") || ""}`;
            const cx = r.left + r.width / 2;
            const cy = r.top + r.height / 2;
            stack[i] = {
                x: Math.round(cx),
                y: Math.round(cy),
                w: Math.round(r.width),
                h: Math.round(r.height),
                o: opacity(e),
                on: cx >= 0 && cy >= 0 && cx < innerWidth && cy < innerHeight,
                top: onTop(e, cx, cy),
                anim: e.getAnimations({ subtree: true }).length > 0,
            };
        });
        // Mana in flight from its source (game/ManaFlow.tsx), each mote by its source and
        // flight.
        const motes = {};
        for (const e of document.querySelectorAll("[data-mana-id]")) {
            const r = e.getBoundingClientRect();
            const cx = r.left + r.width / 2;
            const cy = r.top + r.height / 2;
            motes[`${e.dataset.manaSource}:${e.dataset.manaId}`] = {
                x: Math.round(cx),
                y: Math.round(cy),
                w: Math.round(r.width),
                h: Math.round(r.height),
                o: opacity(e),
                on: cx >= 0 && cy >= 0 && cx < innerWidth && cy < innerHeight,
                top: true,
                anim: e.getAnimations().length > 0,
            };
        }
        // Each arrow, by its ends, with its path and where its ends' elements are.
        const arrows = {};
        for (const g of document.querySelectorAll(".arrow[data-from]")) {
            const ends = [g.dataset.from, g.dataset.to].map((id) => {
                const r = document.getElementById(id)?.getBoundingClientRect();
                return r
                    ? [r.left, r.top, r.width, r.height]
                          .map(Math.round)
                          .join(",")
                    : "";
            });
            arrows[`${g.dataset.from}>${g.dataset.to}`] = {
                d: g.querySelector("path")?.getAttribute("d") ?? "",
                ends: ends.join("|"),
            };
        }
        hits.remove();
        frames.push({
            arrows,
            t: performance.now(),
            cards,
            lives,
            stack,
            uis,
            dupes,
            motes,
        });
        if (!window.__motion.stop) requestAnimationFrame(frame);
    };
    requestAnimationFrame(frame);
    // A few frames of where things were before the caller sets anything going: a view can
    // arrive within a frame of a play. Counted, not timed: a busy machine may draw no frame
    // for a while, and a run whose first frame comes after the play sees nothing change.
    const started = performance.now();
    return new Promise((done) => {
        const wait = () =>
            frames.length >= 3 || performance.now() - started > 3000
                ? done()
                : setTimeout(wait, 20);
        wait();
    });
}

/** What the page recorded, and the views it showed since `since`, with their events. */
export function collect(since) {
    window.__motion.stop = true;
    const shown = (window.berylShown ?? [])
        .filter((s) => s.at >= since)
        .map((s) => ({
            at: s.at,
            turn: `turn ${s.update.view.turn.number} ${s.update.view.turn.active} ${s.update.view.turn.step}`,
            events: s.update.view.events,
            names: Object.fromEntries(
                s.update.view.objects.map((o) => [o.slot, o.name]),
            ),
        }));
    return { frames: window.__motion.frames, shown, since };
}

/** The slots an event is about, which should show it. */
function subjects(e) {
    switch (e.kind) {
        case "zoneChange":
        case "tap":
        case "untap":
        case "countersAdded":
        case "countersRemoved":
        case "gainControl":
        case "cast":
            return [e.slot];
        case "tokensCreated":
            return e.tokens;
        case "attack":
            return e.attackers;
        case "damage":
            return e.to.kind === "object" ? [e.source, e.to.slot] : [e.source];
        case "ability":
            // An emblem's ability has no source (null): its stack item shows it.
            return e.item.source == null ? [] : [e.item.source];
        default:
            return [];
    }
}

/** An event in a few words, objects by name. */
function describe(e, names) {
    const name = (slot) => names[slot] ?? `#${slot}`;
    switch (e.kind) {
        case "zoneChange":
            return `${name(e.slot)} ${e.from}->${e.to} (${e.reason})`;
        case "tokensCreated":
            return `tokens ${e.tokens.map(name).join(", ")}`;
        case "ability":
            return `ability of ${e.item.name ?? name(e.item.source)}`;
        case "damage":
            return `${e.amount} damage from ${name(e.source)} to ${
                e.to.kind === "object" ? name(e.to.slot) : e.to.player
            }`;
        case "lifeGain":
        case "lifeLoss":
            return `${e.player} ${e.kind === "lifeGain" ? "+" : "-"}${e.amount} life`;
        case "draw":
            return `${e.player} draws ${e.cards}`;
        case "cast":
            return `cast ${name(e.slot)}`;
        case "countersAdded":
        case "countersRemoved":
            return `${name(e.slot)} ${e.kind === "countersAdded" ? "+" : "-"}${e.amount} ${e.counter}`;
        case "mana":
            return `${e.amount} ${e.color} mana from ${e.source == null ? "an effect" : name(e.source)}`;
        default:
            return "slot" in e ? `${name(e.slot)}` : JSON.stringify(e);
    }
}

/** The longest run of frames over which `changing(prev, cur)` holds, as ms. */
function span(frames, changing) {
    let first;
    let last;
    for (let i = 1; i < frames.length; i++) {
        if (changing(frames[i - 1], frames[i])) {
            first ??= frames[i - 1].t;
            last = frames[i].t;
        }
    }
    return first === undefined ? 0 : last - first;
}

/** How one slot's element behaved over `frames`: what changed, for how long, how visibly. */
function behavior(frames, slot, of = (f) => f.cards[slot]) {
    const track = frames.map((f) => ({ t: f.t, c: of(f) }));
    const present = track.filter((x) => x.c);
    if (present.length === 0) return { none: true };
    // Appeared: absent at first, then shown; vanished: shown, then absent at the end. A
    // stack item that comes and resolves within the window does both.
    const appeared = !track[0].c;
    const vanished = !track.at(-1).c;
    const moved = present.reduce(
        (m, x) =>
            Math.max(
                m,
                Math.hypot(x.c.x - present[0].c.x, x.c.y - present[0].c.y),
            ),
        0,
    );
    const turned = present.some(
        (x) =>
            Math.abs(x.c.w - present[0].c.w) > MIN_PX ||
            Math.abs(x.c.h - present[0].c.h) > MIN_PX,
    );
    const zone = present.some((x) => x.c.zone !== present[0].c.zone);
    const motion = span(
        present,
        (a, b) =>
            Math.abs(a.c.x - b.c.x) + Math.abs(a.c.y - b.c.y) >= 1 ||
            Math.abs(a.c.w - b.c.w) + Math.abs(a.c.h - b.c.h) >= 1,
    );
    const fade = span(present, (a, b) => a.c.o !== b.c.o);
    const anim = span(present, (a, b) => a.c.anim && b.c.anim);
    // Visible: on screen, on top and mostly opaque, over the frames it changed in (or all of
    // them, if it changed in none).
    const changing = present.filter(
        (x, i) =>
            i > 0 &&
            (x.c.x !== present[i - 1].c.x ||
                x.c.y !== present[i - 1].c.y ||
                x.c.o !== present[i - 1].c.o ||
                x.c.anim),
    );
    const judged = changing.length ? changing : present;
    // When it first showed the change: where it appeared, or its first changing frame.
    const start = appeared ? present[0].t : changing[0]?.t;
    const seen =
        judged.filter((x) => x.c.on && x.c.top && x.c.o > 0.3).length /
        judged.length;
    return {
        appeared,
        vanished,
        moved: Math.round(moved),
        turned,
        zone,
        ms: Math.round(Math.max(motion, fade, anim)),
        seen: Math.round(seen * 100) / 100,
        changed: appeared || vanished || moved >= MIN_PX || turned || zone,
        start,
    };
}

/** How the life total `side` showed over `frames`. */
function lifeBehavior(frames, side) {
    const track = frames.map((f) => f.lives[side]).filter(Boolean);
    if (track.length === 0) return { none: true };
    const changed = track.some((l) => l.text !== track[0].text);
    const shown = frames.filter((f) => f.lives[side]);
    const anim = span(
        shown,
        (a, b) => a.lives[side].anim && b.lives[side].anim,
    );
    // A number that counts its way to the new total over several frames is animated too.
    const counting = span(
        shown,
        (a, b) => a.lives[side].text !== b.lives[side].text,
    );
    const at = shown.findIndex(
        (f, i) =>
            i > 0 &&
            (f.lives[side].text !== shown[i - 1].lives[side].text ||
                f.lives[side].anim),
    );
    return {
        changed,
        ms: Math.round(Math.max(anim, counting)),
        seen: 1,
        start: shown[at]?.t,
    };
}

/** Each event's finding: its verdict and why. */
function judge(e, frames, names, later = frames) {
    const what = describe(e, names);
    if (e.kind === "lifeGain" || e.kind === "lifeLoss") {
        const b = lifeBehavior(frames, e.player);
        if (b.none)
            return { what, verdict: "still", why: "no life total shows it" };
        if (b.ms >= MIN_MS)
            return {
                what,
                verdict: "ok",
                why: `animated ${b.ms} ms`,
                start: b.start,
            };
        return {
            what,
            start: b.start,
            verdict: b.changed ? "instant" : "still",
            why: b.changed
                ? "the number changed without animation"
                : "the life total never changed",
        };
    }
    if (e.kind === "countersAdded" || e.kind === "countersRemoved") {
        return judgeBadge(what, frames, e.slot);
    }
    if (e.kind === "mana") {
        // Its motes flying from its source: the one that showed longest.
        const keys = new Set(
            frames.flatMap((f) =>
                Object.keys(f.motes ?? {}).filter((k) =>
                    k.startsWith(`${e.source}:`),
                ),
            ),
        );
        if (keys.size === 0) {
            return { what, verdict: "still", why: "no mana flew from it" };
        }
        const best = [...keys]
            .map((k) => behavior(frames, k, (f) => f.motes?.[k]))
            .reduce((a, b) => (b.ms > a.ms ? b : a));
        return judgeShown(what, best, "its mana");
    }
    if (e.kind === "ability") {
        // A new item on the stack shows it; a loyalty ability's cost shows on its badge.
        const item = newStackItem(frames);
        if (item) {
            return judgeShown(
                what,
                behavior(frames, item, (f) => f.stack?.[item]),
                "its stack item",
            );
        }
        if (e.loyalty !== null && e.loyalty !== undefined) {
            return judgeBadge(what, frames, e.item.source);
        }
        // An item announced a view after it reached the stack (a trigger put there while the
        // view before showed its cause): its frame showed it then.
        const named = e.item.name ?? "";
        const held = Object.keys(frames[0]?.stack ?? {}).some((k) => {
            const shown = k.slice(k.indexOf(":") + 1);
            return (
                shown && (named.startsWith(shown) || shown.startsWith(named))
            );
        });
        if (held && named) {
            return {
                what,
                verdict: "ok",
                why: "on the stack since an earlier view",
                start: frames[0]?.t,
            };
        }
    }
    // Damage to a player shows on their life total, whatever its source does.
    if (e.kind === "damage" && e.to.kind === "player") {
        const b = lifeBehavior(frames, e.to.player);
        if (!b.none && b.ms >= MIN_MS) {
            return {
                what,
                verdict: "ok",
                why: `life animated ${b.ms} ms`,
                start: b.start,
            };
        }
    }
    if (e.kind === "draw") {
        // The cards drawn: those whose element arrived in a hand over the view or a beat
        // after it, even if they left it again (a card drawn and discarded).
        const before = later[0]?.cards ?? {};
        const drawn = new Set();
        for (const f of later) {
            for (const [s, c] of Object.entries(f.cards)) {
                if (c.zone === "hand" && before[s]?.zone !== "hand")
                    drawn.add(s);
            }
        }
        if (drawn.size === 0) {
            return { what, verdict: "still", why: "no card arrived in a hand" };
        }
        return judgeSlots(what, later, [...drawn]);
    }
    // A spell's cast announced a view after its card reached the stack: the card showed it
    // then, as that view's zone change; a nudge since (the stack making room) is no cast.
    if (e.kind === "cast") {
        const first = frames.find((f) => f.cards[e.slot])?.cards[e.slot];
        if (first?.zone === "stack") {
            return {
                what,
                verdict: "ok",
                why: "on the stack since an earlier view",
                start: frames[0]?.t,
            };
        }
    }
    const slots = subjects(e);
    if (slots.length === 0) {
        return { what, verdict: "still", why: "no element is matched to it" };
    }
    return judgeSlots(what, frames, slots);
}

/** The stack item that appeared over `frames`, if any: one the first frame has not. */
function newStackItem(frames) {
    const before = new Set(Object.keys(frames[0]?.stack ?? {}));
    for (const f of frames) {
        const key = Object.keys(f.stack ?? {}).find((k) => !before.has(k));
        if (key) return key;
    }
    return undefined;
}

/** How a card's counter badges showed a change over `frames`. */
function judgeBadge(what, frames, slot) {
    const shown = frames.filter((f) => f.cards[slot]);
    if (shown.length === 0) {
        return {
            what,
            verdict: "still",
            why: "no element on the table shows it",
        };
    }
    const changed = shown.some(
        (f) => f.cards[slot].badge !== shown[0].cards[slot].badge,
    );
    if (!changed) {
        return { what, verdict: "still", why: "its counters never changed" };
    }
    const ms = Math.round(
        Math.max(
            span(
                shown,
                (a, b) => a.cards[slot].badgeAnim && b.cards[slot].badgeAnim,
            ),
            span(shown, (a, b) => a.cards[slot].anim && b.cards[slot].anim),
        ),
    );
    const at = shown.findIndex(
        (f, i) =>
            i > 0 && f.cards[slot].badge !== shown[i - 1].cards[slot].badge,
    );
    const start = shown[Math.max(0, at - 1)]?.t;
    return ms >= MIN_MS
        ? { what, verdict: "ok", why: `its counters animated ${ms} ms`, start }
        : {
              what,
              verdict: "instant",
              why: "its counters changed without animation",
              start,
          };
}

/** The verdict on one element's `behavior`, named `name`. */
function judgeShown(what, b, name) {
    if (b.none) return { what, verdict: "still", why: `${name} never showed` };
    if (!b.changed)
        return { what, verdict: "still", why: `${name} never changed` };
    // A `ui` finding names its element in `what`: its why starts with how it moved.
    const lead = name ? `${name} ` : "";
    const how = [
        b.appeared && "appeared",
        b.vanished && "vanished",
        b.moved >= MIN_PX && `moved ${b.moved}px`,
        b.turned && "turned",
    ]
        .filter(Boolean)
        .join(", ");
    const start = b.start;
    if (b.ms < MIN_MS) {
        return {
            what,
            verdict: "instant",
            why: `${lead}${how} in ${b.ms} ms`,
            start,
        };
    }
    if (b.seen < SEEN) {
        return {
            what,
            verdict: "unseen",
            why: `${lead}${how} over ${b.ms} ms, visible ${Math.round(b.seen * 100)}% of it`,
            start,
        };
    }
    return {
        what,
        verdict: "ok",
        why: `${lead}${how} over ${b.ms} ms`,
        start,
    };
}

/** The best showing among the elements of `slots`. */
function judgeSlots(what, frames, slots) {
    const found = slots.map((s) => ({ s, b: behavior(frames, s) }));
    const shown = found.filter((x) => !x.b.none);
    if (shown.length === 0) {
        return {
            what,
            verdict: "still",
            why: "no element on the table shows it",
        };
    }
    const changed = shown.filter((x) => x.b.changed);
    if (changed.length === 0) {
        return { what, verdict: "still", why: "its element never changed" };
    }
    const best = changed.reduce((a, x) => (x.b.ms > a.b.ms ? x : a));
    const how = [
        best.b.appeared && "appeared",
        best.b.vanished && "vanished",
        best.b.moved >= MIN_PX && `moved ${best.b.moved}px`,
        best.b.turned && "turned",
        best.b.zone && !best.b.appeared && !best.b.vanished && "changed zone",
    ]
        .filter(Boolean)
        .join(", ");
    // When it first showed: the earliest change of any of its elements.
    const start = Math.min(
        ...changed.map((x) => x.b.start).filter((t) => t !== undefined),
    );
    const at = Number.isFinite(start) ? start : undefined;
    if (best.b.ms < MIN_MS) {
        return {
            what,
            verdict: "instant",
            why: `${how} in ${best.b.ms} ms`,
            start: at,
        };
    }
    if (best.b.seen < SEEN) {
        return {
            what,
            verdict: "unseen",
            why: `${how} over ${best.b.ms} ms, visible ${Math.round(best.b.seen * 100)}% of it`,
            start: at,
        };
    }
    return {
        what,
        verdict: "ok",
        why: `${how} over ${best.b.ms} ms`,
        start: at,
    };
}

/** The slots a view's events account for: their subjects, and with a draw, every card that
 * arrived in a hand. */
function claimed(events, frames) {
    const out = new Set();
    for (const e of events) {
        for (const s of subjects(e)) out.add(String(s));
        if (e.kind === "damage") out.add(String(e.source));
    }
    if (events.some((e) => e.kind === "draw")) {
        const before = frames[0]?.cards ?? {};
        for (const f of frames) {
            for (const [s, c] of Object.entries(f.cards)) {
                if (c.zone === "hand" && before[s]?.zone !== "hand") out.add(s);
            }
        }
    }
    return out;
}

/** The `ui` findings of one window: each card that changed with no event of the view to
 * account for it (a hand rising, a fan making room), and each other element `record`'s `ui`
 * selects that appeared, vanished or moved. */
function uiFindings(window, events) {
    const out = [];
    const accounted = claimed(events, window);
    const slots = new Set(window.flatMap((f) => Object.keys(f.cards)));
    for (const slot of slots) {
        if (accounted.has(slot)) continue;
        const b = behavior(window, slot);
        if (b.none || !b.changed) continue;
        const name = window.find((f) => f.cards[slot])?.cards[slot];
        const label = `${name?.name ?? `#${slot}`} (${name?.zone ?? "?"})`;
        out.push(judgeShown(label, b, ""));
    }
    const keys = new Set(window.flatMap((f) => Object.keys(f.uis ?? {})));
    for (const key of keys) {
        const b = behavior(window, key, (f) => f.uis?.[key]);
        if (b.none || !b.changed) continue;
        const label = window.find((f) => f.uis?.[key])?.uis[key].label ?? "";
        out.push(judgeShown(`${key.replace(/ #\d+$/, "")} «${label}»`, b, ""));
    }
    return out;
}

/** Each arrow that took a new path while its ends stood still (`jump`), or was drawn while an
 * end moved (`stretch`), over `frames`: one finding for each arrow and verdict, counting the
 * frames it happened in. */
function arrowFindings(frames) {
    const runs = {};
    for (let i = 1; i < frames.length; i++) {
        for (const [key, now] of Object.entries(frames[i].arrows ?? {})) {
            const was = frames[i - 1].arrows?.[key];
            if (!was || was.d === now.d) continue;
            const verdict = was.ends === now.ends ? "jump" : "stretch";
            const run = (runs[`${verdict} ${key}`] ??= {
                verdict,
                key,
                start: frames[i].t,
                count: 0,
            });
            run.count++;
        }
    }
    return Object.values(runs).map(({ verdict, key, start, count }) => ({
        kind: "arrow",
        what: `arrow ${key}`,
        verdict,
        why:
            verdict === "jump"
                ? `rerouted ${count} time${count > 1 ? "s" : ""} with its ends still`
                : `redrawn over ${count} frame${count > 1 ? "s" : ""} as an end moved`,
        start,
    }));
}

/** Each card shown twice at once for DUPLICATE_MS or more, over `frames`. */
function duplicates(frames) {
    const runs = {};
    const out = [];
    const close = (name, end) => {
        const run = runs[name];
        delete runs[name];
        if (end - run.from >= DUPLICATE_MS) {
            out.push({
                kind: "duplicate",
                what: `${name} shown twice: ${run.where}`,
                verdict: "duplicate",
                why: `two copies on screen for ${Math.round(end - run.from)} ms`,
                start: run.from,
            });
        }
    };
    for (const f of frames) {
        for (const [name, where] of Object.entries(f.dupes ?? {})) {
            runs[name] ??= { from: f.t, where };
        }
        for (const name of Object.keys(runs)) {
            if (!f.dupes?.[name]) close(name, f.t);
        }
    }
    for (const name of Object.keys(runs)) close(name, frames.at(-1).t);
    return out;
}

/** Each finding of one seat's run: per view, each event's, its `ui` findings and a crowd,
 * then each duplicate. With `live`, an action of the viewer's set the run going (a click, not
 * a scripted play): what it moved before the first view counts as a view of its own, and `ui`
 * findings are not minor. Each finding has `at`, ms from the recording's start to when it
 * first showed, where it showed at all. */
export function audit({ frames, shown, since }, { live = false } = {}) {
    const findings = [];
    const origin = since ?? frames[0]?.t ?? 0;
    const views = [...shown];
    if (live) {
        views.unshift({
            at: origin,
            turn: "the action",
            events: [],
            names: {},
        });
    }
    for (let i = 0; i < views.length; i++) {
        const view = views[i];
        // Until the next view, or long enough for an animation that outlasts a short hold.
        const end = Math.max(
            views[i + 1]?.at ?? frames.at(-1)?.t ?? view.at,
            view.at + 1000,
        );
        // From a frame before the view, for where things were, to the end: the page draws a
        // view a few ms before its effect logs it, so the last frame before `at` may show it.
        const from = Math.max(
            0,
            frames.findIndex((f) => f.t >= view.at - LEAD) - 1,
        );
        const window = frames.slice(from).filter((f) => f.t <= end);
        const later = frames.slice(from).filter((f) => f.t <= end + DRAW_GRACE);
        const events = view.events.filter((e) => !QUIET_EVENTS.has(e.kind));
        for (const e of events) {
            findings.push({
                view: view.turn,
                kind: e.kind,
                minor: e.kind === "tap" || e.kind === "untap",
                ...judge(e, window, view.names, later),
            });
        }
        if (window.length > 1) {
            for (const f of uiFindings(window, events)) {
                findings.push({
                    view: view.turn,
                    kind: "ui",
                    minor: !live,
                    ...f,
                });
            }
        }
        // Cards making the same move together (a mill, a graveyard exiled) read as one move:
        // the game shows them as a batch (beats.ts's showcase).
        const moves = new Set();
        const counted = events.filter((e) => {
            if (MINOR_EVENTS.has(e.kind)) return false;
            if (e.kind !== "zoneChange") return true;
            const move = `${e.from}>${e.to}:${e.reason}`;
            if (moves.has(move)) return false;
            moves.add(move);
            return true;
        });
        if (counted.length > CROWDED) {
            findings.push({
                view: view.turn,
                what: counted.map((e) => describe(e, view.names)).join("; "),
                verdict: "crowded",
                why: `${counted.length} events shown at once, held ${Math.round((views[i + 1]?.at ?? end) - view.at)} ms`,
                start: view.at,
            });
        }
    }
    for (const d of [...duplicates(frames), ...arrowFindings(frames)]) {
        const view = views.findLast((v) => v.at - LEAD <= d.start) ?? views[0];
        findings.push({ view: view?.turn ?? "", ...d });
    }
    return findings.map(({ start, ...f }) => ({
        ...f,
        ...(start === undefined ? {} : { at: Math.round(start - origin) }),
    }));
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
