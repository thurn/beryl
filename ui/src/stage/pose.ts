// A pose: everything about where and how one object is drawn at one stage time. The stage's
// model computes poses and its surface writes them; nothing reads one back from the page, so a
// pose is the same whether a browser drew it or not.

/** One object at one stage time, in stage pixels and degrees. */
export interface Pose {
  /** The top-left corner of its frame, before it is turned or leaned. */
  x: number;
  y: number;
  /** How wide its frame is drawn, its turn aside: a card held up large has a large width. */
  width: number;
  /** Its turn about that corner. */
  rotation: number;
  /** Its lean, as everything lying on the table leans away from the middle. */
  skew: number;
  /** How far its card is turned from how it is dressed, clockwise about the card's own
   * middle: nothing at rest, where a tapped permanent is dressed lying (the skin turns its
   * frame a quarter in its slot), and what a card that is tapping or untapping has yet to
   * turn (tracks.ts `turned`). */
  turn: number;
  opacity: number;
  /** What it is drawn over: the greater is in front. */
  depth: number;
  /** Which side of the card shows. */
  face: "front" | "back";
  /** How much of its card's height shows as it turns over top over bottom, about the card's
   * own middle: nothing said at rest, where it lies flat, and 0 as it is edge-on to the eye
   * (tracks.ts `flatAt`). */
  flat?: number;
}

/** How far over everything at rest a moving object is drawn (CardLayer's FLYING_Z). */
export const ALOFT = 900;

/** The pose `progress` of the way from `from` to `to`, showing the side it will show there:
 * `aloft`, drawn over what lies at rest; or at the depth it will rest at, as what only makes
 * room for another moves. */
export function between(
  from: Pose,
  to: Pose,
  progress: number,
  aloft: boolean,
): Pose {
  const mix = (a: number, b: number) => a + (b - a) * progress;
  return {
    x: mix(from.x, to.x),
    y: mix(from.y, to.y),
    width: mix(from.width, to.width),
    rotation: mix(from.rotation, to.rotation),
    skew: mix(from.skew, to.skew),
    turn: mix(from.turn, to.turn),
    opacity: mix(from.opacity, to.opacity),
    depth: (aloft ? ALOFT : 0) + to.depth,
    face: to.face,
  };
}

/** Whether two poses draw alike. */
export const samePose = (a: Pose, b: Pose) =>
  a.x === b.x &&
  a.y === b.y &&
  a.width === b.width &&
  a.rotation === b.rotation &&
  a.skew === b.skew &&
  a.turn === b.turn &&
  a.opacity === b.opacity &&
  a.depth === b.depth &&
  a.face === b.face &&
  a.flat === b.flat;

/** The CSS transform that draws a frame `frame` wide at `pose`, about its top-left corner:
 * moved, then scaled, turned and leaned, the order game/layout.ts places cards in. A pose's
 * `turn` is not the node's but its card's, inside it (`turnOf`). */
export const transformOf = (pose: Pose, frame: number) =>
  `translate(${pose.x}px, ${pose.y}px) scale(${pose.width / frame}) ` +
  `rotate(${pose.rotation}deg) skewX(${pose.skew}deg)`;

/** The CSS `rotate` of the card inside a node at `pose`, which the skin's own quarter turn of a
 * tapped frame is added to (stage.css): none at rest. */
export const turnOf = (pose: Pose) => (pose.turn ? `${pose.turn}deg` : "");

/** The CSS `scale` of what is inside a node at `pose` as its card turns over, which narrows
 * each about its own middle, the card's (stage.css): none while it lies flat. */
export const flatOf = (pose: Pose) =>
  pose.flat === undefined || pose.flat === 1 ? "" : `1 ${pose.flat}`;
