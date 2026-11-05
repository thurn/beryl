// Where every card of a PlayerView lies on the 1600×900 stage, as in docs/ui_mockups/
// battlefield.png. Each object is placed by its public slot, which it keeps from zone to zone
// (docs/engine-architecture.md §13), so the game screen draws one element per slot and a card
// moving zones glides from its old place to its new one rather than jumping. Pure functions of
// the view: the screen animates between their results.
import type {
  ObjectView,
  PlayerView,
  Side,
  Step,
  ZoneName,
} from "../generated/protocol";
import type { Phase } from "../components/Controls";
import {
  CREATURE_WIDTH,
  depthScale,
  HAND,
  LAND_STEP,
  LAND_WIDTH,
  lean,
  STAGE,
} from "../screens/Table";

/** How a placed card is drawn. */
export type Look =
  | "hand" // a full frame in your hand
  | "back" // a card back: the opponent's hand
  | "field" // a battlefield frame: nonland permanents
  | "land" // a battlefield frame in a land pile
  | "stack" // a full frame on the stack
  | "pile" // a full frame on a graveyard
  | "hidden"; // nothing drawn: libraries and exile

/** One card on the stage: its element's top-left corner at (x, y), then scaled, turned and
 * sheared about that corner, in that order (Motion's transform order). */
export interface Placement {
  slot: number;
  zone: ZoneName;
  look: Look;
  /** The frame's width before scaling. */
  width: number;
  x: number;
  y: number;
  scale: number;
  rotate: number;
  skew: number;
  opacity: number;
  z: number;
}

/** A whole board: every object's placement, by slot. */
export type Layout = Map<number, Placement>;

/** The hand's full frames, and the width of everything drawn from a full frame. */
export const FULL_WIDTH = HAND.width;
const BACK_WIDTH = 105;
export const PILE_WIDTH = 91;
/** Where a graveyard's bottom card lies under its top card on the table: straight below, so the
 * pile shows only as thickness (battlefield.png). */
export const PILE_DEPTH = { x: 0, y: 8 };
/** How much a graveyard's cards are foreshortened, lying on the far table (battlefield.png). */
export const PILE_SQUASH = 0.86;

/** Each side's rows, in stage pixels (the mockup's divided by 1.045): the land piles from
 * (x, y), `size` times what depth alone makes them (battlefield.png paints the far lands larger),
 * with `gap` between them before depth scaling; the other permanents' row at `y`, centered on
 * `center`, `gap` card widths apart (the far row's closer). */
const ROWS = {
  opponent: {
    lands: { x: 158, y: 124, gap: 30, size: 1.045 },
    permanents: { y: 221, center: 802, gap: 0.044 },
  },
  you: {
    lands: { x: 111, y: 564, gap: 3, size: 1 },
    permanents: { y: 418, center: 801, gap: 0.08 },
  },
};
/** Lands fill from the left up to here; other permanents lie on the table between these. */
const LANDS_END = 600;
const PERMANENTS = { start: 580, end: 1250 };

/** Where each side's library and graveyard lie (Table.tsx draws the library piles). */
export const PILES = {
  opponent: {
    library: { x: 1174, y: 13, width: 93 },
    graveyard: { x: 1327, y: 40 },
  },
  you: {
    library: { x: 168, y: 731, width: 104 },
    graveyard: { x: 300, y: 770 },
  },
};

/** The stack, spells and abilities alike, large at the right of the table as in
 * docs/ui_mockups/trigger_on_stack_with_target.png: the top item in front where the mockup
 * draws its trigger, each item beneath it peeking out above by its name bar, closer together
 * the taller the stack. */
export const STACK = { x: 1205, y: 222, width: 295, step: 46, reach: 200 };

/** How far the stack rises to clear the prompt below it, while one is open. */
const STACK_LIFT = 90;

/** Where the stack item at `index` (bottom first) of `count` is drawn, `lifted` or not. */
export function stackPlace(index: number, count: number, lifted = false) {
  const step = count > 1 ? Math.min(STACK.step, STACK.reach / (count - 1)) : 0;
  return {
    x: STACK.x,
    y: STACK.y - (lifted ? STACK_LIFT : 0) - (count - 1 - index) * step,
    width: STACK.width,
  };
}

/** How far attackers stand out of their row toward the other side, so an attack reads at a
 * glance; the rows are too close for more. */
const ADVANCE = 16;

/** Places a thing `width` wide (before its depth scaling) lying on the table at (x, y). */
function onTable(x: number, y: number, width: number) {
  const scale = depthScale(y);
  return { x, y, scale, rotate: 0, skew: lean(x + (width * scale) / 2, y) };
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

const FULL_ASPECT = 63 / 88;

/** The hand fan (Table.tsx's Hand): the more cards, the closer they sit. `raised` straightens
 * and lifts one card, as hovering does. */
function handPlacements(
  cards: ObjectView[],
  raised: number | undefined,
): Placement[] {
  const n = cards.length;
  const mid = (n - 1) / 2;
  const spacing = n > 1 ? Math.min(HAND.spacing, 760 / (n - 1)) : 0;
  return cards.map((o, i) => {
    const left = STAGE.width / 2 + (i - mid) * spacing - FULL_WIDTH / 2;
    const top = HAND.top + (i - mid) ** 2 * HAND.sag;
    const lift = o.slot === raised;
    const place = lift
      ? { x: left - 5, y: top - 70, scale: 1.06, rotate: 0, skew: 0 }
      : turned(left, top, (i - mid) * HAND.turn + HAND.bias, [
          FULL_WIDTH / 2,
          0,
        ]);
    return {
      slot: o.slot,
      zone: o.zone,
      look: "hand",
      width: FULL_WIDTH,
      opacity: 1,
      z: lift ? 300 : 200 + i,
      ...place,
    };
  });
}

/** The opponent's hand: backs fanned along the top edge (Table.tsx's opponent hand). */
function backPlacements(cards: ObjectView[]): Placement[] {
  const n = cards.length;
  const mid = (n - 1) / 2;
  const height = BACK_WIDTH / FULL_ASPECT;
  const total = n * BACK_WIDTH - (n - 1) * 2;
  return cards.map((o, i) => ({
    slot: o.slot,
    zone: o.zone,
    look: "back",
    width: BACK_WIDTH,
    opacity: 1,
    z: 100 + i,
    ...turned(
      STAGE.width / 2 - total / 2 + i * (BACK_WIDTH - 2),
      -87.5,
      (i - mid) * -4.2,
      [BACK_WIDTH / 2, height / 2],
      -((i - mid) ** 2) * 3.5,
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

/** Moves a row's groups, in order, as near as they can be to the lefts they `want`, spaced at
 * least as `row` would space them and inside `start` to `end`: each pushed right of its left
 * neighbor, then left of the end and its right neighbor, then right of the start again. */
function settle(
  wants: number[],
  widths: number[],
  gap: number,
  start: number,
  end: number,
): number[] {
  const n = wants.length;
  const { steps: step } = steps(widths, gap, end - start);
  const xs = [...wants];
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

/** Lands of one name (or tokens of one name), stacked as Table.tsx's LandStack: copies beneath
 * peek out to the left and below; tapped copies lie on top. */
function piles(objects: ObjectView[], key: (o: ObjectView) => string) {
  const groups = new Map<string, ObjectView[]>();
  for (const o of objects) {
    const k = key(o);
    groups.set(k, [...(groups.get(k) ?? []), o]);
  }
  return [...groups.values()].map((g) =>
    [...g].sort((a, b) => Number(a.tapped) - Number(b.tapped)),
  );
}

/** One side's battlefield: land piles on the left, other permanents in the middle. A blocker
 * stands across the table from the attacker it blocks among the other side's placements
 * `across`, as combat.png does, an attacker's blockers side by side about its center, so a
 * block reads as a short line; the rest of the row makes room. */
function battlefieldPlacements(
  side: Side,
  objects: ObjectView[],
  across: Placement[] = [],
): Placement[] {
  const rows = ROWS[side];
  const lands = objects.filter((o) => o.types.includes("land"));
  const others = objects.filter((o) => !o.types.includes("land"));
  const out: Placement[] = [];

  const landPiles = piles(lands, (o) => o.name ?? `#${o.slot}`);
  const landScale = depthScale(rows.lands.y) * rows.lands.size;
  const pileWidth = (n: number) =>
    LAND_WIDTH * (1 + LAND_STEP.x * (n - 1)) * landScale;
  const landXs = row(
    landPiles.map((p) => pileWidth(p.length)),
    rows.lands.gap * landScale,
    rows.lands.x,
    LANDS_END,
  );
  landPiles.forEach((pile, p) => {
    pile.forEach((o, i) => {
      const depth = pile.length - 1 - i;
      const x = landXs[p]! + i * LAND_WIDTH * LAND_STEP.x * landScale;
      const y = rows.lands.y + depth * LAND_WIDTH * LAND_STEP.y * landScale;
      const placed = onTable(x, y, LAND_WIDTH);
      out.push({
        slot: o.slot,
        zone: o.zone,
        look: "land",
        width: LAND_WIDTH,
        opacity: 1,
        z: 10 + p * 8 + i,
        ...placed,
        scale: placed.scale * rows.lands.size,
      });
    });
  });

  // Identical untouched tokens share a pile, as lands do; everything else stands alone.
  const plain = (o: ObjectView) =>
    o.token && o.counters.length === 0 && o.damage === 0 && !o.combat;
  const groups = piles(others, (o) =>
    plain(o) ? `${o.name}|${o.tapped}` : `#${o.slot}`,
  );
  const scale = depthScale(rows.permanents.y);
  const width = CREATURE_WIDTH * scale;
  const groupWidth = (n: number) => width * (1 + LAND_STEP.x * (n - 1));
  const widths = groups.map((g) => groupWidth(g.length));
  const gap = rows.permanents.gap * CREATURE_WIDTH * scale;
  const xs = row(
    widths,
    gap,
    PERMANENTS.start,
    PERMANENTS.end,
    rows.permanents.center,
  );
  // Where each blocker wants to stand: its attacker's blockers side by side about its center.
  const attackers = new Map(across.map((p) => [p.slot, p]));
  const blocking = new Map<number, number[]>();
  for (const o of objects) {
    const attacker = o.combat?.kind === "blocking" ? o.combat.attacker : null;
    if (attacker === null || !attackers.has(attacker)) continue;
    blocking.set(attacker, [...(blocking.get(attacker) ?? []), o.slot]);
  }
  const wants = new Map<number, number>();
  for (const [attacker, blockers] of blocking) {
    const a = attackers.get(attacker)!;
    const center = a.x + (CREATURE_WIDTH * a.scale) / 2;
    blockers.forEach((slot, i) => {
      const offset = (i - (blockers.length - 1) / 2) * width * 1.04;
      wants.set(slot, center - width / 2 + offset);
    });
  }
  const wanted = groups.map((g, i) => wants.get(g[0]!.slot) ?? xs[i]!);
  const order = groups.map((_, i) => i).sort((a, b) => wanted[a]! - wanted[b]!);
  const lefts = settle(
    order.map((g) => wanted[g]!),
    order.map((g) => widths[g]!),
    gap,
    PERMANENTS.start,
    PERMANENTS.end,
  );
  const forward = side === "you" ? -ADVANCE : ADVANCE;
  order.forEach((g, k) => {
    const group = groups[g]!;
    group.forEach((o, i) => {
      const depth = group.length - 1 - i;
      const x = lefts[k]! + i * CREATURE_WIDTH * LAND_STEP.x * scale;
      const y =
        rows.permanents.y +
        depth * CREATURE_WIDTH * LAND_STEP.y * scale +
        (o.combat?.kind === "attacking" ? forward : 0);
      out.push({
        slot: o.slot,
        zone: o.zone,
        look: "field",
        width: CREATURE_WIDTH,
        opacity: 1,
        // Combat's creatures over their neighbors, so their marks show.
        z: 40 + k * 8 + i + (o.combat ? 60 : 0),
        ...onTable(x, y, CREATURE_WIDTH),
      });
    });
  });
  return out;
}

/** A pile lying at a fixed place, as Table.tsx's `Lying` draws it: unscaled, leaning. */
function lying(x: number, y: number, width: number) {
  return { x, y, scale: 1, rotate: 0, skew: lean(x + width / 2, y) };
}

/** Every object's placement. `raised` is the hand card the pointer is over; the stack is
 * `lifted` over a prompt. */
export function layout(
  view: PlayerView,
  raised?: number,
  lifted = false,
): Layout {
  const out: Placement[] = [];
  const objects = view.objects;
  const owned = (side: Side, zone: ZoneName) =>
    objects.filter((o) => o.owner === side && o.zone === zone);
  const bySlot = new Map(objects.map((o) => [o.slot, o]));
  const permanents = view.battlefield.flatMap((s) => bySlot.get(s) ?? []);

  out.push(...handPlacements(owned("you", "hand"), raised));
  out.push(...backPlacements(owned("opponent", "hand")));
  // Blockers stand across from their attackers, which stand where they would anyway: only the
  // defending side's row depends on the other's.
  const field = (side: Side, across?: Placement[]) =>
    battlefieldPlacements(
      side,
      permanents.filter((o) => o.controller === side),
      across,
    );
  const yours = field("you", field("opponent"));
  out.push(...yours, ...field("opponent", yours));
  for (const side of ["opponent", "you"] as const) {
    const { library, graveyard } = PILES[side];
    // Library cards wait, unseen, on their library; exiled cards fade out over the graveyard.
    for (const o of owned(side, "library")) {
      out.push({
        slot: o.slot,
        zone: o.zone,
        look: "hidden",
        width: FULL_WIDTH,
        opacity: 0,
        z: 1,
        ...lying(library.x, library.y, library.width),
        scale: library.width / FULL_WIDTH,
      });
    }
    const pile = view.players[side].graveyard;
    pile.forEach((slot, i) => {
      const o = bySlot.get(slot);
      if (!o) return;
      out.push({
        slot,
        zone: o.zone,
        look: "pile",
        width: FULL_WIDTH,
        opacity: 1,
        z: 20 + i,
        ...lying(graveyard.x, graveyard.y, PILE_WIDTH),
        scale: PILE_WIDTH / FULL_WIDTH,
      });
    });
    for (const o of owned(side, "exile")) {
      out.push({
        slot: o.slot,
        zone: o.zone,
        look: "hidden",
        width: FULL_WIDTH,
        opacity: 0,
        z: 1,
        ...lying(graveyard.x, graveyard.y - 40, PILE_WIDTH),
        scale: (PILE_WIDTH / FULL_WIDTH) * 0.6,
      });
    }
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

const STEPS = Object.keys(STEP_NAMES) as Step[];

/** What passing priority advances to, for the turn button (decision D13): with a spell or
 * ability on the stack, its resolution; otherwise the next stop, the smart stops of
 * docs/engine-architecture.md §15 ("their own main phases, after attackers and after blockers
 * are declared, the opponent's end step") or, under full control, the next step. */
export function nextStop(view: PlayerView): { label: string; caption: string } {
  const { step, active } = view.turn;
  const top = view.stack[view.stack.length - 1];
  if (top) {
    const name =
      top.kind === "spell"
        ? (view.objects.find((o) => o.slot === top.slot)?.name ?? "the spell")
        : top.name;
    return { label: "Resolve", caption: name };
  }
  // Once blockers are declared, passing deals combat damage (combat.png's Resolve Damage).
  const fighting = view.objects.some((o) => o.combat?.kind === "attacking");
  if (
    fighting &&
    (step === "declareBlockers" || step === "firstStrikeDamage")
  ) {
    return { label: "Resolve Damage", caption: "Combat damage" };
  }
  if (view.stops === "exact") {
    const next = STEPS[(STEPS.indexOf(step) + 1) % STEPS.length]!;
    return { label: "Next", caption: `To ${STEP_NAMES[next]}` };
  }
  if (fighting && step === "declareAttackers") {
    return { label: "Next", caption: "To Blockers" };
  }
  if (active === "opponent") {
    return step === "end" || step === "cleanup"
      ? { label: "Next", caption: "To Your Turn" }
      : { label: "Next", caption: "To End Step" };
  }
  const canAttack = view.objects.some(
    (o) =>
      o.zone === "battlefield" &&
      o.controller === "you" &&
      o.pt !== null &&
      !o.tapped &&
      !o.summoningSick,
  );
  switch (step) {
    case "untap":
    case "upkeep":
    case "draw":
      return { label: "Next", caption: "To Main 1" };
    case "precombatMain":
      return canAttack
        ? { label: "Next", caption: "To Combat" }
        : { label: "Next", caption: "To Main 2" };
    case "postcombatMain":
    case "end":
    case "cleanup":
      return { label: "End Turn", caption: "To Opponent's Turn" };
    default:
      return { label: "Next", caption: "To Main 2" };
  }
}
