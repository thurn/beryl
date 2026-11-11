// Where each side's permanents lie: its land piles, creatures' row and back row.
import type { ObjectView, Side } from "../generated/protocol";
import { CARD_ASPECT } from "../components/Card";
import { cardEntry } from "../cards";
import { twins } from "./decisions";
import {
  BACK_ROW_PULL,
  BESIDE_LOG,
  LAND_GAP,
  PERMANENT_GAP,
  ROOM,
  ROWS,
  TAPPED_GAP,
  ZOOM_LEAST,
} from "./tableRows";
import type { Placement } from "./placement";
import {
  BENEATH_STEP,
  CREATURE_WIDTH,
  LAND_STEP,
  LAND_WIDTH,
  footprint,
  onTable,
} from "./placement";
import { STAGE, depthScale, fromMiddle, fromRight } from "./perspective";
import { meets, pileBoxes } from "./pileLook";

/** How far attackers stand out of their row toward the other side, so an attack reads at a
 * glance; the rows are too close for more. */
const ADVANCE = 16;

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
export const isCreature = (o: ObjectView) => o.types.includes("creature");

/** Whether a permanent is drawn in its side's land piles: a land that is not also a creature. */
export const isLand = (o: ObjectView) =>
  o.types.includes("land") && !isCreature(o);

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

/** How many lands a side has before its mana's twins share piles. */
export const LANDS_PILED = 7;

/** Groups one side's permanents `objects` into piles. Twins (decisions.ts) with nothing on them
 * share a pile, the top card wearing their count, as Arena stacks them; everything else stands
 * alone. Twins are alike in all the table shows (Card.tsx's marks), so a sick token and a ready
 * one, a stolen one and its owner's, or two planeswalkers of different loyalty, never share
 * one: a pile hides nothing a player would want to read. Mana piles wait for `LANDS_PILED`
 * lands. Face-down permanents, which may be
 * different cards, and those the stack `named`, which must be seen and are offered alone, stand
 * apart. */
export function sideGroups(
  objects: ObjectView[],
  beneath: Map<number, ObjectView[]>,
  attached: Map<number, ObjectView[]>,
  named: Set<number>,
  pileLands: boolean,
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
  const names = [...new Set(sources.map(name))];
  const order = (a: ObjectView, b: ObjectView) =>
    Number(isLand(b)) - Number(isLand(a)) ||
    names.indexOf(name(a)) - names.indexOf(name(b));
  const sourcePiles = (tapped: boolean) =>
    grouped(sources.filter((o) => o.tapped === tapped).sort(order), pileLands);
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
 * its most crowded row, on either side, needs to fit its room, down to `ZOOM_LEAST`.
 * Back rows pull it no lower than `BACK_ROW_PULL` and fit independently beyond that. */
export function battlefieldZoom(
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
        Math.max(
          BACK_ROW_PULL,
          (rows.lands.end - rows.lands.start) /
            (lands * depthScale(rows.lands.y)),
        ),
      );
    }
    zoom = Math.min(
      zoom,
      rowFit(groups.creatures, rows.creatures, beneath, attached),
      Math.max(
        BACK_ROW_PULL,
        rowFit(groups.others, rows.others, beneath, attached),
      ),
    );
  }
  return Math.max(ZOOM_LEAST, zoom);
}

/** One side's battlefield, as MTG Arena lays it out (ROWS): mana piles on the left behind the
 * creatures' row and other permanents smaller on the right behind it. All rows share
 * `zoom` (`battlefieldZoom`), with back rows fitting further if needed. A blocker stands
 * across the table from the attacker it blocks
 * among the other side's placements `across`, as combat.png does, an attacker's blockers side
 * by side about its center, so a block reads as a short line; the rest of the row makes room.
 * Attackers stand forward in the row, ordered by destination so their lines cross less.
 * Cards exiled until a permanent leaves (`beneath`, by its slot) peek out to its left.
 * The Auras and Equipment attached
 * to it (`attached`, by its slot, whoever controls them, which its groups leave out) behind it,
 * peeking out to its right; its row makes room for both. Each tapped card takes its turned
 * width (`lying`). */
export function battlefieldPlacements(
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
  const landScale = Math.min(
    landFull * zoom,
    (rows.lands.end - rows.lands.start) / run.length,
  );
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
  rowGroups: ObjectView[][],
  spec: RowSpec,
  across: Placement[],
  beneath: Map<number, ObjectView[]>,
  attached: Map<number, ObjectView[]>,
): Placement[] {
  const out: Placement[] = [];
  if (rowGroups.length === 0) return out;
  const acrossBySlot = new Map(across.map((p) => [p.slot, p]));
  const center = (p: Placement) => p.x + (p.span * p.scale) / 2;
  const destination = ([o]: ObjectView[]) => {
    const combat = o?.combat;
    if (combat?.kind !== "attacking" || combat.targetDeparted) return undefined;
    const target =
      combat.planeswalker === null
        ? undefined
        : acrossBySlot.get(combat.planeswalker);
    return target ? center(target) : STAGE.width / 2;
  };
  // Keep the nonattacking groups in order. Only swap attackers' places.
  const advancing = rowGroups
    .flatMap((group) => {
      const x = destination(group);
      return x === undefined ? [] : [{ group, x }];
    })
    .sort((a, b) => a.x - b.x);
  let next = 0;
  const groups = rowGroups.map((group) =>
    destination(group) === undefined ? group : advancing[next++]!.group,
  );
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
  const blocking = new Map<number, number[]>();
  for (const o of groups.flat()) {
    const attacker = o.combat?.kind === "blocking" ? o.combat.attacker : null;
    if (attacker === null || !acrossBySlot.has(attacker)) continue;
    blocking.set(attacker, [...(blocking.get(attacker) ?? []), o.slot]);
  }
  // Each group faces its attacker straight across.
  const wants = new Map<number, number>();
  for (const [attacker, blockers] of blocking) {
    const x = center(acrossBySlot.get(attacker)!);
    blockers.forEach((slot, i) => {
      const offset = (i - (blockers.length - 1) / 2) * width * 1.04;
      wants.set(slot, x - width / 2 + offset);
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
