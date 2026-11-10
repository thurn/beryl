// Mana flowing from what made it to what it pays for, as Arena shows it: a glowing mote for
// each unit of mana, in its color, trailing light along a slight curve from its source (the
// land, mana rock or creature tapped, the spell that added it) into the spell or ability it was
// spent on, landing there as the card reaches the stack. Mana nothing was cast with in the same
// view (tapped ahead, or added by an effect) flows into its player's pool instead, and mana
// spent from the pool flows out of it. It reads the `mana` events every mana ability's
// activation makes, so every source and every payment gets it, from both seats.
import { useLayoutEffect, useRef } from "react";
import { useReducedMotion } from "motion/react";
import { CARD_ASPECT } from "../components/Card";
import type {
  GameEvent,
  Mana,
  PlayerView,
  Side,
  StackItem,
} from "../generated/protocol";
import { sameItem } from "./beats";
import {
  footprint,
  MANA_POOLS,
  stackPlace,
  type Layout,
  type Placement,
} from "./layout";

interface Point {
  x: number;
  y: number;
}

/** One unit of mana's flight, in stage pixels and ms. */
export interface Mote {
  color: Mana;
  from: Point;
  to: Point;
  delay: number;
  /** The slot of what made it, or "pool" for mana spent from a pool: what the motion audit
   * matches the mote to its event by. */
  source: number | "pool";
}

/** A mote's flight, and how long after one mote the next sets off, up to `STAGGER_MAX` for the
 * last of a payment: seven lands' mana lands within about 650 ms. */
const FLIGHT_MS = 420;
const STAGGER_MS = 45;
const STAGGER_MAX = 220;
/** How many motes one event's mana makes at most: a source of much mana flows as a few. */
const MOTES_MAX = 4;
/** Sparks shed behind each mote's tail: this many, each this much later. */
const SPARKS = 3;
const SPARK_LAG_MS = 28;
/** The points a flight's keyframes sample its curve at. */
const SAMPLES = 14;
/** The curve bows sideways by this share of its length. */
const BOW = 0.18;

/** Each kind of mana's light: a pale core and a deeper glow. */
const LIGHT: Record<Mana, { core: string; glow: string }> = {
  white: { core: "#fffbe6", glow: "#f5d76e" },
  blue: { core: "#e3f6ff", glow: "#3fa9f5" },
  black: { core: "#f0e2ff", glow: "#9a5cd6" },
  red: { core: "#ffe9dc", glow: "#f2552c" },
  green: { core: "#e8ffe6", glow: "#3fbf4f" },
  colorless: { core: "#ffffff", glow: "#b9b4c9" },
};

const KINDS: Mana[] = ["white", "blue", "black", "red", "green", "colorless"];

function center(p: Placement): Point {
  const box = footprint(p);
  return { x: (box.left + box.right) / 2, y: (box.top + box.bottom) / 2 };
}

/** Where a player's pool shows its mana. */
function poolPoint(side: Side): Point {
  const { left, top } = MANA_POOLS[side];
  return { x: left + 24, y: top + 17 };
}

/** The card a spell or ability shows as on the stack, as `view` lays the stack out. */
function stackBox(view: PlayerView, item: StackItem) {
  const index = view.stack.findIndex((s) => sameItem(s, item));
  if (index < 0) return undefined;
  const place = stackPlace(index, view.stack.length);
  return { ...place, height: place.width / CARD_ASPECT };
}

/** A point spread across the middle of a card, the `n`th of a payment's motes. */
function onCard(
  box: { x: number; y: number; width: number; height: number },
  n: number,
): Point {
  // Golden-angle spread: motes land apart without looking random from view to view.
  const angle = n * 2.399963;
  const r = Math.sqrt((n % 7) / 7);
  return {
    x: box.x + box.width / 2 + Math.cos(angle) * r * box.width * 0.22,
    y: box.y + box.height * 0.42 + Math.sin(angle) * r * box.height * 0.16,
  };
}

/** The spell or ability the mana of `events[i]` was spent on: the next cast or activation by
 * the same player in the view, which costs are paid for before it is announced
 * ([CR 601.2h](https://yawgatog.com/resources/magic-rules/#R6012h)). */
function spentOn(
  events: GameEvent[],
  i: number,
  player: Side,
): StackItem | undefined {
  for (const e of events.slice(i + 1)) {
    if (e.kind === "resolutionPayment" && e.player === player) return e.item;
    if (e.kind === "cast" && e.controller === player) return e.item;
    if (
      e.kind === "ability" &&
      e.item.kind === "ability" &&
      e.item.controller === player
    ) {
      return e.item;
    }
  }
  return undefined;
}

/** The motes `view`'s events make, after `before`, the view shown before it, with `placements`
 * its table and `previous` the table before (where a source sacrificed for its mana was). */
export function manaMotes(
  before: PlayerView | undefined,
  view: PlayerView,
  placements: Layout,
  previous: Layout | undefined,
): Mote[] {
  if (view.undone) return [];
  const { events } = view;
  const out: Mote[] = [];
  const making = (v: PlayerView | undefined) =>
    v?.status.kind === "deciding" &&
    (v.status.decision.kind === "manaPayment" ||
      (v.status.decision.cancellable && v.status.decision.kind !== "priority"));
  // A cost chooser can follow payment before a Cast event exists. Bind the motes to that
  // pending item, never to a different spell that becomes topmost after it resolves.
  const pending = making(view)
    ? view.stack.at(-1)
    : making(before)
      ? before?.stack.at(-1)
      : undefined;
  const paying = view.stack.find(
    (item) =>
      (pending && sameItem(item, pending)) ||
      (item.kind === "spell" &&
        events.some(
          (e) =>
            e.kind === "zoneChange" &&
            e.slot === item.slot &&
            e.to === "stack" &&
            e.reason === "cast",
        )),
  );
  const payingFor = (side: Side) => {
    const controller =
      paying?.kind === "spell"
        ? view.objects.find((o) => o.slot === paying.slot)?.controller
        : paying?.controller;
    return controller === side ? paying : undefined;
  };
  const destination = (item: StackItem | undefined, n: number, side: Side) => {
    if (item) {
      const box = stackBox(view, item);
      if (box) return onCard(box, n);
      // Resolved by the view's end: its card where it went.
      const placed = item.kind === "spell" && placements.get(item.slot);
      if (placed) return center(placed);
    }
    return poolPoint(side);
  };
  const origin = (slot: number | null): Point | undefined => {
    if (slot === null) return undefined;
    const now = placements.get(slot);
    if (now?.zone === "battlefield") return center(now);
    const was = previous?.get(slot);
    if (was) return center(was);
    return now && center(now);
  };
  const add = (mote: Omit<Mote, "delay">) => {
    const delay = Math.min(out.length * STAGGER_MS, STAGGER_MAX);
    out.push({ ...mote, delay });
  };
  events.forEach((e, i) => {
    if (e.kind !== "mana") return;
    const from = origin(e.source);
    if (!from || e.source === null) return;
    const item = spentOn(events, i, e.player) ?? payingFor(e.player);
    for (let k = 0; k < Math.min(e.amount, MOTES_MAX); k++) {
      add({
        color: e.color,
        from,
        to: destination(item, out.length, e.player),
        source: e.source,
      });
    }
  });
  // Mana spent from a pool, unless a step ended and emptied it (CR 500.4).
  if (before && !events.some((e) => e.kind === "stepBegins")) {
    for (const side of ["you", "opponent"] as const) {
      const index = events.findIndex(
        (e) =>
          (e.kind === "resolutionPayment" && e.player === side) ||
          (e.kind === "cast" && e.controller === side) ||
          (e.kind === "ability" &&
            e.item.kind === "ability" &&
            e.item.controller === side),
      );
      const item =
        index < 0 ? payingFor(side) : spentOn(events, index - 1, side);
      if (!item) continue;
      for (const color of KINDS) {
        const spent =
          before.players[side].mana[color] - view.players[side].mana[color];
        for (let k = 0; k < Math.min(spent, MOTES_MAX); k++) {
          add({
            color,
            from: poolPoint(side),
            to: destination(item, out.length, side),
            source: "pool",
          });
        }
      }
    }
  }
  return out;
}

/** A point `t` of the way along the quadratic curve from `a` to `b` bowed by `bow`. */
function along(a: Point, b: Point, bow: number, t: number): Point {
  const dx = b.x - a.x;
  const dy = b.y - a.y;
  const c = {
    x: (a.x + b.x) / 2 - dy * bow,
    y: (a.y + b.y) / 2 + dx * bow,
  };
  const u = 1 - t;
  return {
    x: u * u * a.x + 2 * u * t * c.x + t * t * b.x,
    y: u * u * a.y + 2 * u * t * c.y + t * t * b.y,
  };
}

/** Numbers the flights, for the motion audit to follow each mote. */
let flights = 0;

/** Sets `mote` flying in `layer` once its delay is up, as a head and its trail, and a flare
 * where it lands. Made only as it sets off: nothing waits on the table unseen. */
function fly(layer: HTMLElement, mote: Mote, n: number) {
  setTimeout(() => launch(layer, mote, n), mote.delay);
}

function launch(layer: HTMLElement, mote: Mote, n: number) {
  const light = LIGHT[mote.color];
  // Alternate sides, so a payment's motes fan out rather than queue on one line.
  const bow = (n % 2 === 0 ? 1 : -1) * BOW * (0.6 + ((n * 37) % 10) / 25);
  const path = Array.from({ length: SAMPLES + 1 }, (_, i) =>
    along(mote.from, mote.to, bow, i / SAMPLES),
  );
  // The heading at each point, unwound so that no turn between two goes the long way round.
  const headings: number[] = [];
  path.forEach((_, i) => {
    const a = path[Math.max(0, i - 1)]!;
    const b = path[Math.min(SAMPLES, i + 1)]!;
    let angle = Math.atan2(b.y - a.y, b.x - a.x);
    const last = headings.at(-1);
    if (last !== undefined) {
      while (angle - last > Math.PI) angle -= 2 * Math.PI;
      while (last - angle > Math.PI) angle += 2 * Math.PI;
    }
    headings.push(angle);
  });
  const timing: KeyframeAnimationOptions = {
    duration: FLIGHT_MS,
    easing: "cubic-bezier(0.5, 0.05, 0.55, 1)",
  };
  const made: HTMLElement[] = [];
  const add = (className: string) => {
    const el = document.createElement("div");
    el.className = className;
    el.style.opacity = "0";
    layer.append(el);
    made.push(el);
    return el;
  };

  // The tail: a streak of light behind the head, turning with the curve, longest where the
  // mote flies fastest.
  const tail = add("mana-tail");
  tail.style.background = `linear-gradient(to left, ${light.core}, ${light.glow} 25%, ${light.glow}00)`;
  tail.style.boxShadow = `0 0 8px 1px ${light.glow}88`;
  tail.animate(
    path.map((p, i) => {
      const t = i / SAMPLES;
      return {
        transform: `translate(${p.x}px, ${p.y}px) rotate(${headings[i]}rad) scaleX(${0.15 + 0.85 * Math.sin(Math.PI * t) ** 0.6})`,
        opacity: i === 0 || i === SAMPLES ? 0 : 0.9,
        offset: t,
      };
    }),
    { ...timing, fill: "both" },
  );

  // Sparks shed behind it, a little off its line.
  for (let k = 1; k <= SPARKS; k++) {
    const spark = add("mana-spark");
    spark.style.background = light.core;
    spark.style.boxShadow = `0 0 6px 2px ${light.glow}`;
    const off = (k % 2 === 0 ? 1 : -1) * (3 + k * 2);
    spark.animate(
      path.map((p, i) => {
        const h = headings[i]! + Math.PI / 2;
        const drift = off * (i / SAMPLES);
        return {
          transform: `translate(${p.x + Math.cos(h) * drift}px, ${p.y + Math.sin(h) * drift}px) scale(${1 - k * 0.2})`,
          opacity: i === 0 || i === SAMPLES ? 0 : 0.8 - k * 0.15,
          offset: i / SAMPLES,
        };
      }),
      { ...timing, delay: k * SPARK_LAG_MS, fill: "both" },
    );
  }

  // The head, lit from its source to where it lands, and gone as it lands, into the flare.
  const head = add("mana-mote");
  head.dataset.mana = mote.color;
  head.dataset.manaSource = String(mote.source);
  head.dataset.manaId = String(++flights);
  head.style.background = `radial-gradient(circle, ${light.core} 0 35%, ${light.glow} 70%, transparent 74%)`;
  head.style.boxShadow = `0 0 12px 5px ${light.glow}, 0 0 32px 12px ${light.glow}99`;
  const flight = head.animate(
    path.map((p, i) => ({
      transform: `translate(${p.x}px, ${p.y}px)`,
      opacity: 1,
      offset: i / SAMPLES,
    })),
    timing,
  );
  const land = () => head.remove();
  flight.finished.then(land, land);

  const flare = document.createElement("div");
  flare.className = "mana-flare";
  flare.style.background = `radial-gradient(circle, ${light.core} 0 18%, ${light.glow}dd 42%, transparent 70%)`;
  layer.append(flare);
  made.push(flare);
  const at = `translate(${mote.to.x}px, ${mote.to.y}px)`;
  const flash = flare.animate(
    [
      { transform: `${at} scale(0.3)`, opacity: 0 },
      { transform: `${at} scale(1)`, opacity: 0.9, offset: 0.3 },
      { transform: `${at} scale(1.7)`, opacity: 0 },
    ],
    {
      duration: 260,
      delay: FLIGHT_MS - 40,
      easing: "ease-out",
      fill: "both",
    },
  );
  const done = () => made.forEach((m) => m.remove());
  flash.finished.then(done, done);
}

/** The payments already set flying: a remount (StrictMode's second) flies none twice. */
const FLOWN = new WeakSet<Mote[]>();

/** The motes of the view shown, flying over the table. A view replaced before its motes land
 * leaves them flying: they belong to what was paid, not to the view. */
export function ManaFlow({ motes }: { motes: Mote[] }) {
  const layer = useRef<HTMLDivElement>(null);
  const reduce = useReducedMotion() ?? false;
  useLayoutEffect(() => {
    const el = layer.current;
    if (!el || reduce || FLOWN.has(motes)) return;
    FLOWN.add(motes);
    motes.forEach((m, n) => fly(el, m, n));
  }, [motes, reduce]);
  return <div ref={layer} className="mana-flow" aria-hidden />;
}
