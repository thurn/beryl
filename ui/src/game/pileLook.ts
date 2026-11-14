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
 * permanents reach under the corner, so the piles claim the band above that row: the deepest
 * layer of the thickest pile ends `PILE_CLEARANCE` above it (`PILE_BASE`). Leave room between
 * piles for the sideways spread of a thick pile and the lean at ultrawide widths. */
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

/** How far above the opponent's back row the deepest layer of their thickest pile ends: no
 * layer of a pile ever lies over a permanent (by-lomm). */
const PILE_CLEARANCE = 4;
/** How thick a pile of `depth` cards under its top card is drawn, in pixels: each of the first
 * dozen shows, and the rest close up so a full library stays in its corner. */
function pileThickness(depth: number) {
  return 1.25 * Math.min(depth, 12) + (13 * depth) / (depth + 45);
}
/** The thickness no pile reaches, however many cards it holds. */
const MOST_THICKNESS = 1.25 * 12 + 13;
/** How far each pile's top card lies below its anchor (`PILES`). The top faces of a side's
 * piles share their bottom edge whatever each holds (by-jcju), and the opponent's deepest
 * layers, which spread downward, end `PILE_CLEARANCE` above their back row (`ROWS`). */
const PILE_BASE = {
  you: 13,
  opponent:
    ROWS.opponent.back -
    PILE_CLEARANCE -
    spread("opponent", MOST_THICKNESS).y -
    PILES.opponent.library.y -
    (PILES.opponent.width * depthScale(PILES.opponent.library.y)) / CARD_ASPECT,
};
/** Where the card `depth` beneath a pile's top lies relative to it. Arena's camera looks down
 * on the table from over its middle, so a pile shows the sides that face the middle and the
 * near edge: your library, in the near left corner, shows its cards' right edges, the deeper
 * ones level with the top card and further right (`_IBqPx0gcag 13:04.5`, `kvVLJ_cS_KE
 * 11:15.5`: a quarter of a card's width for some forty cards), and the opponent's, in the far
 * right corner, shows their near edges below the top card and a little of their left edges
 * (`kvVLJ_cS_KE 11:15.5`, `nBj2ikeYCtk 10:59.5`: a fifth of a card's width down and a tenth
 * left for some forty cards). The far pile spreads less, as it is further from the camera,
 * and so fits the band above the opponent's back row with its top card wholly on the stage. */
function pileOffset(side: Side, depth: number) {
  return spread(side, pileThickness(depth));
}
/** Where a layer `thickness` beneath a pile's top card lies relative to it (`pileOffset`). */
function spread(side: Side, thickness: number) {
  return side === "you"
    ? { x: thickness, y: 0 }
    : { x: -0.3 * thickness, y: 0.65 * thickness };
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
}

/** The top face lies at the pile's anchor, whatever the pile holds, so it shares its bottom
 * edge with its neighbors. Deeper cards spread out behind it. */
export function pileLook(side: Side, at: { x: number; y: number }): PileLook {
  const width = PILES[side].width * depthScale(at.y);
  const center = at.x + width / 2;
  return {
    x: at.x,
    y: at.y + PILE_BASE[side],
    width,
    skew: lean(center, at.y),
  };
}

/** The boxes `side`'s library, graveyard and exile take in their corner, a top card each and
 * the deepest layers any pile could spread to beneath it. */
export function pileBoxes(side: Side): Box[] {
  const deepest = spread(side, MOST_THICKNESS);
  return (["library", "graveyard", "exile"] as const).map((zone) => {
    const look = pileLook(side, PILES[side][zone]);
    const right = look.x + look.width;
    const bottom = look.y + look.width / CARD_ASPECT;
    return {
      left: Math.min(look.x, look.x + deepest.x),
      top: Math.min(look.y, look.y + deepest.y),
      right: Math.max(right, right + deepest.x),
      bottom: Math.max(bottom, bottom + deepest.y),
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
