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
// A finding about a tap or untap is `minor`: mana bookkeeping, which a land in a group shows
// under its neighbours.

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
const QUIET_EVENTS = new Set(["stepBegins", "coinFlip", "win", "lose"]);
/** Events that come with another and count for nothing in a crowd. */
const MINOR_EVENTS = new Set(["tap", "untap", "draw", "cast"]);

/** Starts recording every frame in the page. Run with `page.evaluate`. */
export function record() {
    const opacity = (e) => {
        let o = 1;
        for (let x = e; x && x !== document.body; x = x.parentElement) {
            o *= Number(getComputedStyle(x).opacity);
            if (o === 0) break;
        }
        return Math.round(o * 100) / 100;
    };
    // Hit testing follows paint order only where everything takes the pointer: a card in
    // flight, or the layer it flies in, may not. Nothing is clicked while the audit watches.
    const hits = document.createElement("style");
    hits.textContent = "* { pointer-events: auto !important; }";
    document.head.append(hits);
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
    const frames = [];
    window.__motion = { frames, stop: false };
    const frame = () => {
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
        const stack = document.querySelectorAll(".stack-item").length;
        frames.push({ t: performance.now(), cards, lives, stack });
        if (!window.__motion.stop) requestAnimationFrame(frame);
    };
    requestAnimationFrame(frame);
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
    return { frames: window.__motion.frames, shown };
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
            return e.item.source === undefined ? [] : [e.item.source];
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
        case "countersAdded":
        case "countersRemoved":
            return `${name(e.slot)} ${e.kind === "countersAdded" ? "+" : "-"}${e.amount} ${e.counter}`;
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
function behavior(frames, slot) {
    const track = frames.map((f) => ({ t: f.t, c: f.cards[slot] }));
    const present = track.filter((x) => x.c);
    if (present.length === 0) return { none: true };
    const first = track[0].c;
    const last = track.at(-1).c;
    const appeared = !first && !!last;
    const vanished = !!first && !last;
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
    return { changed, ms: Math.round(Math.max(anim, counting)), seen: 1 };
}

/** Each event's finding: its verdict and why. */
function judge(e, frames, names) {
    const what = describe(e, names);
    if (e.kind === "lifeGain" || e.kind === "lifeLoss") {
        const b = lifeBehavior(frames, e.player);
        if (b.none)
            return { what, verdict: "still", why: "no life total shows it" };
        if (b.ms >= MIN_MS)
            return { what, verdict: "ok", why: `animated ${b.ms} ms` };
        return {
            what,
            verdict: b.changed ? "instant" : "still",
            why: b.changed
                ? "the number changed without animation"
                : "the life total never changed",
        };
    }
    // Damage to a player shows on their life total, whatever its source does.
    if (e.kind === "damage" && e.to.kind === "player") {
        const b = lifeBehavior(frames, e.to.player);
        if (!b.none && b.ms >= MIN_MS) {
            return { what, verdict: "ok", why: `life animated ${b.ms} ms` };
        }
    }
    if (e.kind === "draw") {
        // The cards drawn: those whose element arrived in a hand over the view.
        const before = frames[0]?.cards ?? {};
        const drawn = Object.keys(frames.at(-1)?.cards ?? {}).filter(
            (s) =>
                frames.at(-1).cards[s].zone === "hand" &&
                before[s]?.zone !== "hand",
        );
        if (drawn.length === 0) {
            return { what, verdict: "still", why: "no card arrived in a hand" };
        }
        return judgeSlots(what, frames, drawn);
    }
    const slots = subjects(e);
    if (slots.length === 0) {
        return { what, verdict: "still", why: "no element is matched to it" };
    }
    return judgeSlots(what, frames, slots);
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
    if (best.b.ms < MIN_MS) {
        return { what, verdict: "instant", why: `${how} in ${best.b.ms} ms` };
    }
    if (best.b.seen < SEEN) {
        return {
            what,
            verdict: "unseen",
            why: `${how} over ${best.b.ms} ms, visible ${Math.round(best.b.seen * 100)}% of it`,
        };
    }
    return { what, verdict: "ok", why: `${how} over ${best.b.ms} ms` };
}

/** Each finding of one seat's run: per view, each event's, and a crowd. */
export function audit({ frames, shown }) {
    const findings = [];
    for (let i = 0; i < shown.length; i++) {
        const view = shown[i];
        // Until the next view, or long enough for an animation that outlasts a short hold.
        const end = Math.max(
            shown[i + 1]?.at ?? frames.at(-1)?.t ?? view.at,
            view.at + 1000,
        );
        // From a frame before the view, for where things were, to the end: the page draws a
        // view a few ms before its effect logs it, so the last frame before `at` may show it.
        const from = Math.max(
            0,
            frames.findIndex((f) => f.t >= view.at - LEAD) - 1,
        );
        const window = frames.slice(from).filter((f) => f.t <= end);
        const events = view.events.filter((e) => !QUIET_EVENTS.has(e.kind));
        for (const e of events) {
            findings.push({
                view: view.turn,
                kind: e.kind,
                minor: e.kind === "tap" || e.kind === "untap",
                ...judge(e, window, view.names),
            });
        }
        const counted = events.filter((e) => !MINOR_EVENTS.has(e.kind));
        if (counted.length > CROWDED) {
            findings.push({
                view: view.turn,
                what: counted.map((e) => describe(e, view.names)).join("; "),
                verdict: "crowded",
                why: `${counted.length} events shown at once, held ${Math.round((shown[i + 1]?.at ?? end) - view.at)} ms`,
            });
        }
    }
    return findings;
}
