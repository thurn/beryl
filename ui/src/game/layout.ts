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

/** Where the table's receding edges meet, far above the stage: a card left of center leans
 * left at its near end, and one right of center leans right. */
const VANISHING = { x: STAGE.width / 2, y: -4400 };

/** How far a thing lying on the table with its top-center at (x, y) leans, in degrees: its
 * sides run toward the vanishing point. */
export const lean = (x: number, y: number) =>
  (Math.atan((x - VANISHING.x) / (y - VANISHING.y)) * 180) / Math.PI;

export const CREATURE_WIDTH = 232;
export const LAND_WIDTH = 190;
/** Each copy beneath a land stack's top card, as a fraction of the card's width. */
export const LAND_STEP = { x: 0.103, y: 0.022 };

/** The hand's cards: each card's top center lies on an arc over the near edge, and the fan
 * turns each a little more clockwise than the one on its left. */
export const HAND = {
  width: 182,
  spacing: 172,
  top: 726.5,
  sag: 9,
  turn: 5.9,
  bias: 0.6,
};

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

/** Each side's rows, in stage pixels: its land piles from (x, y), filling rightward to
 * `LANDS_END`, and its other permanents' row at `y`, centered on the table. Every row's cards
 * are scaled by their depth alone (depthScale), keeping the field frame's shape. */
const ROWS: Record<
  Side,
  { lands: { x: number; y: number }; permanents: number }
> = {
  opponent: { lands: { x: 158, y: 124 }, permanents: 221 },
  you: { lands: { x: 111, y: 564 }, permanents: 418 },
};
/** Lands fill from the left up to here, `LAND_GAP` apart before depth scaling. */
const LANDS_END = 600;
const LAND_GAP = 12;
/** The other permanents' rows run from here at the least. */
const PERMANENTS_START = 580;
/** How far apart permanents stand, in frame widths: further while lines cross the table, in
 * combat or with something on the stack, to make room for them. */
const PERMANENT_GAP = { main: 0.06, lines: 0.24 };
/** A crowded row of permanents may run on to here while the table's right is clear; while the
 * stack or a prompt stands there, only to just left of the stack. */
const OPEN_END = 1480;
const STACK_MARGIN = 25;

/** How the table is laid out: lines cross it in combat and while something is on the stack. */
type Mode = keyof typeof PERMANENT_GAP;

/** Where each side's library and graveyard lie: their top cards' top-left corners. */
export const PILES = {
  opponent: {
    library: { x: 1174, y: 13 },
    graveyard: { x: 1327, y: 40 },
  },
  you: {
    library: { x: 168, y: 731 },
    graveyard: { x: 300, y: 770 },
  },
};

/** A pile's top card at full size, before its depth shrinks it. */
const PILE_WIDTH = 104;
/** Where a pile's upright sides meet, below the stage. Seen from above the near edge, the cards
 * beneath a pile's top card show on its side toward this point, and the farther the pile lies
 * from it the more of its side shows: the camera looks at the far table more nearly level. */
const NADIR = { x: STAGE.width / 2, y: 1500 };
/** How far each card beneath a pile's top card lies toward `NADIR`, as a fraction of the way. */
const CARD_RISE = 0.0008;

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

/** How a pile of `count` cards with its top card's top-left corner `at` looks, from where it
 * lies alone: one rule for both sides' libraries and graveyards. */
export function pileLook(
  at: { x: number; y: number },
  count: number,
): PileLook {
  const width = PILE_WIDTH * depthScale(at.y);
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

/** How far a blocker's center leans from its attacker's toward the table's middle. */
const ACROSS_LEAN = 0.16;

const attacking = (o: ObjectView) => o.combat?.kind === "attacking";

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

/** How far the hand sinks under a screen over the table (scry.png). */
const HAND_TUCK = 67;

/** The hand fan, held at the near edge: the more cards, the closer they sit. `raised` straightens
 * and lifts one card, as hovering does; `tucked`, the fan sinks out of the way. */
function handPlacements(
  cards: ObjectView[],
  raised: number | undefined,
  tucked: boolean,
): Placement[] {
  const n = cards.length;
  const mid = (n - 1) / 2;
  const spacing = n > 1 ? Math.min(HAND.spacing, 760 / (n - 1)) : 0;
  return cards.map((o, i) => {
    const left = STAGE.width / 2 + (i - mid) * spacing - FULL_WIDTH / 2;
    const top = HAND.top + (i - mid) ** 2 * HAND.sag + (tucked ? HAND_TUCK : 0);
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

/** The opponent's hand: backs fanned along the top edge. */
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

/** Lands of one name (or tokens of one name), stacked as Player.tsx's LandStack: copies beneath
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
 * block reads as a short line; the rest of the row makes room. A row's attackers stand
 * together, first, so that no other permanent lies hidden between two of them. */
function battlefieldPlacements(
  side: Side,
  objects: ObjectView[],
  mode: Mode,
  end: number,
  across: Placement[] = [],
): Placement[] {
  const rows = ROWS[side];
  const lands = objects.filter((o) => o.types.includes("land"));
  const others = objects.filter((o) => !o.types.includes("land"));
  const out: Placement[] = [];

  const landPiles = piles(lands, (o) => o.name ?? `#${o.slot}`);
  const landScale = depthScale(rows.lands.y);
  const pileWidth = (n: number) =>
    LAND_WIDTH * (1 + LAND_STEP.x * (n - 1)) * landScale;
  const landXs = row(
    landPiles.map((p) => pileWidth(p.length)),
    LAND_GAP * landScale,
    rows.lands.x,
    LANDS_END,
  );
  landPiles.forEach((pile, p) => {
    pile.forEach((o, i) => {
      const depth = pile.length - 1 - i;
      const x = landXs[p]! + i * LAND_WIDTH * LAND_STEP.x * landScale;
      const y = rows.lands.y + depth * LAND_WIDTH * LAND_STEP.y * landScale;
      out.push({
        slot: o.slot,
        zone: o.zone,
        look: "land",
        width: LAND_WIDTH,
        opacity: 1,
        z: 10 + p * 8 + i,
        ...onTable(x, y, LAND_WIDTH),
      });
    });
  });

  // Identical untouched tokens share a pile, as lands do; everything else stands alone.
  const plain = (o: ObjectView) =>
    o.token && o.counters.length === 0 && o.damage === 0 && !o.combat;
  const groups = piles(others, (o) =>
    plain(o) ? `${o.name}|${o.tapped}` : `#${o.slot}`,
  ).sort((a, b) => Number(!attacking(a[0]!)) - Number(!attacking(b[0]!)));
  const scale = depthScale(rows.permanents);
  const width = CREATURE_WIDTH * scale;
  const groupWidth = (n: number) => width * (1 + LAND_STEP.x * (n - 1));
  const widths = groups.map((g) => groupWidth(g.length));
  const gap = PERMANENT_GAP[mode] * width;
  const start = PERMANENTS_START;
  const xs = row(widths, gap, start, end, STAGE.width / 2);
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
    // Across the table, a little toward its middle, as combat.png's perspective draws it.
    const below = a.x + (a.width * a.scale) / 2;
    const center = below + (STAGE.width / 2 - below) * ACROSS_LEAN;
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
    start,
    end,
    order.map((g) => wants.has(groups[g]![0]!.slot)),
  );
  const forward = side === "you" ? -ADVANCE : ADVANCE;
  order.forEach((g, k) => {
    const group = groups[g]!;
    group.forEach((o, i) => {
      const depth = group.length - 1 - i;
      const x = lefts[k]! + i * width * LAND_STEP.x;
      const y =
        rows.permanents +
        depth * width * LAND_STEP.y +
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

/** A full frame drawn as a pile's top card, `shrink` times smaller. */
function onPile(look: PileLook, shrink = 1) {
  return {
    x: look.x,
    y: look.y,
    scale: (look.width / FULL_WIDTH) * shrink,
    rotate: 0,
    skew: look.skew,
  };
}

/** Every object's placement. `raised` is the hand card the pointer is over; the stack is
 * `lifted` over a prompt; the hand is `tucked` under a screen over the table. */
export function layout(
  view: PlayerView,
  raised?: number,
  lifted = false,
  tucked = false,
): Layout {
  const out: Placement[] = [];
  const objects = view.objects;
  const owned = (side: Side, zone: ZoneName) =>
    objects.filter((o) => o.owner === side && o.zone === zone);
  const bySlot = new Map(objects.map((o) => [o.slot, o]));
  const permanents = view.battlefield.flatMap((s) => bySlot.get(s) ?? []);

  out.push(...handPlacements(owned("you", "hand"), raised, tucked));
  out.push(...backPlacements(owned("opponent", "hand")));
  // Blockers stand across from their attackers, which stand where they would anyway: only the
  // defending side's row depends on the other's.
  const stacked = view.stack.length > 0;
  const mode: Mode = combatBanner(view) || stacked ? "lines" : "main";
  // The stack and a prompt stand at the table's right.
  const end = stacked || lifted ? STACK.x - STACK_MARGIN : OPEN_END;
  const field = (side: Side, across?: Placement[]) =>
    battlefieldPlacements(
      side,
      permanents.filter((o) => o.controller === side),
      mode,
      end,
      across,
    );
  const yours = field("you", field("opponent"));
  out.push(...yours, ...field("opponent", yours));
  for (const side of ["opponent", "you"] as const) {
    const player = view.players[side];
    const library = pileLook(PILES[side].library, player.library.length);
    const graveyard = pileLook(PILES[side].graveyard, player.graveyard.length);
    // Library cards wait, unseen, on their library; exiled cards fade out over the graveyard.
    for (const o of owned(side, "library")) {
      out.push({
        slot: o.slot,
        zone: o.zone,
        look: "hidden",
        width: FULL_WIDTH,
        opacity: 0,
        z: 1,
        ...onPile(library),
      });
    }
    player.graveyard.forEach((slot, i) => {
      const o = bySlot.get(slot);
      if (!o) return;
      out.push({
        slot,
        zone: o.zone,
        look: "pile",
        width: FULL_WIDTH,
        opacity: 1,
        z: 20 + i,
        ...onPile(graveyard),
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
        ...onPile(graveyard, 0.6),
        y: graveyard.y - 40,
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
