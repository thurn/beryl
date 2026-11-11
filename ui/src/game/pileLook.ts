// Where the library, graveyard and exile piles lie in their corners and how a pile looks.
import type { Side } from "../generated/protocol";
import { CARD_ASPECT } from "../components/Card";
import type { Box } from "./placement";
import { FULL_WIDTH } from "./placement";
import { ROWS } from "./tableRows";
import { depthScale, fromRight, lean } from "./perspective";

/** The library, graveyard and exile anchors and their unscaled card widths. The two sides
 * mirror each other through the table's middle: yours lie in the near left corner, under your
 * badge and left of the hand's widest fan. The library is outermost, then the graveyard and
 * exile toward the middle. The opponent's lie in the far right corner, their library outermost,
 * then their graveyard and exile toward the middle, left of the gear. Their back row's other
 * permanents reach under the corner, so the piles claim the band above that row: each top card
 * ends `PILE_CLEARANCE` above it (`PILE_BASE`), and a full library's layers rise to the stage's
 * top edge. Leave room between piles for both the sideways rise of a thick pile and the lean at
 * ultrawide widths. */
export const PILES = {
  // In the far right corner, held to the stage's right edge.
  opponent: {
    width: 82,
    get library() {
      return { x: fromRight(1432), y: 8 };
    },
    get graveyard() {
      return { x: fromRight(1336), y: 8 };
    },
    get exile() {
      return { x: fromRight(1240), y: 8 };
    },
  },
  you: {
    width: 80,
    library: { x: 36, y: 772 },
    graveyard: { x: 151, y: 772 },
    exile: { x: 266, y: 772 },
  },
};

/** How far above the opponent's back row their piles' top cards end: no layer of a pile ever
 * lies over a permanent (by-lomm). */
const PILE_CLEARANCE = 4;
/** How far each pile's top card lies below its anchor (`PILES`). Each top layer is readable.
 * Lower layers close up to keep a full library in its corner. The opponent's top cards end
 * `PILE_CLEARANCE` above their back row (`ROWS`). */
const PILE_BASE = {
  you: 13,
  opponent:
    ROWS.opponent.back -
    PILE_CLEARANCE -
    PILES.opponent.library.y -
    (PILES.opponent.width * depthScale(PILES.opponent.library.y)) / CARD_ASPECT,
};
function pileThickness(depth: number) {
  return 1.25 * Math.min(depth, 12) + (13 * depth) / (depth + 45);
}
function pileOffset(side: Side, depth: number) {
  const thickness = pileThickness(depth);
  return { x: (side === "you" ? 0.5 : -0.5) * thickness, y: -thickness };
}

/** How a pile of cards looks on the table. */
export interface PileLook {
  /** The top card's top-left corner. */
  x: number;
  y: number;
  /** The top card's width, shrunk by depth. */
  width: number;
  /** How far the pile leans, in degrees, as everything lying on the table does. */
  skew: number;
  /** Where the bottom card lies relative to the top card, in pixels. */
  depth: { x: number; y: number };
}

/** The top face shares its bottom edge with its neighbors. Deeper cards fan behind it. */
export function pileLook(
  side: Side,
  at: { x: number; y: number },
  count: number,
): PileLook {
  const width = PILES[side].width * depthScale(at.y);
  const center = at.x + width / 2;
  const depth = pileOffset(side, Math.max(0, count - 1));
  return {
    x: at.x - depth.x,
    y: at.y + PILE_BASE[side],
    width,
    skew: lean(center, at.y),
    depth,
  };
}

/** The boxes `side`'s library, graveyard and exile take in their corner, a top card each. */
export function pileBoxes(side: Side): Box[] {
  return (["library", "graveyard", "exile"] as const).map((zone) => {
    const look = pileLook(side, PILES[side][zone], 1);
    return {
      left: look.x,
      top: look.y,
      right: look.x + look.width,
      bottom: look.y + look.width / CARD_ASPECT,
    };
  });
}

/** Whether two boxes overlap. */
export const meets = (a: Box, b: Box) =>
  a.left < b.right && b.left < a.right && a.top < b.bottom && b.top < a.bottom;

/** The position of an individual card, counting from the top of its pile. */
export function onPile(look: PileLook, side: Side, depth: number) {
  const offset = pileOffset(side, depth);
  return {
    x: look.x + offset.x,
    y: look.y + offset.y,
    scale: look.width / FULL_WIDTH,
    rotate: 0,
    skew: look.skew,
  };
}
