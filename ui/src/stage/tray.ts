// Where the cards a prompt holds up lie (table.ts `Tray`): geometry alone, which a layout
// gives its own space and sizes (desktop.ts, phone.ts) and the furniture drawn under the
// cards reads the same way (Tray.tsx). One pile is laid out in rows about the middle of the
// space, as many as it takes. Two are boxes, each a row as long as the pile can come to be,
// so that a box keeps its place whatever is moved: a card goes to the end of its row, and
// the rows close up over one another only where the space is too narrow. The boxes lie side
// by side, or where the layout gives the second a space of its own (a phone, which is
// narrow), one in each.
import type { Place, Point } from "./layout.ts";
import type { Tray } from "./table.ts";

/** A rectangle of the stage. */
export interface Box extends Point {
  width: number;
  height: number;
}

/** The space a layout gives a tray, and the sizes it lays one out at. */
export interface TraySpace extends Box {
  /** The widest and the narrowest a card is drawn, and how wide one is for its height. */
  card: { most: number; least: number; aspect: number };
  /** Between two cards, and between a box's edge and its cards. */
  gap: number;
  /** The strip over a box's cards that says what the pile is. */
  heading: number;
  /** Where the second of two piles lies, where they do not lie side by side. */
  second?: Box;
}

/** A tray laid out: where each card lies, and the box of each pile that has one. */
export interface Laid {
  cards: Map<number, Place>;
  /** A box a pile, in the tray's order; none for a tray of one pile. */
  boxes: Box[];
}

/** The widths a card is tried at, the widest first. */
function* widths({ most, least }: TraySpace["card"]) {
  for (let width = most; width > least; width -= 8) yield width;
  yield least;
}

/** One pile, in rows about the middle of `space`. */
function rows(cards: readonly number[], space: TraySpace): Laid {
  const { gap } = space;
  const heightOf = (width: number) => width / space.card.aspect;
  const count = Math.max(1, cards.length);
  let [width, across, step] = [space.card.least, count, 0];
  for (const tried of widths(space.card)) {
    const fits = Math.max(1, Math.floor((space.width + gap) / (tried + gap)));
    const deep = Math.ceil(count / fits);
    width = tried;
    across = Math.min(count, fits);
    step = tried + gap;
    if (deep * (heightOf(tried) + gap) - gap <= space.height) break;
    if (tried === space.card.least) {
      // Too many for whole cards: the rows the space holds, each closed up over itself.
      const held = Math.max(
        1,
        Math.floor((space.height + gap) / (heightOf(tried) + gap)),
      );
      across = Math.ceil(count / held);
      step = Math.min(step, (space.width - tried) / Math.max(1, across - 1));
    }
  }
  const deep = Math.ceil(count / across);
  const tall = deep * (heightOf(width) + gap) - gap;
  const top = space.y + Math.max(0, (space.height - tall) / 2);
  const laid = new Map<number, Place>();
  cards.forEach((slot, i) => {
    const row = Math.floor(i / across);
    const inRow = Math.min(across, cards.length - row * across);
    const wide = width + (inRow - 1) * step;
    laid.set(slot, {
      x: space.x + (space.width - wide) / 2 + (i % across) * step,
      y: top + row * (heightOf(width) + gap),
      width,
    });
  });
  return { cards: laid, boxes: [] };
}

/** The room each pile of `tray` is laid out for: what it must come to hold, or all the tray
 * holds. */
const roomsOf = (tray: Tray) => {
  const held = tray.piles.reduce((n, pile) => n + pile.cards.length, 0);
  return tray.piles.map((pile) => Math.max(1, pile.room ?? held));
};

/** A pile's cards in a row from the left of its `box`, `step` apart at most: a pile over
 * what it was laid out for closes up within its box. */
function row(
  cards: readonly number[],
  box: Box,
  width: number,
  step: number,
  { gap, heading }: TraySpace,
  into: Map<number, Place>,
) {
  const own = Math.min(
    step,
    (box.width - 2 * gap - width) / Math.max(1, cards.length - 1),
  );
  cards.forEach((slot, i) => {
    into.set(slot, {
      x: box.x + gap + i * own,
      y: box.y + heading + gap,
      width,
    });
  });
}

/** Two piles, each a box as wide as its own space: the first in `space`, the second in
 * `second`. */
function stacked(tray: Tray, space: TraySpace, second: Box): Laid {
  const { gap, heading } = space;
  const heightOf = (width: number) => width / space.card.aspect;
  const spaces = [space, second];
  const rooms = roomsOf(tray);
  const tall = Math.min(space.height, second.height) - heading - 2 * gap;
  const width = Math.max(
    space.card.least,
    Math.min(space.card.most, tall * space.card.aspect),
  );
  const laid: Laid = { cards: new Map(), boxes: [] };
  tray.piles.forEach((pile, at) => {
    const within = spaces[at]!;
    const height = heightOf(width) + heading + 2 * gap;
    const box = {
      x: within.x,
      y: within.y + Math.max(0, (within.height - height) / 2),
      width: within.width,
      height,
    };
    laid.boxes.push(box);
    const step = Math.min(
      width + gap,
      (box.width - 2 * gap - width) / Math.max(1, rooms[at]! - 1),
    );
    row(pile.cards, box, width, step, space, laid.cards);
  });
  return laid;
}

/** Several piles, a box each, side by side about the middle of `space`. */
function boxes(tray: Tray, space: TraySpace): Laid {
  const { gap, heading } = space;
  const heightOf = (width: number) => width / space.card.aspect;
  const rooms = roomsOf(tray);
  const between = rooms.reduce((n, room) => n + room - 1, 0);
  // What is not a card's own width or the step to the next: the boxes' edges and the gaps
  // between the boxes.
  const edges = rooms.length * 2 * gap + (rooms.length - 1) * gap;
  let [width, step] = [space.card.least, 0];
  for (const tried of widths(space.card)) {
    width = tried;
    step = tried + gap;
    const wide = rooms.length * tried + between * step + edges;
    const tall = heightOf(tried) + heading + 2 * gap;
    if (wide <= space.width && tall <= space.height) break;
    if (tried === space.card.least && between > 0) {
      step = Math.min(
        step,
        (space.width - edges - rooms.length * tried) / between,
      );
    }
  }
  const height = heightOf(width) + heading + 2 * gap;
  const wide = rooms.length * width + between * step + edges;
  const y = space.y + Math.max(0, (space.height - height) / 2);
  let x = space.x + (space.width - wide) / 2;
  const laid: Laid = { cards: new Map(), boxes: [] };
  tray.piles.forEach((pile, at) => {
    const box = {
      x,
      y,
      width: width + (rooms[at]! - 1) * step + 2 * gap,
      height,
    };
    laid.boxes.push(box);
    row(pile.cards, box, width, step, space, laid.cards);
    x += box.width + gap;
  });
  return laid;
}

/** `tray` laid out in `space`. */
export function laidOut(tray: Tray, space: TraySpace): Laid {
  if (tray.piles.length === 1) return rows(tray.piles[0]!.cards, space);
  return tray.piles.length === 2 && space.second
    ? stacked(tray, space, space.second)
    : boxes(tray, space);
}

/** Where in a tray something let go lands: in which pile, before the card that lies at `at`
 * there now, its length being the end. */
export interface Landing {
  pile: number;
  at: number;
}

/** Where a card let go at `point` lands in `tray`, laid out in `space`: in the box the point
 * is in, before the first card of that pile whose middle is past it. None outside every box,
 * and in a tray of one pile, which has no order to give. */
export function landing(
  tray: Tray,
  space: TraySpace,
  point: Point,
): Landing | undefined {
  const laid = laidOut(tray, space);
  const pile = laid.boxes.findIndex(
    (box) =>
      point.x >= box.x &&
      point.x <= box.x + box.width &&
      point.y >= box.y &&
      point.y <= box.y + box.height,
  );
  if (pile < 0) return undefined;
  const before = tray.piles[pile]!.cards.filter((slot) => {
    const place = laid.cards.get(slot)!;
    return place.x + place.width / 2 < point.x;
  });
  return { pile, at: before.length };
}
