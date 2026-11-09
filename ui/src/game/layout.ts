// Where every card of a PlayerView lies on the stage (900 high, as wide as the window's shape;
// STAGE), in the table's pseudo-3D perspective (depthScale, lean; no WebGL). Each object is placed by its public slot, which it
// keeps from zone to zone (docs/engine-architecture.md §13), so the game screen draws one
// element per slot and a card moving zones glides from its old place to its new one rather than
// jumping. Pure functions of the view: the screen animates between their results.
import type {
  ObjectView,
  PassTo,
  PlayerView,
  Side,
  Step,
  ZoneName,
} from "../generated/protocol";
import { CARD_ASPECT } from "../components/Card";
import { cardEntry } from "../cards";
import { referenced, twins } from "./decisions";

/** The stage the table is drawn on, scaled to fit the window (Stage.tsx): always 900 high, and
 * as wide as the window's shape asks within `STAGE_WIDTHS`, so a squarer window draws the table
 * larger rather than leave bands above and below it, and a wider one takes the room at its sides.
 * Stage sets `width` (`setStageWidth`) before the table is drawn at it, and every placement reads
 * it when made: positions are pure functions of the view and the window's shape. */
export const STAGE = { width: 1600, height: 900 };

/** How wide the stage may be: `base`, the width every position here was drawn for (16:9);
 * `least`, 16:10, the app's first window and a MacBook's screen, as narrow as a choice
 * screen's tray and side column (components.css .overlay) still fit side by side; `most`, 21:9,
 * beyond which the table's controls would stand too far apart to take in at once. A window of
 * another shape shows the table's cloth past the stage's edges. */
export const STAGE_WIDTHS = { base: 1600, least: 1440, most: 2134 };

/** Sets the stage's width (Stage.tsx), within `STAGE_WIDTHS`. */
export function setStageWidth(width: number) {
  STAGE.width = Math.min(
    STAGE_WIDTHS.most,
    Math.max(STAGE_WIDTHS.least, width),
  );
}

/** Where `x`, drawn for the base stage, lies on the stage as wide as it is now: what stands at
 * the right keeps its distance from the right edge (`fromRight`), what stands at the middle from
 * the middle (`fromMiddle`); what stands at the left keeps its `x`. */
export const fromRight = (x: number) => x + STAGE.width - STAGE_WIDTHS.base;
export const fromMiddle = (x: number) =>
  x + (STAGE.width - STAGE_WIDTHS.base) / 2;

/** The camera's distance from the near edge of the table, in perspective's terms. */
const CAMERA_DISTANCE = 1400;
/** Depth grows linearly up the table, from zero at the near edge. */
const NEAR_Y = 760;
const DEPTH_PER_PX = 0.6;

/** How much a thing lying at screen height `y` is shrunk by distance: d / (d + z), what
 * `perspective(d)` does to something `z` behind the screen plane. Things lie at a depth that
 * grows toward the opponent, so they stay upright rectangles that shrink with distance. */
export const depthScale = (y: number) =>
  CAMERA_DISTANCE / (CAMERA_DISTANCE + Math.max(0, NEAR_Y - y) * DEPTH_PER_PX);

/** How far the camera is from a thing lying on the table at screen height `y`, in stage
 * pixels: what `perspective` would need to grow it as it rises toward the lens. */
export const cameraDistance = (y: number) => CAMERA_DISTANCE / depthScale(y);

/** Where the table's receding edges meet, far above the stage: a card left of center leans
 * left at its near end, and one right of center leans right. */
const VANISHING_Y = -4400;

/** How far a thing lying on the table with its top-center at (x, y) leans, in degrees: its
 * sides run toward the vanishing point, over the stage's middle. */
export const lean = (x: number, y: number) =>
  (Math.atan((x - STAGE.width / 2) / (y - VANISHING_Y)) * 180) / Math.PI;

export const CREATURE_WIDTH = 128;
export const LAND_WIDTH = 100;
/** Each copy beneath a land stack's top card, as a fraction of the card's width. */
export const LAND_STEP = { x: 0.103, y: 0.022 };
/** How far each card a permanent holds in exile peeks out from behind it, to the left and toward
 * its controller (down for yours, up for the opponent's), as a fraction of the permanent's
 * untapped width and height: what it exiled until it leaves (CR 610.3) and what its abilities
 * find "exiled with" it (CR 607.2a), mirrored across the table. A click on one opens them all.
 * Each Aura or Equipment attached to a permanent peeks out from behind it as far, up and to the
 * right on either side, so its name bar always shows. */
export const BENEATH_STEP = { x: 0.3, y: 0.17 };

/** The hand's cards: each card's top center lies on an arc over the near edge, and the fan
 * turns each a little more clockwise than the one on its left. */
export const HAND = {
  width: 182,
  spacing: 172,
  top: 726.5,
  sag: 6,
  turn: 4,
  bias: 0.6,
};

/** How the hand card the pointer is over rises out of the fan: straightened, lifted and a
 * little enlarged about its top center. CardLayer draws it inside the card's element, which
 * stays where the fan puts it, so hovering moves nothing else and the pointer keeps the card.
 * A card rises less where it would reach over your lands (`Placement.lift`). */
export const RAISE = { lift: 70, scale: 1.06 };

/** How a placed card is drawn. */
export type Look =
  | "hand" // a full frame in your hand
  | "held" // the opponent's hand: a back, or the face of a card you know
  | "field" // a battlefield frame: nonland permanents
  | "land" // a battlefield frame in a land pile
  | "beneath" // a battlefield frame tucked behind the permanent holding it in exile
  | "attached" // a battlefield frame tucked behind the permanent it is attached to
  | "stack" // a full frame on the stack
  | "pile" // a full frame on a graveyard or exile pile, which the pile's button covers
  | "shown" // a full frame face up over the table, on its way to a pile (SHOWCASE)
  | "hidden"; // nothing drawn: libraries

/** One card on the stage: its element's top-left corner at (x, y), then scaled, turned and
 * sheared about that corner, in that order (Motion's transform order). */
export interface Placement {
  slot: number;
  zone: ZoneName;
  look: Look;
  /** The frame's width before scaling. */
  width: number;
  /** The width it takes on the table before scaling: a tapped frame's turned height. */
  span: number;
  x: number;
  y: number;
  scale: number;
  rotate: number;
  skew: number;
  opacity: number;
  z: number;
  /** On the top card of a pile of identical permanents, how many it holds. */
  count?: number;
  /** On each card of a pile of identical permanents, the pile's slots, top first: what a choice
   * takes from the pile it takes from the top. */
  pile?: number[];
  /** On a hand card, how far it rises when the pointer is over it (RAISE). */
  lift?: number;
}

/** A whole board: every object's placement, by slot. */
export type Layout = Map<number, Placement>;

/** An upright rectangle on the stage, in stage pixels. */
export interface Box {
  left: number;
  top: number;
  right: number;
  bottom: number;
}

/** The box a placed card covers, its turn aside: as wide as it takes (`span`) and, a tapped
 * battlefield card, as tall as its frame is wide (lying); any other a whole card. Its lean
 * shears its near end sideways (Motion's skewX about its top-left corner). */
export function footprint(p: Placement): Box {
  const field =
    p.look === "field" ||
    p.look === "land" ||
    p.look === "beneath" ||
    p.look === "attached";
  const lies = field && p.span > p.width;
  const height = (lies ? p.width : p.width / CARD_ASPECT) * p.scale;
  const shear = Math.tan((p.skew * Math.PI) / 180) * height;
  const right = p.x + p.span * p.scale;
  return {
    left: Math.min(p.x, p.x + shear),
    top: p.y,
    right: Math.max(right, right + shear),
    bottom: p.y + height,
  };
}

/** The hand's full frames, and the width of everything drawn from a full frame. */
export const FULL_WIDTH = HAND.width;
const BACK_WIDTH = 105;

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
const ROWS: Record<
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
  // under their piles' top cards in the far corner, nearer than the piles and so over their
  // edges. As wide as keeps a planeswalker's loyalty readable at that depth.
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
const ZOOM_LEAST = 0.3;
/** The least zoom the back row's other permanents, in their room right of the orb, may
 * pull the whole battlefield down to: past it, they zoom out further alone, so a few
 * planeswalkers don't shrink every land and creature on the table. */
const OTHERS_PULL = 0.8;
/** Lands of different names stand this far apart before depth scaling, and the tapped ones
 * stand this far from the untapped ones. */
const LAND_GAP = 12;
const TAPPED_GAP = 44;
/** How far apart permanents stand, in frame widths. */
const PERMANENT_GAP = 0.1;
/** Where the permanents' rows may lie: centered on the table while they fit left of the
 * stack's column, sliding left when they don't, and shrinking when they still don't. The
 * stack and a prompt stand to the right, so neither moves a permanent by coming or going. A
 * row reaching into the game log's column folds the log away beside it (overLog); opened, the
 * log takes its column back and the rows start right of it (BESIDE_LOG). */
const ROOM = {
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
const BESIDE_LOG = LOG_RIGHT + 14;

/** Whether a permanent lies over the game log's column, so that the log must fold away rather
 * than hide behind it (Narration.tsx). Both rows of permanents span the log's height, so only
 * how far left one reaches matters; a little slack covers the lean of a card's near end. */
export function overLog(placements: Layout): boolean {
  for (const p of placements.values()) {
    if (
      (p.look === "field" || p.look === "beneath" || p.look === "attached") &&
      p.x < LOG_RIGHT + 8
    ) {
      return true;
    }
  }
  return false;
}

/** Where each side's library, graveyard and exile lie: their top cards' top-left corners, and
 * how wide a top card is before its depth shrinks it. The two sides mirror each other through
 * the table's middle: yours lie side by side in the near left corner, under your badge and left
 * of the hand's widest fan, the library outermost, then the graveyard and exile toward the
 * middle; the opponent's in the far right corner, their library outermost, then their graveyard
 * and exile toward the middle, left of the gear. Their top cards end above the opponent's back
 * row (`ROWS`), whose permanents, nearer, lie over the edges of a thick pile. */
export const PILES = {
  // In the far right corner, held to the stage's right edge.
  opponent: {
    width: 96,
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
    library: { x: 18, y: 772 },
    graveyard: { x: 126, y: 772 },
    exile: { x: 226, y: 772 },
  },
};

/** Where a pile's upright sides meet, below the stage's middle. Seen from above the near edge,
 * the cards beneath a pile's top card show on its side toward this point, and the farther the
 * pile lies from it the more of its side shows: the camera looks at the far table more nearly
 * level. */
const NADIR_Y = 1500;
/** How far each card beneath a pile's top card lies toward the nadir, as a fraction of the
 * way. */
const CARD_RISE = 0.0011;

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

/** How one of `side`'s piles of `count` cards looks, its top card's top-left corner at `at`,
 * from where it lies alone: one rule for every library, graveyard and exile. */
export function pileLook(
  side: Side,
  at: { x: number; y: number },
  count: number,
): PileLook {
  const width = PILES[side].width * depthScale(at.y);
  const center = at.x + width / 2;
  const rise = Math.max(0, count - 1) * CARD_RISE;
  return {
    x: at.x,
    y: at.y,
    width,
    skew: lean(center, at.y),
    depth: {
      x: (STAGE.width / 2 - center) * rise,
      y: (NADIR_Y - at.y) * rise,
    },
  };
}

/** The stack, spells and abilities alike, large at the right of the table as in
 * docs/ui_mockups/trigger_on_stack_with_target.png: the top item in front where the mockup
 * draws its trigger, each item beneath it peeking out above by its name bar, closer together
 * the taller the stack. It stands between the two back rows, whose other permanents reach under
 * its column (`ROWS`), so neither side's are covered by it. `x` and `width` bound it; `y` is
 * the top item's top at full size. */
export const STACK = {
  // Held to the stage's right edge.
  get x() {
    return fromRight(1252);
  },
  y: 236,
  width: 236,
  step: 40,
  reach: 170,
};

/** How high the stack may reach: below the opponent's back row at its full size. */
const STACK_TOP = 234;

/** Where the stack item at `index` (bottom first) of `count` is drawn. The top item's foot stays
 * put, clear of your back row and of the turn controls and the prompts over them; a stack too
 * tall to fit under the opponent's back row shrinks, all its items alike, about its column's
 * middle. */
export function stackPlace(index: number, count: number) {
  const step = count > 1 ? Math.min(STACK.step, STACK.reach / (count - 1)) : 0;
  const height = STACK.width / CARD_ASPECT;
  const foot = STACK.y + height;
  const scale = Math.min(1, (foot - STACK_TOP) / (height + (count - 1) * step));
  const width = STACK.width * scale;
  return {
    x:
      STACK.x +
      (STACK.width - width) / 2 -
      (count - 1 - index) * step * scale * 0.4,
    y: foot - (height + (count - 1 - index) * step) * scale,
    width,
  };
}

/** A showcase: cards up to `width` wide, `gap` apart, in a row no wider than `reach`, held
 * over the pile they are going to (or the permanent that holds them) and kept `margin` inside
 * the stage, so the eye follows them from where they came to where they land. */
export const SHOWCASE = {
  width: 220,
  reach: 900,
  gap: 18,
  margin: 24,
};

/** Cards a beat of the game shows apart from where they lie (beats.ts): those `shown` face up
 * in a row over the table on their way to a pile, and those the opponent just `drawn`, dipping
 * out of their hand at the table's edge, where it would otherwise hide them, before they
 * settle in it. */
export interface Spotlight {
  shown?: ReadonlySet<number>;
  drawn?: ReadonlySet<number>;
}

/** How far below the opponent's hand a card they just drew dips into view. */
const DRAWN_DIP = 120;

/** Where the card at `index` of `count` in a showcase over the point `over` is drawn: the row
 * centered on it, as far as the stage's edges allow. */
export function showcasePlace(
  index: number,
  count: number,
  over: { x: number; y: number },
) {
  const width = Math.min(
    SHOWCASE.width,
    (SHOWCASE.reach - SHOWCASE.gap * (count - 1)) / count,
  );
  const height = width / CARD_ASPECT;
  const row = count * width + (count - 1) * SHOWCASE.gap;
  const within = (at: number, size: number, stage: number) =>
    Math.min(
      Math.max(at - size / 2, SHOWCASE.margin),
      stage - SHOWCASE.margin - size,
    );
  return {
    x: within(over.x, row, STAGE.width) + index * (width + SHOWCASE.gap),
    y: within(over.y, height, STAGE.height),
    width,
  };
}

/** How far attackers stand out of their row toward the other side, so an attack reads at a
 * glance; the rows are too close for more. */
const ADVANCE = 16;

/** Places a thing `width` wide (before its `scale`, its depth's by default) lying on the table
 * at (x, y), or reaching `rise` above it (before scaling) while scaled and leaning as if it lay
 * there. */
function onTable(
  x: number,
  y: number,
  width: number,
  rise = 0,
  scale = depthScale(y),
) {
  return {
    x,
    y: y - rise * scale,
    scale,
    rotate: 0,
    skew: lean(x + (width * scale) / 2, y),
  };
}

/** Places an upright element `width`×`height` whose top-left corner would be (left, top),
 * turned `rotate` degrees about the point (cx, cy) of its own box: the corner moves so that
 * turning about the element's corner, as placements do, lands it where turning about that
 * point would. */
function turned(
  left: number,
  top: number,
  rotate: number,
  [cx, cy]: [number, number],
  translateY = 0,
) {
  const a = (rotate * Math.PI) / 180;
  const [cos, sin] = [Math.cos(a), Math.sin(a)];
  // c - R c + R (0, translateY)
  return {
    x: left + cx - (cos * cx - sin * cy) - sin * translateY,
    y: top + cy - (sin * cx + cos * cy) + cos * translateY,
    scale: 1,
    rotate,
    skew: 0,
  };
}

/** Where the action dock's leftmost button begins, passing on to the next stop left of the turn
 * button: the dock stands 39 from the right edge (screens.css' .dock), the turn button
 * --button-width wide and the round one --button-height, --button-gap apart (tokens.css). */
const dockLeft = () => STAGE.width - 39 - 250 - 10 - 48;

/** The held fan's middle, between its outer cards' centers: its outer card, raised (RAISE), and
 * the glow of a playable card's outline stay clear of the dock (dockLeft). */
const heldRoom = () =>
  2 * (dockLeft() - 12 - STAGE.width / 2 - (HAND.width * RAISE.scale) / 2);

/** How far the hand sinks under a screen over the table (scry.png). */
const HAND_TUCK = 67;

/** The hand raised for a choice of its cards (a discard): the whole fan lifted into view,
 * larger and flatter, each card whole above your life orb (its top at y 625) and clear of the
 * action dock at the right (from x 1310): its middle `room` wide between the outer cards'
 * centers. */
export const RAISED_HAND = {
  top: 325,
  scale: 1.1,
  spacing: 214,
  // The dock is held to the stage's right edge and the fan to its middle, so each half of the
  // fan gains or loses half of what the stage does.
  get room() {
    return 940 + STAGE.width - STAGE_WIDTHS.base;
  },
  sag: 2,
  turn: 2,
};

/** How the hand is held: at the near edge; `tucked`, sunk out of the way under a screen over
 * the table; `away`, below the stage while a screen shows the hand itself (the opening hand),
 * so it is not seen twice; `raised`, lifted whole into view for a choice of its cards. */
export type HandHold = "held" | "tucked" | "away" | "raised";

/** How far below the near edge the hand waits while held `away`. */
const HAND_AWAY = 420;

/** The hand fan, held at the near edge: the more cards, the closer they sit; tucked, the fan
 * sinks out of the way; raised, it rises whole into view. */
function handPlacements(cards: ObjectView[], hold: HandHold): Placement[] {
  const n = cards.length;
  const mid = (n - 1) / 2;
  const raised = hold === "raised";
  const fan = raised ? RAISED_HAND : { ...HAND, scale: 1, room: heldRoom() };
  const width = FULL_WIDTH * fan.scale;
  const spacing = n > 1 ? Math.min(fan.spacing, fan.room / (n - 1)) : 0;
  return cards.map((o, i) => {
    const left = STAGE.width / 2 + (i - mid) * spacing - width / 2;
    const top =
      (raised ? RAISED_HAND.top : HAND.top) +
      (i - mid) ** 2 * fan.sag +
      (hold === "tucked" ? HAND_TUCK : hold === "away" ? HAND_AWAY : 0);
    return {
      slot: o.slot,
      zone: o.zone,
      look: "hand",
      width: FULL_WIDTH,
      span: FULL_WIDTH,
      opacity: 1,
      z: 200 + i,
      ...turned(left, top, (i - mid) * fan.turn + (raised ? 0 : HAND.bias), [
        width / 2,
        0,
      ]),
      scale: fan.scale,
    };
  });
}

/** How far the opponent's hand reaches down the stage: above their life orb's top (y 48),
 * which would otherwise cover the middle backs' feet, so each back can be counted. Its feet lie
 * on one arc whatever the hand's size, rising `HELD_RISE` by `heldReach` either side of the
 * middle, so that the outer backs of the largest hand stay in sight; a hand wider than that
 * closes up, its outer backs clear of the opponent's piles. */
const HELD_FOOT = 44;
const HELD_RISE = 18;
// On the base stage; between the middle and the piles at the right edge, it gains half of what
// the stage does.
const heldReach = () => fromMiddle(370);

/** How far each hand card in `hand` may rise (RAISE) without the raised card, or the reach of
 * its element that keeps the pointer on it, coming over any of the `lands` boxes of your back
 * row: a land, or a permanent beside them, is always yours to point at. */
function handLifts(hand: Placement[], lands: Box[]): Placement[] {
  // Clear of a land's foot by this much.
  const margin = 4;
  return hand.map((p) => {
    const a = (p.rotate * Math.PI) / 180;
    // The card's top center: its element turns about its top-left corner.
    const cx = p.x + (Math.cos(a) * p.width) / 2;
    const cy = p.y + (Math.sin(a) * p.width) / 2;
    // How far the turned card's higher top corner stands above its top center, and how far
    // to either side the raised card and its element's reach can come.
    const corner = (Math.abs(Math.sin(a)) * p.width) / 2;
    const half =
      (p.width * RAISE.scale) / 2 + RAISE.lift * Math.abs(Math.sin(a));
    let lift = RAISE.lift;
    for (const land of lands) {
      if (land.right < cx - half || land.left > cx + half) continue;
      // The element reaches `lift` above its turned top edge; the raised card's top stands
      // `lift` above its top center.
      const room = cy - land.bottom - margin;
      lift = Math.min(lift, (room - corner) / Math.cos(a), room);
    }
    return { ...p, lift: Math.max(0, lift) };
  });
}

/** The opponent's hand fanned along the top edge, as yours is along the bottom turned about the
 * table's middle: backs, and the faces of the cards you know (a look at their hand, a card
 * returned to it), upside down to you as the opponent holds them. Each back's foot lies on the
 * arc (`HELD_RISE`) and the back turns to lie along it, so the fan's lower edge runs smooth at
 * any size, no back's corner standing out of it. */
function heldPlacements(
  cards: ObjectView[],
  drawn?: ReadonlySet<number>,
): Placement[] {
  const n = cards.length;
  const mid = (n - 1) / 2;
  const height = BACK_WIDTH / CARD_ASPECT;
  const spacing =
    n > 1 ? Math.min(BACK_WIDTH - 2, (2 * heldReach()) / (n - 1)) : 0;
  // The arc's foot is HELD_FOOT - bend · dx² at dx from the middle; its slope there, -2 · bend
  // · dx, is how far a back turns.
  const bend = HELD_RISE / heldReach() ** 2;
  return cards.map((o, i) => {
    const dx = (i - mid) * spacing;
    const turn = (Math.atan(-2 * bend * dx) * 180) / Math.PI;
    // The foot on the arc, and the card's middle half a card up from it along its turn: the
    // card lies upside down about that middle, as the opponent holds it, facing them.
    const foot = [
      STAGE.width / 2 + dx,
      HELD_FOOT - bend * dx ** 2 + (drawn?.has(o.slot) ? DRAWN_DIP : 0),
    ] as const;
    const a = (turn * Math.PI) / 180;
    const middle = [
      foot[0] + (Math.sin(a) * height) / 2,
      foot[1] - (Math.cos(a) * height) / 2,
    ] as const;
    return {
      slot: o.slot,
      zone: o.zone,
      look: "held",
      width: BACK_WIDTH,
      span: BACK_WIDTH,
      opacity: 1,
      z: 100 + i,
      ...turned(
        middle[0] - BACK_WIDTH / 2,
        middle[1] - height / 2,
        turn + 180,
        [BACK_WIDTH / 2, height / 2],
      ),
    };
  });
}

/** The step from each group's left edge to the next's in a row of groups `widths` wide, and
 * the row's length: each group's width plus `gap`, cut down evenly when the row would be longer
 * than `room`, but keeping a sliver of each group. */
function steps(widths: number[], gap: number, room: number) {
  const natural = widths.reduce((a, w) => a + w, 0) + gap * (widths.length - 1);
  const cut =
    natural > room && widths.length > 1
      ? (natural - room) / (widths.length - 1)
      : 0;
  return {
    steps: widths.map((w) => Math.max(w * 0.3, w + gap - cut)),
    length: natural - cut * (widths.length - 1),
  };
}

/** A row of groups laid left to right from `start`, or centered on `center` when given,
 * squeezed together so the row ends by `end` (`steps`). Returns each group's x. */
function row(
  widths: number[],
  gap: number,
  start: number,
  end: number,
  center?: number,
): number[] {
  if (widths.length === 0) return [];
  const { steps: step, length } = steps(widths, gap, end - start);
  let x =
    center === undefined
      ? start
      : Math.max(start, Math.min(center - length / 2, end - length));
  return step.map((s) => {
    const at = x;
    x += s;
    return at;
  });
}

/** A row's groups `settle`d about the pinned ones, those with a left to `face`: the order of
 * the groups, by index, and each one's left in that order. Pinned groups stand in the order of
 * the middles they `want`, and the rest in their own order between them. Where the row has no
 * room for them all where they want to stand, the rest give way to whichever side of each pinned
 * group leaves the pinned ones nearest the lefts they face; otherwise, and between equals, each
 * stands before the first pinned group whose middle its own is left of. It tries every such
 * split, so its cost grows with the pinned groups' count as a power, which blocks keep small. */
function arrange(
  wants: number[],
  face: (number | undefined)[],
  widths: number[],
  gap: number,
  start: number,
  end: number,
): { order: number[]; lefts: number[] } {
  const middle = (i: number) => wants[i]! + widths[i]! / 2;
  const pinned = face.map((f) => f !== undefined);
  const ids = wants.map((_, i) => i);
  const fixed = ids
    .filter((i) => pinned[i])
    .sort((a, b) => middle(a) - middle(b));
  const free = ids.filter((i) => !pinned[i]);
  // How many of the rest stand before each pinned group, by their middles.
  const natural = fixed.map(
    (f) => free.filter((i) => middle(i) < middle(f)).length,
  );
  let best: { order: number[]; lefts: number[]; off: number; moved: number };
  // Each way of splitting the rest about the pinned groups, keeping both in order: `before[j]`
  // of the rest stand before fixed[j].
  const visit = (before: number[]) => {
    if (before.length < fixed.length) {
      for (let n = before.at(-1) ?? 0; n <= free.length; n++) {
        visit([...before, n]);
      }
      return;
    }
    const order = [
      ...fixed.flatMap((f, k) => [
        ...free.slice(k === 0 ? 0 : before[k - 1], before[k]),
        f,
      ]),
      ...free.slice(before.at(-1) ?? 0),
    ];
    const lefts = settle(
      order.map((i) => wants[i]!),
      order.map((i) => widths[i]!),
      gap,
      start,
      end,
      order.map((i) => pinned[i]!),
    );
    const off = Math.round(
      order.reduce(
        (a, i, k) => a + Math.abs(lefts[k]! - (face[i] ?? lefts[k]!)),
        0,
      ),
    );
    const moved = before.reduce((a, n, k) => a + Math.abs(n - natural[k]!), 0);
    if (!best || off < best.off || (off === best.off && moved < best.moved)) {
      best = { order, lefts, off, moved };
    }
  };
  visit([]);
  return best!;
}

/** Moves a row's groups, in order, as near as they can be to the lefts they `want`, spaced at
 * least as `row` would space them and inside `start` to `end`: each group not `pinned` first
 * gives way leftward to its right neighbor, then each is pushed right of its left neighbor, then
 * left of the end and its right neighbor, then right of the start again. */
function settle(
  wants: number[],
  widths: number[],
  gap: number,
  start: number,
  end: number,
  pinned: boolean[] = [],
): number[] {
  const n = wants.length;
  const { steps: step } = steps(widths, gap, end - start);
  const xs = [...wants];
  for (let i = n - 2; i >= 0; i--) {
    if (!pinned[i]) xs[i] = Math.min(xs[i]!, xs[i + 1]! - step[i]!);
  }
  const fromLeft = () => {
    for (let i = 0; i < n; i++) {
      xs[i] = Math.max(xs[i]!, i === 0 ? start : xs[i - 1]! + step[i - 1]!);
    }
  };
  fromLeft();
  for (let i = n - 1; i >= 0; i--) {
    xs[i] = Math.min(
      xs[i]!,
      i === n - 1 ? end - widths[i]! : xs[i + 1]! - step[i]!,
    );
  }
  fromLeft();
  return xs;
}

/** How a battlefield card `width` wide lies on the table, before depth scaling: its frame's
 * width, and the width it takes. Tapped, it turns a quarter (Card.tsx) whole and takes its
 * turned width: lying on its side, it stands lower than its row and never reaches into the
 * next. */
function lying(width: number, tapped: boolean) {
  return { frame: width, span: tapped ? width / CARD_ASPECT : width };
}

/** A row of permanents other than lands: how wide each card's frame is before scaling, where
 * the row's top lies at its depth's full scale, the room it may take, centered on `center`
 * while it fits there or laid from `start`, and the battlefield's zoom, by which its depth's
 * scale is multiplied (`battlefieldZoom`). */
interface RowSpec {
  width: number;
  y: number;
  start: number;
  end: number;
  center?: number;
  zoom: number;
  /** Whether a zoomed-out row keeps its edge toward the lane between the creatures' rows, as
   * they do, rather than toward its player. */
  lane?: boolean;
}

/** Whether a permanent is drawn in its side's creatures' row: a creature, as its types say after
 * continuous effects, so an animated land or a crewed Vehicle joins it and leaves it again. */
const isCreature = (o: ObjectView) => o.types.includes("creature");

/** Whether a permanent is drawn in its side's land piles: a land that is not also a creature. */
const isLand = (o: ObjectView) => o.types.includes("land") && !isCreature(o);

/** Whether a permanent is a mana source on the back row's land side: a land, or a noncreature
 * artifact whose card says "{T}: Add" a mana (a mana rock, a Treasure). */
const MANA_ABILITY = /^[^\n:]*:\s*Add\b/m;
const isManaSource = (o: ObjectView) =>
  isLand(o) ||
  (o.types.includes("artifact") &&
    !isCreature(o) &&
    o.name !== null &&
    (cardEntry(o.name)?.faces ?? []).some((f) =>
      MANA_ABILITY.test(f.oracle_text ?? ""),
    ));

/** A pile's cards, bottom first, so that its lowest slot lies on top: the engine offers the
 * lowest slot of interchangeable objects (architecture §7.2) and its payments tap a class of
 * sources from the lowest slot up, so whatever a choice takes from a pile (an untap, a cost's
 * taps, a sacrifice, a target) leaves it from the top, and the cards beneath stay put. */
const topLowest = (pile: ObjectView[]) =>
  [...pile].sort((a, b) => b.slot - a.slot);

/** A pile's slots top first, for the placements of a pile of more than one card. */
const pileOf = (pile: ObjectView[]) =>
  pile.length > 1 ? pile.map((o) => o.slot).reverse() : undefined;

/** One side's permanents as the table groups them (`battlefieldPlacements`): its creatures'
 * row, its back row's other permanents, and its mana's piles, untapped first (`untapped` of
 * them) and then tapped. */
interface SideGroups {
  creatures: ObjectView[][];
  others: ObjectView[][];
  landPiles: ObjectView[][];
  untapped: number;
}

/** How many lands a side has before its mana's twins share piles, unless left apart they would
 * shrink the table's cards (`layout`) below `LANDS_APART_ZOOM` of their piled size. */
const LANDS_PILED = 7;
const LANDS_APART_ZOOM = 0.9;

/** Groups one side's permanents `objects` into piles. Twins (decisions.ts) with nothing on them
 * share a pile, the top card wearing their count, as Arena stacks them; everything else stands
 * alone. Twins are alike in all the table shows (Card.tsx's marks), so a sick token and a ready
 * one, a stolen one and its owner's, or two planeswalkers of different loyalty, never share
 * one: a pile hides nothing a player would want to read. Mana piles wait for `LANDS_PILED`
 * lands. Face-down permanents, which may be
 * different cards, and those the stack `named`, which must be seen and are offered alone, stand
 * apart. */
function sideGroups(
  objects: ObjectView[],
  beneath: Map<number, ObjectView[]>,
  attached: Map<number, ObjectView[]>,
  named: Set<number>,
  pileLands?: boolean,
): SideGroups {
  const plain = (o: ObjectView) =>
    o.facing === "up" &&
    o.counters.length === 0 &&
    o.damage === 0 &&
    !o.combat &&
    o.attachedTo === null &&
    !beneath.has(o.slot) &&
    !attached.has(o.slot) &&
    !named.has(o.slot);
  const grouped = (permanents: ObjectView[], piling = true) => {
    const groups: ObjectView[][] = [];
    for (const o of permanents) {
      const pile =
        piling && plain(o)
          ? groups.find((g) => plain(g[0]!) && twins(g[0]!, o))
          : undefined;
      if (pile) pile.push(o);
      else groups.push([o]);
    }
    return groups.map(topLowest);
  };
  // The back row's left side holds the mana: lands and mana artifacts, the untapped ones first
  // and the tapped ones in a group of their own after a wider gap, so how much mana is open
  // reads at a glance. Each name stands together, piled only once there are `LANDS_PILED`
  // lands: fewer read best one by one, a basic's frameless art hiding a pile's edges.
  const name = (o: ObjectView) => o.name ?? `#${o.slot}`;
  const sources = objects.filter(isManaSource);
  const piling = pileLands ?? sources.filter(isLand).length >= LANDS_PILED;
  const names = [...new Set(sources.map(name))];
  const order = (a: ObjectView, b: ObjectView) =>
    Number(isLand(b)) - Number(isLand(a)) ||
    names.indexOf(name(a)) - names.indexOf(name(b));
  const sourcePiles = (tapped: boolean) =>
    grouped(sources.filter((o) => o.tapped === tapped).sort(order), piling);
  const untapped = sourcePiles(false);
  return {
    creatures: grouped(objects.filter(isCreature)),
    others: grouped(objects.filter((o) => !isCreature(o) && !isManaSource(o))),
    landPiles: [...untapped, ...sourcePiles(true)],
    untapped: untapped.length,
  };
}

/** The mana piles' widths and the gaps before each, before scaling: each copy beneath a pile's
 * top card peeks out by a step of its frame's size. */
function landRun({ landPiles, untapped }: SideGroups) {
  const widths = landPiles.map((pile) => {
    const { frame, span } = lying(LAND_WIDTH, pile[0]!.tapped);
    return span + frame * LAND_STEP.x * (pile.length - 1);
  });
  const gaps = landPiles.map((_, p): number =>
    p === 0 ? 0 : p === untapped ? TAPPED_GAP : LAND_GAP,
  );
  const length =
    widths.reduce((a, w) => a + w, 0) + gaps.reduce((a, g) => a + g, 0);
  return { widths, gaps, length };
}

/** The room each of a side's rows may take (`battlefieldPlacements`). */
function sideRows(side: Side, logOpen: boolean) {
  const rows = ROWS[side];
  return {
    // From the table's edge to its player's orb, at the middle.
    lands: { y: rows.back, start: rows.lands, end: fromMiddle(rows.landsEnd) },
    creatures: {
      width: CREATURE_WIDTH,
      y: rows.permanents,
      start: logOpen ? BESIDE_LOG : ROOM.start,
      end: fromRight(ROOM.end),
      center: STAGE.width / 2,
      lane: true,
    },
    // From the orb's phase stops outward, as the lands run from the table's edge inward.
    others: {
      width: rows.others.width,
      y: rows.back,
      start: fromMiddle(rows.others.x),
      end: fromRight(rows.others.end),
    },
  };
}

/** How far each row's room fits its cards, as a fraction of its depth's scale (1 or more when
 * they fit at full size). */
function rowFit(
  groups: ObjectView[][],
  spec: { width: number; y: number; start: number; end: number },
  beneath: Map<number, ObjectView[]>,
  attached: Map<number, ObjectView[]>,
) {
  if (groups.length === 0) return 1;
  const length = rowLength(groups, spec.width, beneath, attached);
  return (spec.end - spec.start) / (length * depthScale(spec.y));
}

/** How far the whole battlefield zooms out, as a fraction of each row's depth's scale: as far as
 * its most crowded row, on either side, needs to fit its room, down to `ZOOM_LEAST`, though the
 * back row's other permanents pull it no lower than `OTHERS_PULL`. Every row of both sides
 * takes the same zoom, so the cards keep one size across the table, as if the camera drew
 * back, rather than a crowded row's shrinking alone. */
function battlefieldZoom(
  sides: [Side, SideGroups][],
  beneath: Map<number, ObjectView[]>,
  attached: Map<number, ObjectView[]>,
  logOpen: boolean,
): number {
  let zoom = 1;
  for (const [side, groups] of sides) {
    const rows = sideRows(side, logOpen);
    const lands = landRun(groups).length;
    if (lands > 0) {
      zoom = Math.min(
        zoom,
        (rows.lands.end - rows.lands.start) /
          (lands * depthScale(rows.lands.y)),
      );
    }
    zoom = Math.min(
      zoom,
      rowFit(groups.creatures, rows.creatures, beneath, attached),
      Math.max(
        OTHERS_PULL,
        rowFit(groups.others, rows.others, beneath, attached),
      ),
    );
  }
  return Math.max(ZOOM_LEAST, zoom);
}

/** One side's battlefield, as MTG Arena lays it out (ROWS): mana piles on the left behind the
 * creatures' row, other permanents smaller on the right behind it, every row zoomed out alike
 * by `zoom` (`battlefieldZoom`). A blocker stands across the table from the attacker it blocks
 * among the other side's placements `across`, as combat.png does, an attacker's blockers side
 * by side about its center, so a block reads as a short line; the rest of the row makes room.
 * An attacker keeps its place in the row, standing out of it toward the other side, so
 * declaring one moves no other. The cards each permanent exiled until it leaves (`beneath`, by
 * its slot) lie tucked under it, peeking out to its left, and the Auras and Equipment attached
 * to it (`attached`, by its slot, whoever controls them, which its groups leave out) behind it,
 * peeking out to its right; its row makes room for both. Each tapped card takes its turned
 * width (`lying`). */
function battlefieldPlacements(
  side: Side,
  groups: SideGroups,
  zoom: number,
  across: Placement[] = [],
  beneath: Map<number, ObjectView[]> = new Map(),
  attached: Map<number, ObjectView[]> = new Map(),
  logOpen = false,
): Placement[] {
  const rows = sideRows(side, logOpen);
  const out: Placement[] = [];
  const { landPiles } = groups;
  const run = landRun(groups);
  const landFull = depthScale(rows.lands.y);
  const landScale = landFull * zoom;
  const landHeight = LAND_WIDTH / CARD_ASPECT;
  // A zoomed-out row keeps its edge toward its player where a full-size one has it.
  const landTop =
    rows.lands.y + (side === "you" ? (landFull - landScale) * landHeight : 0);
  let landX = rows.lands.start;
  landPiles.forEach((pile, p) => {
    landX += run.gaps[p]! * landScale;
    const { frame, span } = lying(LAND_WIDTH, pile[0]!.tapped);
    pile.forEach((o, i) => {
      const depth = pile.length - 1 - i;
      const x = landX + i * frame * LAND_STEP.x * landScale;
      const y = landTop + depth * frame * LAND_STEP.y * landScale;
      out.push({
        slot: o.slot,
        zone: o.zone,
        look: "land",
        width: frame,
        span,
        opacity: 1,
        z: 10 + p * 8 + i,
        count: depth === 0 && pile.length > 1 ? pile.length : undefined,
        pile: pileOf(pile),
        ...onTable(x, y, span, 0, landScale),
      });
    });
    landX += run.widths[p]! * landScale;
  });

  const front = permanentRow(
    side,
    groups.creatures,
    { ...rows.creatures, zoom },
    across,
    beneath,
    attached,
  );
  // Everything else stands on the back row's right, always, zooming out further alone when it
  // still doesn't fit there.
  const othersFit = rowFit(groups.others, rows.others, beneath, attached);
  const behind = permanentRow(
    side,
    groups.others,
    { ...rows.others, zoom: Math.max(ZOOM_LEAST, Math.min(zoom, othersFit)) },
    [],
    beneath,
    attached,
  );
  out.push(...front, ...behind);
  return out;
}

/** How long a row of permanents' `groups` is before scaling, each a card `frameWidth` wide
 * (`permanentRow`). */
function rowLength(
  groups: ObjectView[][],
  frameWidth: number,
  beneath: Map<number, ObjectView[]>,
  attached: Map<number, ObjectView[]>,
) {
  return groupWidths(groups, frameWidth, beneath, attached).reduce(
    (a, w) => a + w + PERMANENT_GAP * frameWidth,
    -PERMANENT_GAP * frameWidth,
  );
}

/** Each group's width before scaling: its top card's, each copy beneath it peeking out by a
 * step of its frame's size, and the cards of `beneath` and `attached` tucked under it peeking
 * out to either side (`permanentRow`). */
function groupWidths(
  groups: ObjectView[][],
  frameWidth: number,
  beneath: Map<number, ObjectView[]>,
  attached: Map<number, ObjectView[]>,
) {
  return groups.map(
    (g) =>
      lying(frameWidth, g[0]!.tapped).span +
      frameWidth * LAND_STEP.x * (g.length - 1) +
      peeking(g, frameWidth, beneath) +
      peeking(g, frameWidth, attached),
  );
}

/** How far the cards `under` a group's top card peek out beside it, before scaling. */
function peeking(
  g: ObjectView[],
  frameWidth: number,
  under: Map<number, ObjectView[]>,
) {
  const top = g[g.length - 1]!;
  const n = under.get(top.slot)?.length ?? 0;
  const narrower = (frameWidth - lying(frameWidth, top.tapped).span) / 2;
  return n === 0 ? 0 : n * frameWidth * BENEATH_STEP.x + narrower;
}

/** One row of permanents' `groups` (each a pile of twins, bottom first, or a card alone), laid
 * out as `spec` says, its blockers across from their attackers among `across`, and what each
 * holds `beneath` it and has `attached` to it tucked under it (`battlefieldPlacements`). */
function permanentRow(
  side: Side,
  groups: ObjectView[][],
  spec: RowSpec,
  across: Placement[],
  beneath: Map<number, ObjectView[]>,
  attached: Map<number, ObjectView[]>,
): Placement[] {
  const out: Placement[] = [];
  if (groups.length === 0) return out;
  const frameWidth = spec.width;
  const standing = (tapped: boolean) => lying(frameWidth, tapped);
  const height = frameWidth / CARD_ASPECT;
  const unitStep = {
    x: frameWidth * LAND_STEP.x,
    y: frameWidth * LAND_STEP.y,
  };
  const unitWidths = groupWidths(groups, frameWidth, beneath, attached);
  // The battlefield's zoom fits its most crowded row; past its least, `row` crowds groups
  // together.
  const full = depthScale(spec.y);
  const scale = full * spec.zoom;
  // A zoomed-out creatures' row keeps its edge toward the lane between the rows, where combat
  // and the stack's arrows run, so the lane stays as narrow as at full size; a back row keeps
  // its edge toward its player.
  const nearer = (side === "you") !== (spec.lane ?? false);
  const rowTop = spec.y + (nearer ? (full - scale) * height : 0);
  const width = frameWidth * scale;
  const step = { x: unitStep.x * scale, y: unitStep.y * scale };
  const tucked = (g: ObjectView[]) => peeking(g, frameWidth, beneath) * scale;
  const widths = unitWidths.map((w) => w * scale);
  const gap = PERMANENT_GAP * width;
  const { start, end } = spec;
  const xs = row(widths, gap, start, end, spec.center);
  // Where each blocker wants to stand: its attacker's blockers side by side about its center.
  const attackers = new Map(across.map((p) => [p.slot, p]));
  const blocking = new Map<number, number[]>();
  for (const o of groups.flat()) {
    const attacker = o.combat?.kind === "blocking" ? o.combat.attacker : null;
    if (attacker === null || !attackers.has(attacker)) continue;
    blocking.set(attacker, [...(blocking.get(attacker) ?? []), o.slot]);
  }
  // Each group faces its attacker straight across.
  const wants = new Map<number, number>();
  for (const [attacker, blockers] of blocking) {
    const a = attackers.get(attacker)!;
    const center = a.x + (a.span * a.scale) / 2;
    blockers.forEach((slot, i) => {
      const offset = (i - (blockers.length - 1) / 2) * width * 1.04;
      wants.set(slot, center - width / 2 + offset);
    });
  }
  const wanted = groups.map((g, i) => wants.get(g[0]!.slot) ?? xs[i]!);
  // With no blockers, every group stands where `row` put it.
  const { order, lefts } =
    blocking.size === 0
      ? { order: groups.map((_, i) => i), lefts: wanted }
      : arrange(
          wanted,
          groups.map((g) => wants.get(g[0]!.slot)),
          widths,
          gap,
          start,
          end,
        );
  const forward = side === "you" ? -ADVANCE : ADVANCE;
  order.forEach((g, k) => {
    const group = groups[g]!;
    const left = lefts[k]! + tucked(group);
    group.forEach((o, i) => {
      const depth = group.length - 1 - i;
      const x = left + i * step.x;
      const y =
        rowTop +
        depth * step.y +
        (o.combat?.kind === "attacking" ? forward : 0);
      // Combat's creatures over their neighbors, so their marks show.
      const z = 40 + k * 8 + i + (o.combat ? 60 : 0);
      const { frame, span } = standing(o.tapped);
      out.push({
        slot: o.slot,
        zone: o.zone,
        look: "field",
        width: frame,
        span,
        opacity: 1,
        z,
        count: depth === 0 && group.length > 1 ? group.length : undefined,
        pile: pileOf(group),
        ...onTable(x, y, span, 0, scale),
      });
      // Untapped, each a step further to the left and toward its controller, or for what is
      // attached to it up and to the right, centered on the permanent.
      const center = x + (span * scale) / 2;
      // An exiled card steps toward its controller: down for yours, up for the opponent's.
      const controllerward = side === "you" ? -1 : 1;
      const tuck = (
        under: ObjectView[] | undefined,
        look: "beneath" | "attached",
        toward: -1 | 1,
      ) =>
        under?.forEach((e, j) => {
          const n = j + 1;
          const up = (look === "beneath" ? controllerward : 1) * n;
          const at = (rise: number): Placement => ({
            slot: e.slot,
            zone: e.zone,
            look,
            width: frameWidth,
            span: frameWidth,
            opacity: 1,
            z: z - n,
            ...onTable(
              center +
                (toward * n * frameWidth * BENEATH_STEP.x - frameWidth / 2) *
                  scale,
              y,
              frameWidth,
              rise * height * BENEATH_STEP.y,
              scale,
            ),
          });
          // One that would rise into its side's piles in the corner peeks out sideways only.
          const peek = at(up);
          const box = footprint(peek);
          out.push(pileBoxes(side).some((b) => meets(box, b)) ? at(0) : peek);
        });
      tuck(beneath.get(o.slot), "beneath", -1);
      tuck(attached.get(o.slot), "attached", 1);
    });
  });
  return out;
}

/** The slot each of a side's emblems is drawn under: one no object has, counting down from -1,
 * the two sides' taking turns, so that each emblem keeps its own as more are made. */
export const emblemSlot = (side: Side, index: number) =>
  -1 - (2 * index + (side === "you" ? 1 : 0));

/** Whether a slot is an emblem's (`emblemSlot`), which nothing can be done with. */
export const isEmblem = (slot: number) => slot < 0;

/** Each player's emblems ([CR 114.1](https://yawgatog.com/resources/magic-rules/#R1141): "An
 * emblem is a marker used to represent an object that has one or more abilities, but no other
 * characteristics") as objects to draw: each a card, under its `emblemSlot`, in its player's
 * back row after their other permanents, where pointing at it shows it whole. The table draws
 * the command zone there ([CR 114.2](https://yawgatog.com/resources/magic-rules/#R1142): "An
 * emblem is created in the command zone"). */
export function emblemObjects(view: PlayerView): ObjectView[] {
  return (["opponent", "you"] as const).flatMap((side) =>
    view.players[side].emblems.map((name, i) => ({
      slot: emblemSlot(side, i),
      zone: "battlefield" as const,
      owner: side,
      controller: side,
      name,
      facing: "up" as const,
      token: false,
      tapped: false,
      summoningSick: false,
      damage: 0,
      counters: [],
      types: [],
      keywords: [],
      pt: null,
      attachedTo: null,
      exiledBy: null,
      exiledWith: null,
      combat: null,
    })),
  );
}

/** The Auras and Equipment drawn behind the permanents they are attached to, whoever controls
 * them, by the permanent's slot: those attached to a permanent outside the land piles that is
 * not itself attached to another. Anything else attached (to a land in a pile, or to an
 * attachment) stands in its controller's row. */
export function attachments(view: PlayerView): Map<number, ObjectView[]> {
  const bySlot = new Map(view.objects.map((o) => [o.slot, o]));
  const out = new Map<number, ObjectView[]>();
  for (const slot of view.battlefield) {
    const o = bySlot.get(slot);
    const host = o?.attachedTo == null ? undefined : bySlot.get(o.attachedTo);
    if (
      !o ||
      !host ||
      host.zone !== "battlefield" ||
      host.attachedTo !== null ||
      isLand(host)
    ) {
      continue;
    }
    out.set(host.slot, [...(out.get(host.slot) ?? []), o]);
  }
  return out;
}

/** The permanents linked to `slot` by attachment: what it is attached to, and what is attached
 * to it. */
export function linked(view: PlayerView, slot: number | undefined): number[] {
  if (slot === undefined) return [];
  const self = view.objects.find((o) => o.slot === slot);
  if (self?.zone !== "battlefield") return [];
  return view.objects
    .filter(
      (o) =>
        o.zone === "battlefield" &&
        (o.slot === self.attachedTo || o.attachedTo === slot),
    )
    .map((o) => o.slot);
}

/** The cards you can play from outside your hand at your priority, in the order the engine
 * offers them: a graveyard's flashback or escape, a card exiled with "you may play it", the
 * top of a library you may play from. Your hand fan holds them, outlined apart from the cards
 * of your hand, until they can no longer be played, when they go back where they lie. */
export function playableElsewhere(view: PlayerView): number[] {
  const status = view.status;
  if (status.kind !== "deciding" || status.decision.kind !== "priority") {
    return [];
  }
  const bySlot = new Map(view.objects.map((o) => [o.slot, o]));
  const out = new Set<number>();
  for (const { option } of status.decision.candidates) {
    if (option.kind !== "cast" && option.kind !== "playLand") continue;
    const o = bySlot.get(option.slot);
    if (
      o &&
      o.name !== null &&
      o.zone !== "hand" &&
      o.zone !== "battlefield" &&
      o.zone !== "stack"
    ) {
      out.add(o.slot);
    }
  }
  return [...out];
}

/** A pile of one player's cards lying face up, which opens to show them all: their graveyard,
 * or what they own in exile that no permanent holds beneath it (those lie under it). */
export interface ZonePile {
  side: Side;
  zone: "graveyard" | "exile" | "library";
  /** Bottom first: the last lies on top. */
  cards: ObjectView[];
  look: PileLook;
}

/** Only identities already disclosed by the session may be browsed in a library. */
export function knownLibraries(view: PlayerView): ZonePile[] {
  const objects = new Map(view.objects.map((o) => [o.slot, o]));
  return (["you", "opponent"] as const).map((side) => ({
    side,
    zone: "library",
    look: pileLook(
      side,
      PILES[side].library,
      view.players[side].library.length,
    ),
    cards: view.players[side].library.flatMap((slot) => {
      const object = objects.get(slot);
      return object?.name ? [object] : [];
    }),
  }));
}

/** The permanent among `onField` holding the card `o` in exile, drawn behind it: the one that
 * exiled it until it leaves (CR 610.3), or else the one whose abilities find it "exiled with"
 * it (CR 607.2a). */
export function held(
  o: ObjectView,
  onField: ReadonlySet<number>,
): number | undefined {
  if (o.zone !== "exile") return undefined;
  for (const host of [o.exiledBy, o.exiledWith]) {
    if (host !== null && onField.has(host)) return host;
  }
  return undefined;
}

/** The cards in exile linked to the permanent in `slot`: those it exiled until it leaves
 * (CR 610.3) and those its abilities find "exiled with" it (CR 607.2a). */
export function exiledWith(view: PlayerView, slot: number): ObjectView[] {
  return view.objects.filter(
    (o) => o.zone === "exile" && (o.exiledBy === slot || o.exiledWith === slot),
  );
}

/** Each side's graveyard and exile piles, but for the cards your hand fan holds because you can
 * play them (`playableElsewhere`). */
export function zonePiles(view: PlayerView): ZonePile[] {
  const bySlot = new Map(view.objects.map((o) => [o.slot, o]));
  const holding = new Set(view.battlefield);
  const inHand = new Set(playableElsewhere(view));
  return (["opponent", "you"] as const).flatMap((side) => {
    const graveyard = view.players[side].graveyard.flatMap((slot) =>
      inHand.has(slot) ? [] : (bySlot.get(slot) ?? []),
    );
    const exile = view.objects.filter(
      (o) =>
        o.owner === side &&
        o.zone === "exile" &&
        !inHand.has(o.slot) &&
        held(o, holding) === undefined,
    );
    return [
      { zone: "graveyard" as const, cards: graveyard },
      { zone: "exile" as const, cards: exile },
    ].map(({ zone, cards }) => ({
      side,
      zone,
      cards,
      look: pileLook(side, PILES[side][zone], cards.length),
    }));
  });
}

/** How far a targeted card under a pile's top rises out of it, in its own heights. */
const PILE_RISE = 0.6;

/** The boxes `side`'s library, graveyard and exile take in their corner, a top card each. */
function pileBoxes(side: Side): Box[] {
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
const meets = (a: Box, b: Box) =>
  a.left < b.right && b.left < a.right && a.top < b.bottom && b.top < a.bottom;

/** A full frame drawn as a pile's top card. */
function onPile(look: PileLook) {
  return {
    x: look.x,
    y: look.y,
    scale: look.width / FULL_WIDTH,
    rotate: 0,
    skew: look.skew,
  };
}

/** Your hand in its order of arrival (standIns.ts), falling back to the view's slot order. */
function inOrder(
  hand: ObjectView[],
  order: ReadonlyMap<number, number> | undefined,
): ObjectView[] {
  if (!order?.size) return hand;
  const key = (o: ObjectView) => order.get(o.slot) ?? Infinity;
  return [...hand].sort((a, b) => key(a) - key(b));
}

/** Every object's placement. The hand is tucked under a screen over the table or raised for a
 * choice of its cards (`hold`); the permanents' rows keep clear of the game log while it is
 * `logOpen` over them; the cards in a `spotlight` are shown apart from where they lie.
 * Hovering changes none of it (CardLayer raises a hovered hand card). */
export function layout(
  view: PlayerView,
  hold: HandHold = "held",
  logOpen = false,
  spotlight: Spotlight = {},
  order?: ReadonlyMap<number, number>,
): Layout {
  const out: Placement[] = [];
  const objects = view.objects;
  const owned = (side: Side, zone: ZoneName) =>
    objects.filter((o) => o.owner === side && o.zone === zone);
  const bySlot = new Map(objects.map((o) => [o.slot, o]));
  // Emblems stand after their player's other permanents.
  const permanents = [
    ...view.battlefield.flatMap((s) => bySlot.get(s) ?? []),
    ...emblemObjects(view),
  ];
  // What you can play from outside your hand waits at the right of your hand fan.
  const elsewhere = playableElsewhere(view).flatMap((s) => bySlot.get(s) ?? []);
  const inHand = new Set(elsewhere.map((o) => o.slot));
  // The cards each permanent holds in exile, by the permanent's slot.
  const beneath = new Map<number, ObjectView[]>();
  const onField = new Set(view.battlefield);
  for (const o of objects) {
    const host = held(o, onField);
    if (host === undefined || inHand.has(o.slot)) continue;
    beneath.set(host, [...(beneath.get(host) ?? []), o]);
  }
  const attached = attachments(view);
  const named = referenced(view);
  const behindHost = new Set([...attached.values()].flat().map((o) => o.slot));

  out.push(...heldPlacements(owned("opponent", "hand"), spotlight.drawn));
  // Blockers stand across from their attackers, which stand where they would anyway: only the
  // defending side's row depends on the other's.
  const grouping = (pileLands?: boolean) =>
    new Map(
      (["opponent", "you"] as const).map((side) => [
        side,
        sideGroups(
          permanents.filter(
            (o) => o.controller === side && !behindHost.has(o.slot),
          ),
          beneath,
          attached,
          named,
          pileLands,
        ),
      ]),
    );
  // Lands left apart that would shrink the table's cards much pile after all.
  let groups = grouping();
  let zoom = battlefieldZoom([...groups], beneath, attached, logOpen);
  const piled = grouping(true);
  const piledZoom = battlefieldZoom([...piled], beneath, attached, logOpen);
  if (zoom < piledZoom * LANDS_APART_ZOOM) [groups, zoom] = [piled, piledZoom];
  const field = (side: Side, across?: Placement[]) =>
    battlefieldPlacements(
      side,
      groups.get(side)!,
      zoom,
      across,
      beneath,
      attached,
      logOpen,
    );
  const yours = field("you", field("opponent"));
  out.push(...yours, ...field("opponent", yours));
  // A raised hand card keeps clear of your back row: your lands and the permanents beside them.
  const backRow = (p: Placement) => {
    // An emblem has no object here: it stands in the back row too.
    const o = bySlot.get(p.slot);
    return p.look === "land" || (p.look === "field" && !(o && isCreature(o)));
  };
  out.push(
    ...handLifts(
      handPlacements(
        [...inOrder(owned("you", "hand"), order), ...elsewhere],
        hold,
      ),
      yours.filter(backRow).map(footprint),
    ),
  );
  for (const side of ["opponent", "you"] as const) {
    const player = view.players[side];
    const library = pileLook(side, PILES[side].library, player.library.length);
    // Only a top card whose identity is known to this viewer shows over the back.
    const top = player.library.at(-1);
    for (const o of owned(side, "library")) {
      if (inHand.has(o.slot)) continue;
      const known = o.slot === top && o.name !== null;
      out.push({
        slot: o.slot,
        zone: o.zone,
        look: known ? "pile" : "hidden",
        width: FULL_WIDTH,
        span: FULL_WIDTH,
        opacity: known ? 1 : 0,
        z: known ? 2 : 1,
        ...onPile(library),
      });
    }
  }
  // A card under the top of a pile that the top of the stack targets rises out of the pile,
  // toward the table's middle, so that the target arrow meets that card and not the pile's top.
  const aimed = new Set(
    view.stack
      .at(-1)
      ?.targets.flatMap((t) => (t.kind === "player" ? [] : [t.slot])) ?? [],
  );
  for (const pile of zonePiles(view)) {
    pile.cards.forEach((o, i) => {
      const at = onPile(pile.look);
      const buried = aimed.has(o.slot) && i < pile.cards.length - 1;
      const rise = (FULL_WIDTH * at.scale * PILE_RISE) / CARD_ASPECT;
      out.push({
        slot: o.slot,
        zone: o.zone,
        look: "pile",
        width: FULL_WIDTH,
        span: FULL_WIDTH,
        opacity: 1,
        z: buried ? 20 + pile.cards.length + i : 20 + i,
        ...at,
        y: buried ? at.y + (pile.side === "you" ? -rise : rise) : at.y,
      });
    });
  }
  view.stack.forEach((item, i) => {
    if (item.kind !== "spell") return;
    const o = bySlot.get(item.slot);
    if (!o) return;
    const place = stackPlace(i, view.stack.length);
    out.push({
      slot: item.slot,
      zone: o.zone,
      look: "stack",
      width: FULL_WIDTH,
      span: FULL_WIDTH,
      opacity: 1,
      z: STACK_Z + i,
      x: place.x,
      y: place.y,
      scale: place.width / FULL_WIDTH,
      rotate: 0,
      skew: 0,
    });
  });
  const shown = objects.filter((o) => spotlight.shown?.has(o.slot));
  // The showcase stands over where its cards land: the middle of the first one's resting place.
  const landing = out.find((p) => p.slot === shown[0]?.slot);
  const over = landing
    ? {
        x: landing.x + (landing.width * landing.scale) / 2,
        y: landing.y + (landing.width * landing.scale) / CARD_ASPECT / 2,
      }
    : { x: STAGE.width / 2, y: STAGE.height / 2 };
  shown.forEach((o, i) => {
    const place = showcasePlace(i, shown.length, over);
    out.push({
      slot: o.slot,
      zone: o.zone,
      look: "shown",
      width: FULL_WIDTH,
      span: FULL_WIDTH,
      opacity: 1,
      z: SHOWCASE_Z + i,
      x: place.x,
      y: place.y,
      scale: place.width / FULL_WIDTH,
      rotate: 0,
      skew: 0,
    });
  });
  return new Map(out.map((p) => [p.slot, p]));
}

/** Cards in a showcase are drawn over the permanents and the stack. */
const SHOWCASE_Z = 800;

/** The stack's items are drawn over everything else on the table, the top item in front. */
export const STACK_Z = 400;

export const STEP_NAMES: Record<Step, string> = {
  untap: "Untap",
  upkeep: "Upkeep",
  draw: "Draw",
  precombatMain: "Main 1",
  beginningOfCombat: "Combat",
  declareAttackers: "Attackers",
  declareBlockers: "Blockers",
  firstStrikeDamage: "First-strike damage",
  combatDamage: "Combat damage",
  endOfCombat: "End of combat",
  postcombatMain: "Main 2",
  end: "End step",
  cleanup: "Cleanup",
};

/** The turn button at a halt of the session's (`DecisionView.halt`), where passing is all the
 * player can do: ending their own turn from its first main phase. */
export const HALT_BUTTON = { label: "End Turn", caption: "Nothing to play" };

/** The turn button for passing priority (decision D13): where the pass goes, as the engine
 * says (`DecisionView.passTo`). */
export function passButton(
  view: PlayerView,
  to: PassTo,
): { label: string; caption: string } {
  switch (to.kind) {
    case "resolve":
      return { label: "Resolve", caption: "" };
    case "combatDamage":
      // combat.png's Resolve Damage.
      return { label: "Resolve Damage", caption: "Combat damage" };
    case "step":
      return {
        label: "Next",
        caption: `To ${to.step === "end" ? "End Step" : STEP_NAMES[to.step]}`,
      };
    case "nextTurn":
      return view.turn.active === "you"
        ? { label: "End Turn", caption: "To Opponent's Turn" }
        : { label: "Next", caption: "To Your Turn" };
  }
}

/** The step banner during combat: what is being declared, what has been, or the damage. */
export function combatBanner(
  view: PlayerView,
): { title: string; detail: string } | undefined {
  const step = view.turn.step;
  const status = view.status;
  const kind = status.kind === "deciding" ? status.decision.kind : undefined;
  const fighting = view.objects.some((o) => o.combat?.kind === "attacking");
  const detail =
    kind === "attacker" || kind === "attackTarget"
      ? "Declare attackers"
      : kind === "blocker" || kind === "blockTarget"
        ? "Declare blockers"
        : !fighting
          ? undefined
          : step === "declareAttackers"
            ? "Attackers declared"
            : step === "declareBlockers"
              ? "Blockers declared"
              : step === "firstStrikeDamage" || step === "combatDamage"
                ? "Combat damage"
                : undefined;
  return detail && { title: "Combat", detail };
}
