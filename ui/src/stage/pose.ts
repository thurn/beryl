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
  opacity: number;
  /** What it is drawn over: the greater is in front. */
  depth: number;
  /** Which side of the card shows. */
  face: "front" | "back";
}

/** How far over everything at rest a moving object is drawn (CardLayer's FLYING_Z). */
const ALOFT = 900;

/** The pose `progress` of the way from `from` to `to`, drawn over what lies at rest, showing
 * the side it will show there. */
export function between(from: Pose, to: Pose, progress: number): Pose {
  const mix = (a: number, b: number) => a + (b - a) * progress;
  return {
    x: mix(from.x, to.x),
    y: mix(from.y, to.y),
    width: mix(from.width, to.width),
    rotation: mix(from.rotation, to.rotation),
    skew: mix(from.skew, to.skew),
    opacity: mix(from.opacity, to.opacity),
    depth: ALOFT + to.depth,
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
  a.opacity === b.opacity &&
  a.depth === b.depth &&
  a.face === b.face;

/** The CSS transform that draws a frame `frame` wide at `pose`, about its top-left corner:
 * moved, then scaled, turned and leaned, the order game/layout.ts places cards in. */
export const transformOf = (pose: Pose, frame: number) =>
  `translate(${pose.x}px, ${pose.y}px) scale(${pose.width / frame}) ` +
  `rotate(${pose.rotation}deg) skewX(${pose.skew}deg)`;
