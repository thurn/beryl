// Routing for the arrows that tie a stack item to its source and targets: each picks, among
// curves leaving one element's edge square to it and arriving square to the other's, the one
// that crosses the fewest other cards, stays readable between neighbors, and keeps clear of
// the arrows routed before it.

export type Vector = readonly [number, number];

export interface Box {
  x: number;
  y: number;
  width: number;
  height: number;
}

/** A point on an element's edge an arrow may leave or meet it at, and the edge's outward
 * normal there, a unit vector. */
interface Port {
  at: Vector;
  normal: Vector;
  /** How far it lies from the middle of its side, as a fraction of the side: arrows prefer
   * the middle, but spread along a side when another arrow uses it. */
  off: number;
}

/** A box's ports lie along the middle `SPREAD` of each side, an odd number of them, at least
 * three, more on a long side: no closer than `SPACING` pixels apart. */
const SPREAD = 0.6;
const SPACING = 40;

/** Where an arrow may meet `b`: ports along each side of a box, or for an `oval`, twelve round
 * the ellipse inscribed in it. */
function ports(b: Box, oval: boolean): Port[] {
  const [cx, cy] = [b.x + b.width / 2, b.y + b.height / 2];
  if (oval) {
    const [rx, ry] = [b.width / 2, b.height / 2];
    return Array.from({ length: 12 }, (_, i) => {
      const a = (i * Math.PI) / 6;
      const [c, s] = [Math.cos(a), Math.sin(a)];
      // The ellipse's gradient there, normalized, is its outward normal.
      const [gx, gy] = [c / rx, s / ry];
      const g = Math.hypot(gx, gy);
      return {
        at: [cx + rx * c, cy + ry * s],
        normal: [gx / g, gy / g],
        off: i % 3 === 0 ? 0 : 0.25,
      };
    });
  }
  /** Fractions of a side of `length`, with how far each is off its middle. */
  const along = (length: number) => {
    const n = Math.max(1, Math.floor((length * SPREAD) / SPACING / 2));
    return Array.from({ length: 2 * n + 1 }, (_, i) => {
      const off = ((i - n) / n) * (SPREAD / 2);
      return { f: 0.5 + off, off: Math.abs(off) };
    });
  };
  const { x, y, width: w, height: h } = b;
  return [
    ...along(w).flatMap(({ f, off }): Port[] => [
      { at: [x + w * f, y], normal: [0, -1], off },
      { at: [x + w * f, y + h], normal: [0, 1], off },
    ]),
    ...along(h).flatMap(({ f, off }): Port[] => [
      { at: [x, y + h * f], normal: [-1, 0], off },
      { at: [x + w, y + h * f], normal: [1, 0], off },
    ]),
  ];
}

/** One end of an arrow to route: the element's box and whether it is round. */
export interface End {
  box: Box;
  oval?: boolean;
}

/** What routing weighs, in pixels of length: a pixel over another card costs this many. */
const OVER_CARD = 5;
/** ...a pixel back over or close alongside the arrow's own source or target (within `HUG`
 * of either, once the curve is a `LEAVING` fraction of the way from the one or still that far
 * from the other), or off the table, this many. */
const OVER_ENDS = 12;
const HUG = 10;
const LEAVING = 0.1;
/** ...a pixel within `NEAR` of an earlier arrow, this many, and crossing one this many. */
const ALONGSIDE = 3;
const NEAR = 18;
const CROSSING = 300;
/** An arrow shorter than this is too short to read between two cards side by side: each
 * pixel short costs this many. */
const SHORTEST = 110;
const SHORT = 8;
/** Ending within `CROWD` of an earlier arrow's end, as two arrows on one side of the stack
 * item would, costs this much. */
const CROWD = 30;
const CROWDED = 400;
/** A port away from the middle of its side costs this much per side's length off it. */
const OFF_MIDDLE = 60;
/** Up to this many arrows are routed in every order, and the cheapest kept: routed first,
 * one can take the only clear way out of the stack item from another. */
const REORDER = 3;
/** How far each control point lies from its end, as a fraction of the distance between the
 * ends: flatter, tighter or rounder curves. The flattest lets a long arrow run along the
 * narrow gap between a row of cards and what lies above or below it. */
const REACH = [0.1, 0.3, 0.55];
/** A new route replaces the one an arrow has only when it costs less than this `share` of it
 * less `by`: clearly better, not just a little after something moved. */
const STAY = { share: 0.8, by: 150 };
/** How many points along a curve price it: roughly, to shortlist the `SHORTLIST` likeliest
 * routes for an arrow, then finely, against the other arrows. */
const COARSE = 10;
const SHORTLIST = 30;
const FINE = 32;

/** The cubic through `p` at `t`. */
function bezier(p: readonly Vector[], t: number): Vector {
  const u = 1 - t;
  const [a, b, c, d] = [u * u * u, 3 * u * u * t, 3 * u * t * t, t * t * t];
  return [
    a * p[0]![0] + b * p[1]![0] + c * p[2]![0] + d * p[3]![0],
    a * p[0]![1] + b * p[1]![1] + c * p[2]![1] + d * p[3]![1],
  ];
}

const inside = (b: Box, [x, y]: Vector, margin: number) =>
  x > b.x - margin &&
  x < b.x + b.width + margin &&
  y > b.y - margin &&
  y < b.y + b.height + margin;

const overlaps = (a: Box, b: Box) =>
  a.x < b.x + b.width &&
  b.x < a.x + a.width &&
  a.y < b.y + b.height &&
  b.y < a.y + a.height;

/** Whether segment `a`–`b` crosses segment `c`–`d`. */
function crosses(a: Vector, b: Vector, c: Vector, d: Vector): boolean {
  const side = (p: Vector, q: Vector, r: Vector) =>
    Math.sign((q[0] - p[0]) * (r[1] - p[1]) - (q[1] - p[1]) * (r[0] - p[0]));
  return side(a, b, c) * side(a, b, d) < 0 && side(c, d, a) * side(c, d, b) < 0;
}

const grown = (b: Box, by: number): Box => ({
  x: b.x - by,
  y: b.y - by,
  width: b.width + 2 * by,
  height: b.height + 2 * by,
});

const point = ([x, y]: Vector) => `${x.toFixed(1)} ${y.toFixed(1)}`;

/** One arrow to route: its ends, and the boxes it keeps off (the other cards). */
export interface Leg {
  from: End;
  to: End;
  avoid: readonly Box[];
}

/** A routed arrow: its curve, where it starts and ends, and its heading at the end in
 * degrees. `choice` names the ports and reach it used, to follow them as its ends move. */
export interface Route {
  d: string;
  /** The cubic's control points, from start to end. */
  p: readonly Vector[];
  start: Vector;
  end: Vector;
  angle: number;
  choice: Choice;
  /** Points along the curve, which later arrows keep clear of, and the box round them. */
  samples: Vector[];
  bounds: Box;
  cost: number;
}

/** Which of its ends' ports a route leaves and meets, and how far it reaches. Each port is
 * also kept as where it lies on its end (`from`, `to`), since how many ports a side has, and
 * so which index names a port, changes as a card grows or shrinks. */
export interface Choice {
  start: number;
  end: number;
  reach: number;
  from?: Anchor;
  to?: Anchor;
}

/** Where a port lies on its box: as fractions of the box's width and height, and which way it
 * faces. */
interface Anchor {
  u: number;
  v: number;
  normal: Vector;
}

const anchorOf = (p: Port, b: Box): Anchor => ({
  u: b.width ? (p.at[0] - b.x) / b.width : 0.5,
  v: b.height ? (p.at[1] - b.y) / b.height : 0.5,
  normal: p.normal,
});

/** The index among `list`, the ports of `b`, of the port facing as `a` does nearest where it
 * lay; undefined if none faces that way. */
function anchored(
  list: readonly Port[],
  b: Box,
  a: Anchor,
): number | undefined {
  let found: number | undefined;
  let nearest = Infinity;
  list.forEach((p, i) => {
    if (p.normal[0] * a.normal[0] + p.normal[1] * a.normal[1] < 0.9) return;
    const at = anchorOf(p, b);
    const d = Math.hypot(at.u - a.u, at.v - a.v);
    if (d < nearest) [found, nearest] = [i, d];
  });
  return found;
}

/** The candidate `choice` names for `leg` now: its ports found by where they lay, or by index
 * for a choice that does not say; undefined if its ends no longer have them. */
function again(leg: Leg, choice: Choice): Candidate | undefined {
  const starts = ports(leg.from.box, leg.from.oval ?? false);
  const ends = ports(leg.to.box, leg.to.oval ?? false);
  const start = choice.from
    ? anchored(starts, leg.from.box, choice.from)
    : choice.start;
  const end = choice.to ? anchored(ends, leg.to.box, choice.to) : choice.end;
  const s = start === undefined ? undefined : starts[start];
  const e = end === undefined ? undefined : ends[end];
  if (!s || !e) return undefined;
  return candidate(s, e, { start: start!, end: end!, reach: choice.reach });
}

interface Candidate {
  choice: Choice;
  start: Port;
  end: Port;
  /** The cubic's control points. */
  p: Vector[];
}

/** The curve from port `s` to port `e` reaching `reach` of the way between them. */
function candidate(s: Port, e: Port, choice: Choice): Candidate {
  const chord = Math.hypot(e.at[0] - s.at[0], e.at[1] - s.at[1]);
  const handle = Math.max(40, Math.min(260, chord * REACH[choice.reach]!));
  return {
    choice,
    start: s,
    end: e,
    p: [
      s.at,
      [s.at[0] + s.normal[0] * handle, s.at[1] + s.normal[1] * handle],
      [e.at[0] + e.normal[0] * handle, e.at[1] + e.normal[1] * handle],
      e.at,
    ],
  };
}

/** What `c` costs as a route for `leg` on a `table`, judged at `count` points along it, and
 * those points. */
function price(
  leg: Leg,
  c: Candidate,
  earlier: readonly Route[],
  table: Box,
  count: number,
): { cost: number; samples: Vector[] } {
  // Only the cards near the curve's control polygon, which holds it, can lie under it.
  const xs = c.p.map((q) => q[0]);
  const ys = c.p.map((q) => q[1]);
  const hull = {
    x: Math.min(...xs),
    y: Math.min(...ys),
    width: Math.max(...xs) - Math.min(...xs),
    height: Math.max(...ys) - Math.min(...ys),
  };
  const near = leg.avoid.filter((b) => overlaps(b, hull));
  const lines = earlier.filter((r) => overlaps(grown(r.bounds, NEAR), hull));
  const samples = Array.from({ length: count + 1 }, (_, n) =>
    bezier(c.p, n / count),
  );
  let length = 0;
  let cost = OFF_MIDDLE * (c.start.off + c.end.off);
  for (let n = 1; n <= count; n++) {
    const [a, b] = [samples[n - 1]!, samples[n]!];
    const ds = Math.hypot(b[0] - a[0], b[1] - a[1]);
    const m: Vector = [(a[0] + b[0]) / 2, (a[1] + b[1]) / 2];
    const t = (n - 0.5) / count;
    length += ds;
    if (near.some((o) => inside(o, m, 4))) cost += ds * OVER_CARD;
    if (
      inside(leg.from.box, m, t > LEAVING ? HUG : -2) ||
      inside(leg.to.box, m, t < 1 - LEAVING ? HUG : -2) ||
      !inside(table, m, 0)
    ) {
      cost += ds * OVER_ENDS;
    }
    for (const line of lines) {
      // A segment can neither near nor cross a line whose box its middle is far from.
      if (!inside(line.bounds, m, NEAR + ds)) continue;
      const q = line.samples;
      const close = (p: Vector) =>
        (p[0] - m[0]) ** 2 + (p[1] - m[1]) ** 2 < NEAR ** 2;
      if (q.some(close)) cost += ds * ALONGSIDE;
      for (let i = 1; i < q.length; i++) {
        if (crosses(a, b, q[i - 1]!, q[i]!)) cost += CROSSING;
      }
    }
  }
  cost += length + Math.max(0, SHORTEST - length) * SHORT;
  for (const line of earlier) {
    for (const q of [line.start, line.end]) {
      for (const port of [c.start, c.end]) {
        if (Math.hypot(q[0] - port.at[0], q[1] - port.at[1]) < CROWD) {
          cost += CROWDED;
        }
      }
    }
  }
  return { cost, samples };
}

function drawn(leg: Leg, c: Candidate, cost: number, samples: Vector[]): Route {
  const xs = samples.map((q) => q[0]);
  const ys = samples.map((q) => q[1]);
  return {
    bounds: {
      x: Math.min(...xs),
      y: Math.min(...ys),
      width: Math.max(...xs) - Math.min(...xs),
      height: Math.max(...ys) - Math.min(...ys),
    },
    d: `M${point(c.p[0]!)} C${point(c.p[1]!)} ${point(c.p[2]!)} ${point(c.p[3]!)}`,
    p: c.p,
    start: c.start.at,
    end: c.end.at,
    angle: (Math.atan2(-c.end.normal[1], -c.end.normal[0]) * 180) / Math.PI,
    choice: {
      ...c.choice,
      from: anchorOf(c.start, leg.from.box),
      to: anchorOf(c.end, leg.to.box),
    },
    samples,
    cost,
  };
}

/** `leg`'s likeliest candidates on a `table`: all priced roughly as if it were the only arrow,
 * the cheapest `SHORTLIST` kept. */
function shortlist(leg: Leg, table: Box): Candidate[] {
  const starts = ports(leg.from.box, leg.from.oval ?? false);
  const ends = ports(leg.to.box, leg.to.oval ?? false);
  return starts
    .flatMap((s, start) =>
      ends.flatMap((e, end) =>
        REACH.map((_, reach) => candidate(s, e, { start, end, reach })),
      ),
    )
    .map((c) => ({ c, cost: price(leg, c, [], table, COARSE).cost }))
    .sort((a, b) => a.cost - b.cost)
    .slice(0, SHORTLIST)
    .map(({ c }) => c);
}

/** The best of `leg`'s `candidates` on a `table`, priced finely, clear of the arrows routed
 * before it (`earlier`); the route it had (`kept`) stays unless another is clearly better
 * (`STAY`), so that an arrow does not jump to a new way round every time a card somewhere on
 * the table moves and settles. */
function best(
  leg: Leg,
  candidates: readonly Candidate[],
  earlier: readonly Route[],
  table: Box,
  kept?: Candidate,
): Route {
  const priced = (c: Candidate) => {
    const { cost, samples } = price(leg, c, earlier, table, FINE);
    return drawn(leg, c, cost, samples);
  };
  const found = candidates
    .map(priced)
    .reduce((a, b) => (b.cost < a.cost ? b : a));
  if (!kept) return found;
  const stay = priced(kept);
  return found.cost < stay.cost * STAY.share - STAY.by ? found : stay;
}

/** Every ordering of `items`. */
function orders<T>(items: readonly T[]): T[][] {
  if (items.length <= 1) return [[...items]];
  return items.flatMap((item, i) =>
    orders([...items.slice(0, i), ...items.slice(i + 1)]).map((rest) => [
      item,
      ...rest,
    ]),
  );
}

/** Routes `legs` over a `size` table, each clear of the cards it avoids and of the others,
 * in whichever order (of up to `REORDER` legs) costs least in all; returned in `legs`' order.
 * A leg routed before keeps its `previous` choice unless another is clearly better (`best`). */
export function route(
  legs: readonly Leg[],
  size: Vector,
  previous: readonly (Choice | undefined)[] = [],
): Route[] {
  const table = { x: 0, y: 0, width: size[0], height: size[1] };
  const indices = legs.map((_, i) => i);
  const candidates = legs.map((leg) => shortlist(leg, table));
  const kept = legs.map((leg, i) => {
    const choice = previous[i];
    return choice && again(leg, choice);
  });
  let cheapest: { cost: number; routes: Route[] } | undefined;
  for (const order of legs.length <= REORDER ? orders(indices) : [indices]) {
    const routes: Route[] = [];
    const placed: Route[] = [];
    for (const i of order) {
      const r = best(legs[i]!, candidates[i]!, placed, table, kept[i]);
      routes[i] = r;
      placed.push(r);
    }
    const cost = placed.reduce((sum, r) => sum + r.cost, 0);
    if (!cheapest || cost < cheapest.cost) cheapest = { cost, routes };
  }
  return cheapest?.routes ?? [];
}

/** `leg` routed as `choice` was, from the same ports with the same reach, as its ends move;
 * undefined if its ends no longer have those ports. */
export function retrace(leg: Leg, choice: Choice): Route | undefined {
  const c = again(leg, choice);
  if (!c) return undefined;
  return drawn(
    leg,
    c,
    0,
    Array.from({ length: FINE + 1 }, (_, n) => bezier(c.p, n / FINE)),
  );
}
