// Where every card of a PlayerView lies on a portrait phone, after the approved prototype's
// DESIGN.md (beryl-mobile-prototype): one vertical stack of bands, the opponent's half mirroring
// yours so that each player's creatures sit nearest the middle. Like the desktop's layout.ts,
// pure functions of the view and the viewport, and each object is placed by its public slot,
// which it keeps from zone to zone (docs/engine-architecture.md §13), so the screen draws one
// element per slot and a card changing zones can glide from its old place to its new one.
import type {
  ObjectView,
  PlayerView,
  Side,
  StackItem,
  Step,
} from "../generated/protocol";
import type { Phase } from "../components/Controls";
import { referenced, twins } from "../game/decisions";
import { onArc } from "../game/fanArc";
import { emblemObjects, playableElsewhere } from "../game/zoneViews";
import { creatureHeight, RATIO } from "./sizing.ts";

export { RATIO };
/** The gap between cards in a row. */
export const GAP = 5;
/** How far each copy beneath the top card of a stack of identical permanents peeks out, up and
 * to the right. */
export const PEEK = 3;
/** The page's side gutter. */
export const SIDE = 8;
/** Each player's strip: piles, life orb, and the opponent's hand and menu or your dock. */
export const STRIP = { opponent: 48, you: 60 } as const;
/** The middle: the phase icon on the gold line, and the stack when it holds anything. */
export const MIDDLE = { idle: 44, stack: 110 } as const;
/** The tallest a creature may stand. */
/** Between creature rows. */
const ROW_GAP = 8;
/** The stack's cards: their width and the step between them. */
export const STACK_CARD = { width: 68, step: 44 } as const;

/** A pile in a player's strip, its library and then its graveyard: its card's size and each
 * pile's left edge (phone.css `.phone-piles`). */
export const PILE = {
  width: 26,
  height: 36,
  library: 10,
  graveyard: 46,
} as const;
/** The opponent's hand, a back per card peeking from the top edge of their strip: a back's
 * width, the step between them, how far they rise over the edge, and the room kept at the
 * right for the menu (phone.css `.phone-backs`). */
export const BACKS = { width: 24, step: 14, rise: 10, right: 56 } as const;

/** The space the layout fills: the viewport inside its top safe area, and the bottom safe area
 * (the home bar), which the hand runs under. */
export interface Viewport {
  width: number;
  height: number;
  insetBottom: number;
}

/** A band of the screen, top to bottom. */
export interface Band {
  top: number;
  height: number;
}

export type BandName =
  | "opponentStrip"
  | "opponentLands"
  | "opponentOther"
  | "opponentCreatures"
  | "middle"
  | "yourCreatures"
  | "yourOther"
  | "yourLands"
  | "yourStrip"
  | "hand";

/** Where one card lies: its top-left corner, its untapped width (a tapped card's footprint is
 * turned a quarter, `width` high), its stacking order, and a hand card's turn in the fan, in
 * degrees about its bottom center. */
export interface Spot {
  slot: number;
  zone: "battlefield" | "hand";
  x: number;
  y: number;
  width: number;
  tapped: boolean;
  z: number;
  rotate: number;
}

/** An item on the stack, bottom first: a spell is its card's slot, an ability has none. */
export interface StackSpot {
  item: StackItem;
  index: number;
  x: number;
  y: number;
  width: number;
  z: number;
}

export interface PhoneLayout {
  bands: Record<BandName, Band>;
  cards: Spot[];
  stack: StackSpot[];
}

/** Where the top card of `side`'s library or graveyard lies in their strip. */
export function pileSpot(
  bands: Record<BandName, Band>,
  side: Side,
  zone: "library" | "graveyard",
) {
  const strip = side === "you" ? bands.yourStrip : bands.opponentStrip;
  return {
    x: PILE[zone],
    y: strip.top + (strip.height - PILE.height) / 2,
    width: PILE.width,
  };
}

/** Where the back of the opponent's hand card `index` of `count` lies, on a board `width`
 * wide. */
export function backSpot(
  bands: Record<BandName, Band>,
  width: number,
  index: number,
  count: number,
) {
  const left = width - BACKS.right - BACKS.step * (count - 1) - BACKS.width;
  return {
    x: left + BACKS.step * index,
    y: bands.opponentStrip.top - BACKS.rise,
    width: BACKS.width,
  };
}

/** The phase a step belongs to, for the middle's icon. */
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

/** The permanents on one side, in the rows they stand in. */
interface Rows {
  lands: ObjectView[][];
  other: ObjectView[][];
  creatures: ObjectView[][];
}

/** Each permanent `side` controls in its row, identical ones stacked as one group (one card,
 * the rest peeking from under it): interchangeable as the engine offers them (`twins`), and
 * neither named on the stack, whose glow must show apart. Emblems follow the other permanents. */
function sideRows(view: PlayerView, side: Side, named: Set<number>): Rows {
  const bySlot = new Map(view.objects.map((o) => [o.slot, o]));
  const rows: Rows = { lands: [], other: [], creatures: [] };
  for (const slot of view.battlefield) {
    const o = bySlot.get(slot);
    if (!o || o.controller !== side) continue;
    const row = o.types.includes("creature")
      ? rows.creatures
      : o.types.includes("land")
        ? rows.lands
        : rows.other;
    const group = named.has(slot)
      ? undefined
      : row.find((g) => !named.has(g[0]!.slot) && twins(g[0]!, o));
    if (group) group.push(o);
    else row.push([o]);
  }
  for (const emblem of emblemObjects(view)) {
    if (emblem.controller === side) rows.other.push([emblem]);
  }
  return rows;
}

/** A group's footprint with cards `h` high: a tapped card's turned a quarter, and each copy
 * under the top one peeking out by `PEEK` both ways. */
function footprint(group: ObjectView[], h: number): [number, number] {
  const w = h * RATIO;
  const extra = PEEK * (group.length - 1);
  return group[0]!.tapped ? [h + extra, w + extra] : [w + extra, h + extra];
}

/** The tallest card that fits `row` across `width` in one line, at its gaps and peeks. */
function fitAcross(row: ObjectView[][], width: number): number {
  const units = row.reduce((a, g) => a + (g[0]!.tapped ? 1 : RATIO), 0);
  const fixed =
    GAP * (row.length - 1) + row.reduce((a, g) => a + PEEK * (g.length - 1), 0);
  return (width - fixed) / units;
}

/** One row of groups with cards `h` high, centered across `width` in the band from `top`
 * `rowH` high; where it is too wide, its groups overlap evenly rather than scroll or wrap.
 * Later groups lie over earlier ones, and a group's top card over its copies. */
function placeRow(
  groups: ObjectView[][],
  h: number,
  width: number,
  top: number,
  rowH: number,
): Spot[] {
  if (!groups.length) return [];
  const sizes = groups.map((g) => footprint(g, h));
  const total = sizes.reduce((a, [w]) => a + w, 0) + GAP * (groups.length - 1);
  const squeeze =
    total > width && groups.length > 1
      ? (total - width) / (groups.length - 1)
      : 0;
  let x = SIDE + (width - Math.min(total, width)) / 2;
  const out: Spot[] = [];
  groups.forEach((group, i) => {
    const [w, sh] = sizes[i]!;
    const y = top + (rowH - sh) / 2;
    const extra = PEEK * (group.length - 1);
    group.forEach((o, k) =>
      out.push({
        slot: o.slot,
        zone: "battlefield",
        x: x + k * PEEK,
        y: y + extra - k * PEEK,
        width: h * RATIO,
        tapped: o.tapped,
        z: (i + 1) * 64 - k,
        rotate: 0,
      }),
    );
    x += w + GAP - squeeze;
  });
  return out;
}

/** A side's creatures in the band `band`: in one, two or three rows, the groups split evenly,
 * whichever gives the tallest card (up to 128px). The opponent's rows stand at the
 * band's foot with their fullest nearest the middle; yours at its head. */
function placeCreatures(
  groups: ObjectView[][],
  side: Side,
  band: Band,
  width: number,
): Spot[] {
  if (!groups.length) return [];
  const room = band.height - 8;
  let best = { h: 0, rows: [groups] };
  for (let count = 1; count <= 3; count++) {
    const per = Math.ceil(groups.length / count);
    const rows: ObjectView[][][] = [];
    for (let i = 0; i < groups.length; i += per) {
      rows.push(groups.slice(i, i + per));
    }
    if (side === "opponent") rows.reverse();
    const byHeight = (room - (rows.length - 1) * ROW_GAP - 12) / rows.length;
    const byWidth = Math.min(...rows.map((row) => fitAcross(row, width)));
    const h = creatureHeight(byHeight, byWidth);
    if (h > best.h + 1) best = { h, rows };
  }
  const h = Math.floor(best.h);
  const rowH = h + 12;
  const total = best.rows.length * rowH + (best.rows.length - 1) * ROW_GAP;
  let top = side === "opponent" ? band.top + room + 4 - total : band.top + 10;
  const out: Spot[] = [];
  best.rows.forEach((row, r) => {
    for (const spot of placeRow(row, h, width, top, rowH)) {
      out.push({ ...spot, z: spot.z + r * 1000 });
    }
    top += rowH + ROW_GAP;
  });
  return out;
}

/** Your hand as a fan: cards `min(100, W/4)` wide, as close as `W` allows up to 4px apart,
 * centered, the later ones on top; each card's top center lies on the desktop fan's circle
 * (`onArc`), dropped and turned to lie along it. Two thirds of each shows above the bottom
 * edge. */
function placeHand(hand: ObjectView[], width: number, band: Band): Spot[] {
  const cw = handWidth(width);
  const n = hand.length;
  const step = n > 1 ? Math.min(cw + 4, (width - cw) / (n - 1)) : 0;
  const x0 = SIDE + (width - (cw + step * (n - 1))) / 2;
  return hand.map((o, i) => {
    const { sink, turn } = onArc((i - (n - 1) / 2) * step, cw);
    return {
      slot: o.slot,
      zone: "hand",
      x: x0 + i * step,
      y: band.top + 8 + sink,
      width: cw,
      tapped: false,
      z: 10000 + i,
      rotate: turn,
    };
  });
}

/** A hand card's width at content width `width`. */
const handWidth = (width: number) => Math.min(100, width * 0.25);

/** Every card's spot and each band, for `view` on `viewport`. Creatures get the height they
 * can use first; lands and the other permanents share what is left:
 *
 *     oneRow = the tallest card that fits each side's creatures in one row
 *     zMin   = min(110, oneRow) + 24
 *     R      = avail / 2 - zMin
 *     L      = clamp(R × 0.42, 36, 60)   // a land row
 *     O      = clamp(R × 0.58, 48, 88)   // the other permanents' row
 *
 * with `avail` the height left after the strips, the middle and the hand. Cards in those rows
 * stand 8px shorter than the row. */
export function phoneLayout(view: PlayerView, viewport: Viewport): PhoneLayout {
  const width = viewport.width - 2 * SIDE;
  const handH =
    Math.round((handWidth(width) / RATIO) * 0.66) + viewport.insetBottom;
  const midH = view.stack.length ? MIDDLE.stack : MIDDLE.idle;
  const avail = viewport.height - STRIP.opponent - STRIP.you - midH - handH;
  const named = referenced(view);
  const opponent = sideRows(view, "opponent", named);
  const you = sideRows(view, "you", named);
  const oneRow = (rows: Rows) =>
    rows.creatures.length ? fitAcross(rows.creatures, width) : Infinity;
  const zMin = Math.min(110, oneRow(opponent), oneRow(you)) + 24;
  const r = avail / 2 - zMin;
  const lands = Math.round(Math.min(60, Math.max(36, r * 0.42)));
  const other = Math.round(Math.min(88, Math.max(48, r * 0.58)));
  const creatures = Math.max(0, (avail - 2 * lands - 2 * other) / 2);
  const heights: [BandName, number][] = [
    ["opponentStrip", STRIP.opponent],
    ["opponentLands", lands],
    ["opponentOther", other],
    ["opponentCreatures", creatures],
    ["middle", midH],
    ["yourCreatures", creatures],
    ["yourOther", other],
    ["yourLands", lands],
    ["yourStrip", STRIP.you],
    ["hand", handH],
  ];
  const bands = {} as Record<BandName, Band>;
  let top = 0;
  for (const [name, height] of heights) {
    bands[name] = { top, height };
    top += height;
  }
  const row = (groups: ObjectView[][], band: Band) =>
    placeRow(groups, band.height - 8, width, band.top, band.height);
  // What you can play from outside your hand waits at the right of the fan, as on the desktop.
  const bySlot = new Map(view.objects.map((o) => [o.slot, o]));
  const hand = [
    ...view.objects.filter((o) => o.zone === "hand" && o.owner === "you"),
    ...playableElsewhere(view).flatMap((s) => bySlot.get(s) ?? []),
  ];
  const cards = [
    ...row(opponent.lands, bands.opponentLands),
    ...row(opponent.other, bands.opponentOther),
    ...placeCreatures(
      opponent.creatures,
      "opponent",
      bands.opponentCreatures,
      width,
    ),
    ...placeCreatures(you.creatures, "you", bands.yourCreatures, width),
    ...row(you.other, bands.yourOther),
    ...row(you.lands, bands.yourLands),
    ...placeHand(hand, width, bands.hand),
  ];
  const n = view.stack.length;
  const stack = view.stack.map((item, index) => ({
    item,
    index,
    x:
      SIDE +
      width / 2 -
      STACK_CARD.width / 2 +
      (index - (n - 1) / 2) * STACK_CARD.step,
    y: bands.middle.top + (midH - STACK_CARD.width / RATIO) / 2,
    width: STACK_CARD.width,
    z: 20000 + index,
  }));
  return { bands, cards, stack };
}
