// Where every card of a PlayerView lies on the 1600×900 stage, in the table's pseudo-3D
// perspective (depthScale, lean; no WebGL). Each object is placed by its public slot, which it
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
import { referenced, twins } from "./decisions";
import type { Phase } from "../components/Controls";

/** The fixed stage the table is drawn on, scaled to fit the window (Stage.tsx). */
export const STAGE = { width: 1600, height: 900 };

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
const VANISHING = { x: STAGE.width / 2, y: -4400 };

/** How far a thing lying on the table with its top-center at (x, y) leans, in degrees: its
 * sides run toward the vanishing point. */
export const lean = (x: number, y: number) =>
  (Math.atan((x - VANISHING.x) / (y - VANISHING.y)) * 180) / Math.PI;

export const CREATURE_WIDTH = 128;
export const LAND_WIDTH = 100;
/** Each copy beneath a land stack's top card, as a fraction of the card's width. */
export const LAND_STEP = { x: 0.103, y: 0.022 };
/** How far each card exiled by a permanent until it leaves (CR 610.3) peeks out from beneath
 * it, up and to the left, as a fraction of the permanent's untapped width and height: its whole
 * name bar shows, where a copy beneath a pile's top card peeks out down and to the left. Each
 * Aura or Equipment attached to a permanent peeks out from behind it as far, up and to the
 * right. */
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
  | "beneath" // a battlefield frame tucked beneath the permanent that exiled it
  | "attached" // a battlefield frame tucked behind the permanent it is attached to
  | "stack" // a full frame on the stack
  | "pile" // a full frame on a graveyard or exile pile, which the pile's button covers
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
 * its top at `back`, in front of its library and graveyard: its land piles from `lands`, filling
 * rightward to `LANDS_END`, and its other permanents (artifacts, enchantments, planeswalkers that
 * are not creatures), `others.width` wide, smaller than a creature, right of its life orb from
 * `others.x` to `others.end`; those that overflow it stand left of the orb, past the lands, up to
 * `others.overflowEnd`. Each permanent's row follows its types after continuous effects, so
 * an animated land or a crewed Vehicle stands with the creatures. The rows lie clear of one
 * another, so a long row of creatures may pass over the land piles' side of the table. Every
 * row's cards are scaled by their depth (depthScale); a row too long for its room shrinks, all
 * its cards alike, so that none covers another's marks. */
/** Where each side's life orb begins (Board.tsx's Life, with its wings): your back row's other
 * permanents that overflow the room right of it stand left of it, right of the lands. */
const ORB_LEFT = 664;
const ROWS: Record<
  Side,
  {
    back: number;
    lands: number;
    permanents: number;
    others: { x: number; end: number; overflowEnd: number; width: number };
  }
> = {
  // Just under the opponent's hand; their other permanents clear of the step banner
  // (screens.css .step-banner, 602 to 1002 across), whose detail text would lie over them, and well
  // left of their library, which lies behind the row in the far corner. As wide as keeps a
  // planeswalker's loyalty readable at that depth.
  opponent: {
    back: 112,
    lands: 158,
    permanents: 235,
    others: { x: 998, end: 1180, overflowEnd: 598, width: 110 },
  },
  // Right of your orb, left of the stack's column and the prompt; your library and graveyard
  // lie behind the row, in the near left corner.
  you: {
    back: 570,
    lands: 111,
    permanents: 408,
    others: { x: 940, end: 1196, overflowEnd: ORB_LEFT, width: 92 },
  },
};
/** How narrow, on the stage, the back row's other permanents may shrink to fit their room before
 * they crowd one another instead: as narrow as keeps their loyalty and counters readable. */
const OTHERS_LEAST = 84;
/** How far the land piles may crowd together, as a fraction of their length, to make room for
 * the back row's other permanents left of the orb, before those shrink. */
const LANDS_SQUEEZE = 0.6;
/** Lands fill from the left up to here, `LAND_GAP` apart before depth scaling. */
const LANDS_END = 600;
const LAND_GAP = 12;
/** How far a length may overrun the room it is measured against and still fit, in stage pixels. */
const SLACK = 0.01;
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
  // last card to the stack.
  end: 1160,
  center: STAGE.width / 2,
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
 * how wide a top card is before its depth shrinks it. The opponent's lie at the far right; yours
 * lie side by side in the near left corner, under your badge and left of the hand's widest fan,
 * so nothing covers them. */
export const PILES = {
  opponent: {
    width: 104,
    library: { x: 1236, y: 13 },
    graveyard: { x: 1336, y: 40 },
    exile: { x: 1432, y: 40 },
  },
  you: {
    width: 80,
    library: { x: 18, y: 772 },
    graveyard: { x: 126, y: 772 },
    exile: { x: 226, y: 772 },
  },
};

/** Where a pile's upright sides meet, below the stage. Seen from above the near edge, the cards
 * beneath a pile's top card show on its side toward this point, and the farther the pile lies
 * from it the more of its side shows: the camera looks at the far table more nearly level. */
const NADIR = { x: STAGE.width / 2, y: 1500 };
/** How far each card beneath a pile's top card lies toward `NADIR`, as a fraction of the way. */
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
    depth: { x: (NADIR.x - center) * rise, y: (NADIR.y - at.y) * rise },
  };
}

/** The stack, spells and abilities alike, large at the right of the table as in
 * docs/ui_mockups/trigger_on_stack_with_target.png: the top item in front where the mockup
 * draws its trigger, each item beneath it peeking out above by its name bar, closer together
 * the taller the stack. `x` and `width` bound it; `y` is the top item's top at full size. */
export const STACK = { x: 1205, y: 222, width: 295, step: 46, reach: 200 };

/** How far the stack rises to clear the prompt below it, while one is open. */
const STACK_LIFT = 90;

/** How high the stack may reach: below the opponent's piles at the far right of the table,
 * whose top cards end by y 160, with room for a pile of about 20 cards' thickness beneath. */
const STACK_TOP = 175;

/** Where the stack item at `index` (bottom first) of `count` is drawn, `lifted` or not. The top
 * item's foot stays put; a stack too tall to fit under the opponent's piles shrinks, all its
 * items alike, about its column's middle. */
export function stackPlace(index: number, count: number, lifted = false) {
  const step = count > 1 ? Math.min(STACK.step, STACK.reach / (count - 1)) : 0;
  const height = STACK.width / CARD_ASPECT;
  const foot = STACK.y - (lifted ? STACK_LIFT : 0) + height;
  const scale = Math.min(1, (foot - STACK_TOP) / (height + (count - 1) * step));
  const width = STACK.width * scale;
  return {
    x: STACK.x + (STACK.width - width) / 2,
    y: foot - (height + (count - 1 - index) * step) * scale,
    width,
  };
}

/** How far attackers stand out of their row toward the other side, so an attack reads at a
 * glance; the rows are too close for more. */
const ADVANCE = 16;

/** How far a blocker's center leans from its attacker's toward the table's middle. */
const ACROSS_LEAN = 0.16;

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

/** How far the hand sinks under a screen over the table (scry.png). */
const HAND_TUCK = 67;

/** The hand fan, held at the near edge: the more cards, the closer they sit; `tucked`, the fan
 * sinks out of the way. */
function handPlacements(cards: ObjectView[], tucked: boolean): Placement[] {
  const n = cards.length;
  const mid = (n - 1) / 2;
  const spacing = n > 1 ? Math.min(HAND.spacing, 760 / (n - 1)) : 0;
  return cards.map((o, i) => {
    const left = STAGE.width / 2 + (i - mid) * spacing - FULL_WIDTH / 2;
    const top = HAND.top + (i - mid) ** 2 * HAND.sag + (tucked ? HAND_TUCK : 0);
    return {
      slot: o.slot,
      zone: o.zone,
      look: "hand",
      width: FULL_WIDTH,
      span: FULL_WIDTH,
      opacity: 1,
      z: 200 + i,
      ...turned(left, top, (i - mid) * HAND.turn + HAND.bias, [
        FULL_WIDTH / 2,
        0,
      ]),
    };
  });
}

/** How far the opponent's hand reaches down the stage: above their life orb's top (y 48),
 * which would otherwise cover the middle backs' feet, so each back can be counted. Its arc
 * rises toward the ends no more than `HELD_RISE`, so that the outer backs of a large hand, which
 * their turn already lifts, stay in sight. */
const HELD_FOOT = 44;
const HELD_RISE = 16;

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

/** The opponent's hand fanned along the top edge: backs, and the faces of the cards you know
 * (a look at their hand, a card returned to it). */
function heldPlacements(cards: ObjectView[]): Placement[] {
  const n = cards.length;
  const mid = (n - 1) / 2;
  const height = BACK_WIDTH / CARD_ASPECT;
  const total = n * BACK_WIDTH - (n - 1) * 2;
  return cards.map((o, i) => ({
    slot: o.slot,
    zone: o.zone,
    look: "held",
    width: BACK_WIDTH,
    span: BACK_WIDTH,
    opacity: 1,
    z: 100 + i,
    ...turned(
      STAGE.width / 2 - total / 2 + i * (BACK_WIDTH - 2),
      HELD_FOOT - height,
      (i - mid) * -4.2,
      [BACK_WIDTH / 2, height / 2],
      -Math.min((i - mid) ** 2 * 3.5, HELD_RISE),
    ),
  }));
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

/** Lands of one name, stacked as Player.tsx's LandStack: copies beneath peek out to the left
 * and below. Callers key tapped and untapped copies apart. */
function piles(objects: ObjectView[], key: (o: ObjectView) => string) {
  const groups = new Map<string, ObjectView[]>();
  for (const o of objects) {
    const k = key(o);
    groups.set(k, [...(groups.get(k) ?? []), o]);
  }
  return [...groups.values()];
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
 * while it fits there or laid from `start`, and the least scale it may shrink to before its
 * groups crowd one another instead (`row`). A row with `overflow` fills its room from its last
 * group back, and its first groups that do not fit there stand in the overflow's room, toward
 * its end. */
interface RowSpec {
  width: number;
  y: number;
  start: number;
  end: number;
  center?: number;
  least: number;
  overflow?: { start: number; end: number };
}

/** How a row's groups, `widths` wide and `gap` apart, divide between a room `room` long and the
 * overflow before it: the first group in the room (of the last groups that fit it, at least
 * one), and how long the groups before it are. */
function split(widths: number[], gap: number, room: number) {
  let first = Math.max(0, widths.length - 1);
  let length = widths[first] ?? 0;
  while (first > 0 && length + gap + widths[first - 1]! <= room) {
    length += gap + widths[--first]!;
  }
  const before = widths.slice(0, first);
  return {
    first,
    overflow:
      before.reduce((a, w) => a + w, 0) + gap * Math.max(0, before.length - 1),
  };
}

/** Whether a permanent is drawn in its side's creatures' row: a creature, as its types say after
 * continuous effects, so an animated land or a crewed Vehicle joins it and leaves it again. */
const isCreature = (o: ObjectView) => o.types.includes("creature");

/** Whether a permanent is drawn in its side's land piles: a land that is not also a creature. */
const isLand = (o: ObjectView) => o.types.includes("land") && !isCreature(o);

/** A pile's cards, bottom first, so that its lowest slot lies on top: the engine offers the
 * lowest slot of interchangeable objects (architecture §7.2) and its payments tap a class of
 * sources from the lowest slot up, so whatever a choice takes from a pile (an untap, a cost's
 * taps, a sacrifice, a target) leaves it from the top, and the cards beneath stay put. */
const topLowest = (pile: ObjectView[]) =>
  [...pile].sort((a, b) => b.slot - a.slot);

/** A pile's slots top first, for the placements of a pile of more than one card. */
const pileOf = (pile: ObjectView[]) =>
  pile.length > 1 ? pile.map((o) => o.slot).reverse() : undefined;

/** One side's battlefield, as MTG Arena lays it out (ROWS): land piles on the left behind the
 * creatures' row, other permanents smaller on the right behind it. A blocker stands across the
 * table from the attacker it blocks among the other side's placements `across`, as combat.png
 * does, an attacker's blockers side by side about its center, so a block reads as a short line;
 * the rest of the row makes room. An attacker keeps its place in the row, standing out of it
 * toward the other side, so declaring one moves no other. The cards each permanent exiled until
 * it leaves (`beneath`, by its slot) lie tucked under it, peeking out to its left, and the Auras
 * and Equipment attached to it (`attached`, by its slot, whoever controls them, which `objects`
 * leaves out) behind it, peeking out to its right; its row makes room for both. Each tapped card
 * takes its turned width (`lying`). */
function battlefieldPlacements(
  side: Side,
  objects: ObjectView[],
  across: Placement[] = [],
  beneath: Map<number, ObjectView[]> = new Map(),
  attached: Map<number, ObjectView[]> = new Map(),
  named: Set<number> = new Set(),
  logOpen = false,
): Placement[] {
  const rows = ROWS[side];
  const room = { ...ROOM, start: logOpen ? BESIDE_LOG : ROOM.start };
  const lands = objects.filter(isLand);
  const out: Placement[] = [];
  // Twins (decisions.ts) with nothing on them share a pile, as lands do, the top card wearing
  // their count, as Arena stacks them; everything else stands alone. Twins are alike in all the
  // table shows (Card.tsx's marks) and the inspector tells, so a sick token and a ready one, a
  // stolen one and its owner's, or one an effect has made a Vehicle and a creature, never share
  // one; and either offers what the engine offers for both, so a click on the top card does
  // what the one beneath would. Face-down permanents, which may be different cards, and those
  // the stack `named`, which must be seen and are offered alone, stand apart.
  const plain = (o: ObjectView) =>
    o.facing === "up" &&
    o.counters.length === 0 &&
    o.damage === 0 &&
    !o.combat &&
    o.attachedTo === null &&
    !beneath.has(o.slot) &&
    !attached.has(o.slot) &&
    !named.has(o.slot);
  const grouped = (permanents: ObjectView[]) => {
    const groups: ObjectView[][] = [];
    for (const o of permanents) {
      const pile = plain(o)
        ? groups.find((g) => plain(g[0]!) && twins(g[0]!, o))
        : undefined;
      if (pile) pile.push(o);
      else groups.push([o]);
    }
    return groups.map(topLowest);
  };
  const creatures = grouped(objects.filter(isCreature));
  const others = grouped(objects.filter((o) => !isCreature(o) && !isLand(o)));
  // The back row's other permanents that overflow their room right of the orb stand left of
  // it, past the land piles, which crowd together for them, down to `LANDS_SQUEEZE` of their
  // length, as far as the others need at their least scale.
  const landScale = depthScale(rows.back);
  const otherWidth = rows.others.width;
  const othersLeast = Math.min(
    depthScale(rows.back),
    OTHERS_LEAST / otherWidth,
  );
  const { overflow: deficit } = split(
    groupWidths(others, otherWidth, beneath, attached).map(
      (w) => w * othersLeast,
    ),
    PERMANENT_GAP * otherWidth * othersLeast,
    rows.others.end - rows.others.x,
  );

  // Each name's untapped lands, then its tapped ones beside them.
  const name = (o: ObjectView) => o.name ?? `#${o.slot}`;
  const names = [...new Set(lands.map(name))];
  const landPiles = piles(
    [...lands].sort(
      (a, b) =>
        names.indexOf(name(a)) - names.indexOf(name(b)) ||
        Number(a.tapped) - Number(b.tapped),
    ),
    (o) => `${name(o)}|${o.tapped}`,
  ).map(topLowest);
  // Each copy beneath a pile's top card peeks out by a step of its frame's size.
  const pileWidth = (pile: ObjectView[]) => {
    const { frame, span } = lying(LAND_WIDTH, pile[0]!.tapped);
    return (span + frame * LAND_STEP.x * (pile.length - 1)) * landScale;
  };
  const landGap = LAND_GAP * landScale;
  const landsLength =
    landPiles.reduce((a, pile) => a + pileWidth(pile), 0) +
    landGap * Math.max(0, landPiles.length - 1);
  const landXs = row(
    landPiles.map(pileWidth),
    landGap,
    rows.lands,
    Math.min(
      LANDS_END,
      Math.max(
        rows.lands + landsLength * LANDS_SQUEEZE,
        rows.others.overflowEnd - 2 * landGap - deficit,
      ),
    ),
  );
  // Where the land piles end: the back row's other permanents may stand from there to the orb.
  let landsEnd = rows.lands;
  landPiles.forEach((pile, p) => {
    landsEnd = landXs[p]! + pileWidth(pile);
    const { frame, span } = lying(LAND_WIDTH, pile[0]!.tapped);
    pile.forEach((o, i) => {
      const depth = pile.length - 1 - i;
      const x = landXs[p]! + i * frame * LAND_STEP.x * landScale;
      const y = rows.back + depth * frame * LAND_STEP.y * landScale;
      out.push({
        slot: o.slot,
        zone: o.zone,
        look: "land",
        width: frame,
        span,
        opacity: 1,
        z: 10 + p * 8 + i,
        pile: pileOf(pile),
        ...onTable(x, y, span),
      });
    });
  });

  const front = permanentRow(
    side,
    creatures,
    {
      width: CREATURE_WIDTH,
      y: rows.permanents,
      start: room.start,
      end: room.end,
      center: room.center,
      least: 0,
    },
    across,
    beneath,
    attached,
  );
  const behind = permanentRow(
    side,
    others,
    {
      width: otherWidth,
      y: rows.back,
      start: rows.others.x,
      end: rows.others.end,
      // Toward the stack's column, clear of the hand's middle.
      center: Infinity,
      least: othersLeast,
      overflow: { start: landsEnd + 2 * landGap, end: rows.others.overflowEnd },
    },
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
  const unitGap = PERMANENT_GAP * frameWidth;
  const unitLength = rowLength(groups, frameWidth, beneath, attached);
  // A row too long for its room at its depth's scale shrinks, all its cards alike, down to its
  // least scale; past that, `row` crowds its groups together.
  const full = depthScale(spec.y);
  const least = Math.min(full, spec.least);
  // The land piles crowd together to leave the overflow exactly the room it needs, which
  // rounding may shave by a hair: within `SLACK`, it fits.
  const overflowRoom = spec.overflow
    ? spec.overflow.end - spec.overflow.start + SLACK
    : 0;
  const overflow =
    spec.overflow && overflowRoom >= frameWidth * least
      ? spec.overflow
      : undefined;
  // Without an overflow, as large as fits its room; with one, the largest scale, in steps, at
  // which the groups that overflow the room fit the overflow's.
  let scale = Math.max(
    least,
    Math.min(full, (spec.end - spec.start) / unitLength),
  );
  if (overflow) {
    const fits = (at: number) =>
      split(
        unitWidths.map((w) => w * at),
        unitGap * at,
        spec.end - spec.start,
      ).overflow <= overflowRoom;
    scale = full;
    for (let k = 1; k <= 20 && !fits(scale); k++) {
      scale = full - ((full - least) * k) / 20;
    }
  }
  // A shrunken row keeps its edge toward its player where a full-size one has it, leaving the
  // lane between the rows, where combat and the stack's arrows run, as wide as it can be.
  const rowTop = spec.y + (side === "you" ? (full - scale) * height : 0);
  const width = frameWidth * scale;
  const step = { x: unitStep.x * scale, y: unitStep.y * scale };
  const tucked = (g: ObjectView[]) => peeking(g, frameWidth, beneath) * scale;
  const widths = unitWidths.map((w) => w * scale);
  const gap = PERMANENT_GAP * width;
  const { start, end } = spec;
  // The last groups that fit the room, at least one; those before them overflow.
  const first = overflow ? split(widths, gap, end - start).first : 0;
  const xs = [
    // Toward the overflow's end, beside the room.
    ...(overflow
      ? row(widths.slice(0, first), gap, overflow.start, overflow.end, Infinity)
      : []),
    ...row(widths.slice(first), gap, start, end, spec.center),
  ];
  // Where each blocker wants to stand: its attacker's blockers side by side about its center.
  const attackers = new Map(across.map((p) => [p.slot, p]));
  const blocking = new Map<number, number[]>();
  for (const o of groups.flat()) {
    const attacker = o.combat?.kind === "blocking" ? o.combat.attacker : null;
    if (attacker === null || !attackers.has(attacker)) continue;
    blocking.set(attacker, [...(blocking.get(attacker) ?? []), o.slot]);
  }
  // Each also faces its attacker straight across, which is where it stands when its row is too
  // full to lean it.
  const wants = new Map<number, number>();
  const faces = new Map<number, number>();
  for (const [attacker, blockers] of blocking) {
    const a = attackers.get(attacker)!;
    // Across the table, a little toward its middle, as combat.png's perspective draws it.
    const below = a.x + (a.span * a.scale) / 2;
    const center = below + (STAGE.width / 2 - below) * ACROSS_LEAN;
    blockers.forEach((slot, i) => {
      const offset = (i - (blockers.length - 1) / 2) * width * 1.04;
      wants.set(slot, center - width / 2 + offset);
      faces.set(slot, below - width / 2 + offset);
    });
  }
  const wanted = groups.map((g, i) => wants.get(g[0]!.slot) ?? xs[i]!);
  // With no blockers, every group stands where `row` put it.
  const { order, lefts } =
    blocking.size === 0
      ? { order: groups.map((_, i) => i), lefts: wanted }
      : arrange(
          wanted,
          groups.map((g) => faces.get(g[0]!.slot)),
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
      // Untapped, each a step further up and to the left, or for what is attached to it to
      // the right, centered on the permanent. What is attached rises above what lies beneath,
      // `above` steps up, so that each name bar shows whole.
      const center = x + (span * scale) / 2;
      const tuck = (
        under: ObjectView[] | undefined,
        look: "beneath" | "attached",
        toward: -1 | 1,
        above = 0,
      ) =>
        under?.forEach((e, j) => {
          const n = j + 1;
          const up = above + n;
          out.push({
            slot: e.slot,
            zone: e.zone,
            look,
            width: frameWidth,
            span: frameWidth,
            opacity: 1,
            z: z - up,
            ...onTable(
              center +
                (toward * n * frameWidth * BENEATH_STEP.x - frameWidth / 2) *
                  scale,
              y,
              frameWidth,
              up * height * BENEATH_STEP.y,
              scale,
            ),
          });
        });
      const exiled = beneath.get(o.slot);
      tuck(exiled, "beneath", -1);
      tuck(attached.get(o.slot), "attached", 1, exiled?.length);
    });
  });
  return out;
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
  zone: "graveyard" | "exile";
  /** Bottom first: the last lies on top. */
  cards: ObjectView[];
  look: PileLook;
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
        !(o.exiledBy !== null && holding.has(o.exiledBy)),
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

/** Every object's placement. The stack is `lifted` over a prompt; the hand is `tucked` under a
 * screen over the table; the permanents' rows keep clear of the game log while it is
 * `logOpen` over them. Hovering changes none of it (CardLayer raises a hovered hand card). */
export function layout(
  view: PlayerView,
  lifted = false,
  tucked = false,
  logOpen = false,
): Layout {
  const out: Placement[] = [];
  const objects = view.objects;
  const owned = (side: Side, zone: ZoneName) =>
    objects.filter((o) => o.owner === side && o.zone === zone);
  const bySlot = new Map(objects.map((o) => [o.slot, o]));
  const permanents = view.battlefield.flatMap((s) => bySlot.get(s) ?? []);
  // What you can play from outside your hand waits at the right of your hand fan.
  const elsewhere = playableElsewhere(view).flatMap((s) => bySlot.get(s) ?? []);
  const inHand = new Set(elsewhere.map((o) => o.slot));
  // The cards each permanent exiled until it leaves (CR 610.3), by the permanent's slot.
  const beneath = new Map<number, ObjectView[]>();
  for (const o of objects) {
    if (o.zone !== "exile" || o.exiledBy === null || inHand.has(o.slot)) {
      continue;
    }
    beneath.set(o.exiledBy, [...(beneath.get(o.exiledBy) ?? []), o]);
  }
  const attached = attachments(view);
  const named = referenced(view);
  const behindHost = new Set([...attached.values()].flat().map((o) => o.slot));

  out.push(...heldPlacements(owned("opponent", "hand")));
  // Blockers stand across from their attackers, which stand where they would anyway: only the
  // defending side's row depends on the other's.
  const field = (side: Side, across?: Placement[]) =>
    battlefieldPlacements(
      side,
      permanents.filter(
        (o) => o.controller === side && !behindHost.has(o.slot),
      ),
      across,
      beneath,
      attached,
      named,
      logOpen,
    );
  const yours = field("you", field("opponent"));
  out.push(...yours, ...field("opponent", yours));
  // A raised hand card keeps clear of your back row: your lands and the permanents beside them.
  const backRow = (p: Placement) => {
    const o = bySlot.get(p.slot);
    return p.look === "land" || (p.look === "field" && !!o && !isCreature(o));
  };
  out.push(
    ...handLifts(
      handPlacements([...owned("you", "hand"), ...elsewhere], tucked),
      yours.filter(backRow).map(footprint),
    ),
  );
  for (const side of ["opponent", "you"] as const) {
    const player = view.players[side];
    const library = pileLook(side, PILES[side].library, player.library.length);
    // Library cards wait, unseen, on their library.
    for (const o of owned(side, "library")) {
      if (inHand.has(o.slot)) continue;
      out.push({
        slot: o.slot,
        zone: o.zone,
        look: "hidden",
        width: FULL_WIDTH,
        span: FULL_WIDTH,
        opacity: 0,
        z: 1,
        ...onPile(library),
      });
    }
  }
  for (const pile of zonePiles(view)) {
    pile.cards.forEach((o, i) => {
      out.push({
        slot: o.slot,
        zone: o.zone,
        look: "pile",
        width: FULL_WIDTH,
        span: FULL_WIDTH,
        opacity: 1,
        z: 20 + i,
        ...onPile(pile.look),
      });
    });
  }
  view.stack.forEach((item, i) => {
    if (item.kind !== "spell") return;
    const o = bySlot.get(item.slot);
    if (!o) return;
    const place = stackPlace(i, view.stack.length, lifted);
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
  return new Map(out.map((p) => [p.slot, p]));
}

/** The stack's items are drawn over everything else on the table, the top item in front. */
export const STACK_Z = 400;

/** The compact phase tracker's phase for a step. */
export function phaseOf(step: Step): Phase {
  switch (step) {
    case "untap":
    case "upkeep":
    case "draw":
      return "beginning";
    case "precombatMain":
      return "main1";
    case "postcombatMain":
      return "main2";
    case "end":
    case "cleanup":
      return "end";
    default:
      return "combat";
  }
}

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

/** The turn button for passing priority (decision D13): where the pass goes, as the engine
 * says (`DecisionView.passTo`). */
export function passButton(
  view: PlayerView,
  to: PassTo,
): { label: string; caption: string } {
  switch (to.kind) {
    case "resolve": {
      const top = view.stack[view.stack.length - 1];
      const name =
        top?.kind === "spell"
          ? (view.objects.find((o) => o.slot === top.slot)?.name ?? "the spell")
          : (top?.name ?? "the stack");
      return { label: "Resolve", caption: name };
    }
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
