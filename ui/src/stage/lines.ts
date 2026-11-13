// The shape of an arrow or line (UI Conventions, Verbs, point): where it leaves one thing and
// meets the other, given only how the two lie at one stage time. A shape is data, like a pose:
// the stage works one out for every line it holds on every tick, from the poses its model has
// for the line's two ends, so a line is never anywhere its ends are not, and the surface only
// writes it. Nothing here reads the page, routes round other cards or remembers the last
// frame: Arena's arcs go over whatever lies between (`I3Te0m1bBFw 31:35.05`), and a shape
// that is a function of its two ends alone is the same however its stage time was reached.
//
// What each says is drawn its own way: an arrow at a target is a bowed arc with a head, from
// the spell or ability over the top to what it is aimed at; the line from an ability to its
// source is thin with no head, and on the desktop hangs low over the table between the two
// (`hung`); a line of combat is straight, from the tip
// of an attacker's chevron to what it attacks, and from a blocker's edge to its attacker's.
import type { Arrow } from "../generated/protocol";
import type { Point, Resting, StageLayout } from "./layout.ts";
import type { Pose } from "./pose.ts";

/** One end of a line as it lies at one stage time: its middle, and how far it reaches from
 * there across and down, as a box or, `round`, as the ellipse inside that box (a life total).
 * `toward` is the way its controller faces across the table, up the stage (-1) or down it
 * (1), which is where an attacker wears its chevron, whose tip is `ahead` of its edge; 0 for
 * what faces neither way. */
export interface Reach {
  middle: Point;
  half: Point;
  round: boolean;
  toward: -1 | 0 | 1;
  ahead: number;
  /** How far its card is turned from how it lies, clockwise in degrees: a striking
   * creature leans (tracks.ts `STRIKE.lean`), and the marks it wears lean with it. */
  lean?: number;
}

/** A player's life total as an end of a line, its art lying from `corner` about `middle`
 * (layout.ts `Anchors.life`): the oval within it. */
export const reachOfLife = ({
  corner,
  middle,
}: {
  corner: Point;
  middle: Point;
}): Reach => ({
  middle,
  half: { x: middle.x - corner.x, y: middle.y - corner.y },
  round: true,
  toward: 0,
  ahead: 0,
});

/** A player's unspent mana as the place a streak of it leaves, its symbols lying from
 * `corner` (layout.ts `Anchors.pool`): a point a little way in from there. */
export const reachOfPool = (corner: Point): Reach => ({
  middle: { x: corner.x + POOL.x, y: corner.y + POOL.y },
  half: { x: 0, y: 0 },
  round: true,
  toward: 0,
  ahead: 0,
});
/** How far in from its corner a pool's first symbol has its middle, in stage pixels. */
const POOL = { x: 24, y: 17 };

/** A card or an ability on the stack as an end of a line, as `layout` has what rests as
 * `resting` lying at `pose`: yours faces up the stage and the opponent's down it. */
export const reachOfCard = (
  layout: Pick<StageLayout, "middle" | "half" | "chevron">,
  resting: Resting,
  pose: Pose,
): Reach => ({
  middle: layout.middle(resting, pose),
  half: layout.half(resting, pose),
  round: false,
  toward: resting.object?.controller === "opponent" ? 1 : -1,
  ahead: layout.chevron * pose.width,
  ...(pose.turn ? { lean: pose.turn } : {}),
});

/** A line as it is drawn at one stage time: its path in stage pixels, and where it leaves
 * its `from` end and meets its `to` end when drawn whole. */
export interface Shape {
  d: string;
  from: Point;
  to: Point;
}

/** How far an arc bows from the straight way between its ends, as a share of that way, and
 * the least and most it bows, in half-heights of its smaller end; and for two ends with less
 * than `close` of those between them, how far past the side of both its arc goes round. */
const BOW = { share: 0.2, least: 0.35, most: 2.6, close: 1.5, round: 0.8 };

/** An arrowhead's length as a share of half the width of what it meets, the least and most
 * it is in stage pixels, and its half-width as a share of its length. */
const HEAD = { share: 0.24, least: 8, most: 14, spread: 0.45 };

/** How much of a head's length the shaft stops short of the tip, to end inside it. */
const INSIDE = 0.6;

/** How an arrow comes onto a card: round a point in front of it, out on the table it faces,
 * as far out as a `share` of the way the arrow has come (the `least` and `most` in
 * half-heights of the smaller end) and leaning a little back the way it came, never further
 * `aside` than within the card's own width, so that it meets the edge that faces the table;
 * and how far
 * `into` the card's face, from its edge toward its middle, the head's tip lies. */
const ONTO = {
  share: 0.15,
  least: 0.7,
  most: 2,
  lean: 0.12,
  aside: 0.6,
  into: 0.4,
};

const sum = (a: Point, b: Point, by = 1): Point => ({
  x: a.x + b.x * by,
  y: a.y + b.y * by,
});
const mix = (a: Point, b: Point, t: number) => sum(a, sum(b, a, -1), t);
const length = (a: Point) => Math.hypot(a.x, a.y);
const said = ({ x, y }: Point) => `${x.toFixed(1)} ${y.toFixed(1)}`;

/** Where the way from `end`'s middle toward `point` crosses its edge; none if `point` lies
 * within it. */
function edge(end: Reach, point: Point): Point | undefined {
  const way = sum(point, end.middle, -1);
  const [across, down] = [way.x / end.half.x, way.y / end.half.y];
  const out = end.round
    ? Math.hypot(across, down)
    : Math.max(Math.abs(across), Math.abs(down));
  return out > 1 ? sum(end.middle, way, 1 / out) : undefined;
}

/** Whether the two ends lie over one another: the middle of either is within the other. */
const overlap = (a: Reach, b: Reach) =>
  edge(a, b.middle) === undefined || edge(b, a.middle) === undefined;

/** Whether two ends that lie apart have little between them, along the straight way from
 * one's middle to the other's. */
function tight(from: Reach, to: Reach): boolean {
  const [near, there] = [edge(from, to.middle), edge(to, from.middle)];
  const unit = Math.min(from.half.y, to.half.y);
  return !near || !there || length(sum(there, near, -1)) < BOW.close * unit;
}

/** The arc of an arrow onto a card that faces across the table: out of `from` and round a
 * point in front of `to`, on the open table, so that it comes onto its target square, from
 * the side no neighbour of its row stands on, and names it and no other (Arena's comes down
 * onto the middle of its target, `I3Te0m1bBFw 31:35.08`). None where `from` itself stands
 * in front of its target. */
function onto(from: Reach, to: Reach): [Point, Point, Point] | undefined {
  const back = sum(from.middle, to.middle, -1);
  const unit = Math.min(from.half.y, to.half.y);
  const ahead = Math.min(
    Math.max(ONTO.share * length(back), ONTO.least * unit),
    ONTO.most * unit,
  );
  const lean = Math.max(-1, Math.min(1, (ONTO.lean * back.x) / to.half.x));
  const front = {
    x: to.middle.x + lean * ONTO.aside * to.half.x,
    y: to.middle.y + to.toward * (to.half.y + ahead),
  };
  const [a, b] = [edge(from, front), edge(to, front)];
  return a && b ? [a, front, b] : undefined;
}

/** The arc between two ends that lie apart: out of `from` and into `to` round a point off
 * the straight way between their middles, on the side that is up the stage, so that an arc
 * goes over what lies between (Arena's "over the top"). Two ends with little between them
 * (a phone's stack and the row beside it) would leave no room for an arrow there: its arc
 * goes round the side of both instead, out of one's edge and into the other's. */
function arc(from: Reach, to: Reach): [Point, Point, Point] | undefined {
  const way = sum(to.middle, from.middle, -1);
  const far = length(way);
  const unit = Math.min(from.half.y, to.half.y);
  const side = way.x >= 0 ? 1 : -1;
  const up = { x: (side * way.y) / far, y: (-side * way.x) / far };
  // How far each end reaches the way the arc bows.
  const out = (end: Reach) =>
    Math.abs(up.x) * end.half.x + Math.abs(up.y) * end.half.y;
  const [near, there] = [edge(from, to.middle), edge(to, from.middle)];
  if (!near || !there) return undefined;
  const bow = tight(from, to)
    ? Math.max(out(from), out(to)) + BOW.round * unit
    : Math.min(Math.max(BOW.share * far, BOW.least * unit), BOW.most * unit);
  const over = sum(mix(from.middle, to.middle, 0.5), up, bow);
  // Out of each end toward that point, or straight toward the other end where the point is
  // within one of them.
  return [edge(from, over) ?? near, over, edge(to, over) ?? there];
}

/** The arc from a stack item to the one it lies over (UI Conventions, "Two items on the
 * stack"). The one on top lies a little across and down from the other, of which a strip
 * shows along one edge, its name on it, and its side beside that. The arc leaves the side of
 * `from` that faces the same way, swings out clear of both and comes back into the end of
 * that strip, so that its head lies on the table beside the stack, pointing at the name of
 * what it is aimed at and over no other item's. */
function loop(from: Reach, to: Reach): [Point, Point, Point] {
  const side = from.middle.x >= to.middle.x ? -1 : 1;
  const high = from.middle.y >= to.middle.y ? -1 : 1;
  const [shown, covered] = [to, from].map(
    (end) => end.middle.y + high * end.half.y,
  ) as [number, number];
  const strip = Math.abs(covered - shown) || 0.24 * to.half.y;
  const a = {
    x: from.middle.x + side * from.half.x,
    y: from.middle.y + high * from.half.y * 0.5,
  };
  const b = {
    x: to.middle.x + side * to.half.x,
    y: shown - (high * strip) / 2,
  };
  const out = 0.7 * Math.min(from.half.x, to.half.x);
  const clear = side < 0 ? Math.min(a.x, b.x) : Math.max(a.x, b.x);
  return [a, { x: clear + side * out, y: (a.y + b.y) / 2 }, b];
}

/** The line from an ability `from` to its `source` where it sags to the stage height `sag`,
 * below both: out of the bottom edge of each, round a point `SAG_ALONG` of the way across from
 * the source to the ability and as low as puts the middle of the curve at that height.
 * Arena's leaves the bottom of its source going down, sags to about 0.9 of the table's height
 * wherever its ends lie, and comes up into the bottom of the stack card, from either seat
 * (`0VsYGs88NXA 19:39.85`, `2VW6ZzmS8U4 32:16.0`, `5UHzpVvUkfI 19:48.89`): a curve fitted to
 * the two whole ones, within 0.03 of the table's height of either. */
function hung(from: Reach, source: Reach, sag: number): [Point, Point, Point] {
  const bottom = (end: Reach) => ({
    x: end.middle.x,
    y: end.middle.y + end.half.y,
  });
  const [a, b] = [bottom(from), bottom(source)];
  // The middle of a curve round `c` is a quarter of each end and half of `c`.
  const low = 2 * sag - (a.y + b.y) / 2;
  return [a, { x: b.x + SAG_ALONG * (a.x - b.x), y: low }, b];
}
const SAG_ALONG = 0.225;

/** The first `t` of the curve from `a` round `c` to `b`. */
const part = (
  [a, c, b]: [Point, Point, Point],
  t: number,
): [Point, Point, Point] => [
  a,
  mix(a, c, t),
  mix(mix(a, c, t), mix(c, b, t), t),
];

/** A line of combat's two ends: from the middle of the creature it leaves to the middle of
 * what it points at, over both, as Arena draws a block line from its blocker to its
 * attacker (`IEXcjBn3BXk 22:23.0`) and an attack line to the planeswalker attacked
 * (`UrGalw_f35w 48:56.6`). None where the two lie over each other. */
function straight(from: Reach, to: Reach): [Point, Point] | undefined {
  const apart = (axis: "x" | "y") =>
    Math.abs(to.middle[axis] - from.middle[axis]) >= from.half[axis] / 2;
  if (!(apart("x") || apart("y"))) return undefined;
  return [from.middle, to.middle];
}

/** The whole way of the line that says `arrow` between `from` and `to` as they lie, as a
 * curve from its first point round its second to its third (a line of combat is straight, its
 * second point half way): what its path is cut from, and what its light is drawn along
 * (light.ts). None where there is nothing to draw: an ability has yet to come clear of the
 * source it lifts out of, or a creature has closed on what it strikes. The line to a source
 * sags to `sag`, where the layout gives one (`hung`). */
export function wayOf(
  arrow: Arrow,
  from: Reach,
  to: Reach,
  sag?: number,
): [Point, Point, Point] | null {
  if (arrow === "attack" || arrow === "block") {
    const ends = straight(from, to);
    return ends ? [ends[0], mix(ends[0], ends[1], 0.5), ends[1]] : null;
  }
  const over = overlap(from, to);
  if (arrow === "source") {
    if (over) return null;
    return sag === undefined ? (arc(from, to) ?? null) : hung(from, to, sag);
  }
  // Onto a card from the table in front of it; round the side of one that stands right by;
  // over the top to a player; and into the strip of the stack item under its own. But for
  // that strip, the head lies on the face of what it is aimed at, not at its edge.
  const apart = over ? undefined : arc(from, to);
  const faced =
    apart && to.toward !== 0 && !tight(from, to) ? onto(from, to) : undefined;
  const reached = faced ?? apart;
  return reached
    ? [reached[0], reached[1], mix(reached[2], to.middle, ONTO.into)]
    : loop(from, to);
}

/** The line that says `arrow` between `from` and `to` as they lie, `shown` of it drawn (the
 * line from an ability to its source grows out of the source, every other out of `from`);
 * none where there is nothing to draw: nothing is shown yet, an ability has yet to come
 * clear of the source it lifts out of, or a creature has closed on what it strikes. */
export function shapeOf(
  arrow: Arrow,
  from: Reach,
  to: Reach,
  shown: number,
  sag?: number,
): Shape | null {
  if (shown <= 0) return null;
  const whole = wayOf(arrow, from, to, sag);
  if (!whole) return null;
  if (arrow === "attack" || arrow === "block") {
    const [a, , b] = whole;
    return { d: `M${said(a)} L${said(mix(a, b, shown))}`, from: a, to: b };
  }
  if (arrow === "source") {
    const [a, c, b] = whole;
    const [start, round, end] = part([b, c, a], shown);
    return {
      d: `M${said(start)} Q${said(round)} ${said(end)}`,
      from: a,
      to: b,
    };
  }
  const [a, c, tip] = part(whole, shown);
  // The head lies along the way the arc arrives, its tip where the arc ends, and the shaft
  // goes there and back so that only the head is filled.
  const long = Math.min(
    Math.max(HEAD.share * to.half.x, HEAD.least),
    HEAD.most,
  );
  const way = sum(tip, c, -1);
  const far = length(way) || 1;
  const back = { x: -way.x / far, y: -way.y / far };
  const wing = {
    x: -back.y * long * HEAD.spread,
    y: back.x * long * HEAD.spread,
  };
  const base = sum(tip, back, long);
  const neck = sum(tip, back, long * INSIDE);
  const d =
    `M${said(a)} Q${said(c)} ${said(neck)} Q${said(c)} ${said(a)} Z ` +
    `M${said(tip)} L${said(sum(base, wing))} L${said(sum(base, wing, -1))} Z`;
  return { d, from: whole[0], to: whole[2] };
}
