// Where each side's rows of permanents may lie, and the game log's column they avoid.
import type { Side } from "../generated/protocol";
import { CARD_ASPECT } from "../components/Card";
import { LAND_WIDTH } from "./placement";
import { depthScale } from "./perspective";

/** Each side's rows, in stage pixels, as MTG Arena lays them out: its creatures' row at
 * `permanents`, toward the middle of the table and centered on it; behind it, one back row with
 * its top at `back`. Its mana, lands and mana artifacts, stands on the left, from `lands` to
 * `landsEnd`, untapped ones before tapped ones; everything else that is not a creature
 * (artifacts, enchantments, planeswalkers, battles, and its player's emblems), `others.width`
 * wide, smaller than a creature, stands on the right, from its life orb's phase stops at
 * `others.x` out to the stack's column's outer edge at `others.end`, about as much room as its
 * lands have. The sides never change; a row too long for its room shrinks, all its cards
 * alike, instead. Each permanent's row follows its types after continuous effects, so an
 * animated land or a crewed Vehicle stands with the creatures. The rows lie clear of one
 * another, so a long row of creatures may pass over the land side of the table. Every row's
 * cards are scaled by their depth (depthScale). */
/** Where each side's step stops about its life orb begin (screens.css .turn-controls
 * .step-stops, whose lower row's first stop stands at its left edge): the lands end clear of
 * them, a leaning card's near corner included. */
const ORB_LEFT = 646;
/** Where both back rows' other permanents may reach: the stack's column's outer edge, which
 * stands between the two back rows (STACK), clear of both. */
const OTHERS_END = 1500;
/** Each side's rows on the base stage (`STAGE_WIDTHS`); `sideRows` holds them to the stage's
 * edges and middle as it is. */
export const ROWS: Record<
  Side,
  {
    back: number;
    lands: number;
    landsEnd: number;
    permanents: number;
    others: { x: number; end: number; width: number };
  }
> = {
  // Just under the opponent's hand; their other permanents clear of the step banner
  // (screens.css .step-banner, 602 to 1002 across), whose detail text would lie over them, and
  // under their piles in the far corner, which end above it (`PILE_BASE`). As wide as keeps a
  // planeswalker's loyalty readable at that depth.
  opponent: {
    back: 112,
    lands: 158,
    landsEnd: 590,
    permanents: 235,
    others: { x: 998, end: OTHERS_END, width: 110 },
  },
  // Right of your orb and its phase stops (which end at 975), above the turn button; your
  // library and graveyard lie in front of the lands, in the near left corner.
  you: {
    back: 570,
    lands: 111,
    landsEnd: ORB_LEFT - 10,
    permanents: 408,
    others: { x: 988, end: OTHERS_END, width: 92 },
  },
};
/** Where each player's unspent mana shows, its top-left corner: over the start of their lands,
 * on the side toward the middle of the table (above yours, under the opponent's at their full
 * size), clear of the life orbs' phase stops. */
const POOL_HEIGHT = 34;
export const MANA_POOLS: Record<Side, { left: number; top: number }> = {
  opponent: {
    left: ROWS.opponent.lands,
    top:
      ROWS.opponent.back +
      (LAND_WIDTH * depthScale(ROWS.opponent.back)) / CARD_ASPECT +
      2,
  },
  you: { left: ROWS.you.lands, top: ROWS.you.back - POOL_HEIGHT - 2 },
};

/** The least zoom the battlefield may take (`battlefieldZoom`) before its rows crowd their cards
 * together instead. */
export const ZOOM_LEAST = 0.3;
/** The least zoom the back rows may pull the battlefield down to. Below it, each crowded
 * back row fits its own space so the front rows stay readable. */
export const BACK_ROW_PULL = 0.8;
/** Lands of different names stand this far apart before depth scaling, and the tapped ones
 * stand this far from the untapped ones. */
export const LAND_GAP = 12;
export const TAPPED_GAP = 44;
/** How far apart permanents stand, in frame widths. */
export const PERMANENT_GAP = 0.1;
/** Where the permanents' rows may lie: centered on the table while they fit left of the
 * stack's column, sliding left when they don't, and shrinking when they still don't. The
 * stack and a prompt stand to the right, so neither moves a permanent by coming or going. A
 * row reaching into the game log's column folds the log away beside it (overLog); opened, the
 * log takes its column back and the rows start right of it (BESIDE_LOG). */
export const ROOM = {
  // Right of the folded game log's button.
  start: 76,
  // Left of the stack's column (STACK.x), with room between for an arrow from the row's
  // last card to the stack; on the base stage, held to its right edge as the stack is.
  end: 1160,
};

/** The game log's column at the table's left (screens.css .game-log): the permanents' rows
 * pass through its height, and a long row reaches into it, unless the log is open. */
const LOG_RIGHT = 318;
/** Where the permanents' rows start while the open log stands in its column: right of it. */
export const BESIDE_LOG = LOG_RIGHT + 14;
