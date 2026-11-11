// Where every card of a PlayerView lies on the stage (900 high, as wide as the window's shape;
// STAGE), in the table's pseudo-3D perspective (depthScale, lean; no WebGL). Each object is placed by its public slot, which it
// keeps from zone to zone (docs/engine-architecture.md §13), so the game screen draws one
// element per slot and a card moving zones glides from its old place to its new one rather than
// jumping. Pure functions of the view: the screen animates between their results.
import type {
  ObjectView,
  PlayerView,
  Side,
  ZoneName,
} from "../generated/protocol";
import { CARD_ASPECT } from "../components/Card";
import { referenced } from "./decisions";
import type { Layout, Placement } from "./placement";
import { FULL_WIDTH, footprint, inOrder } from "./placement";
import type { HandHold } from "./handLayout";
import { handLifts, handPlacements, heldPlacements } from "./handLayout";
import {
  LANDS_PILED,
  battlefieldPlacements,
  battlefieldZoom,
  isCreature,
  sideGroups,
} from "./battlefield";
import { PILES, onPile, pileLook } from "./pileLook";
import { STAGE, fromMiddle, fromRight } from "./perspective";
import type { Spotlight } from "./stackLayout";
import { showcasePlace, stackPlace } from "./stackLayout";
import type { ZonePile } from "./zoneViews";
import {
  attachments,
  emblemObjects,
  held,
  playableElsewhere,
} from "./zoneViews";

/** How far a targeted card under a pile's top rises out of it, in its own heights. */
const PILE_RISE = 0.6;

/** The same handling can span several private picks or an arrangement. */
export function sameHandling(before: PlayerView, after: PlayerView): boolean {
  const a = before.handling;
  const b = after.handling;
  return (
    a !== undefined &&
    b !== undefined &&
    a.side === b.side &&
    a.zone === b.zone &&
    (a.zone !== "library" ||
      b.zone !== "library" ||
      (a.scry === b.scry && (a.count === null) === (b.count === null)))
  );
}

/** Keep the opponent's hand and a full-library search at their table positions.
 * A limited library look can lift the specific cards it examines. */
export function liftsHandling({ handling }: PlayerView): boolean {
  return (
    handling !== undefined &&
    (handling.zone === "hand"
      ? handling.side === "you"
      : handling.count !== null)
  );
}

/** The group in its initial table order. Keep that order while it is up so private
 * selections and same-zone reorders do not shuffle the backs around on screen. */
export function handlingSlots(
  view: PlayerView,
  handOrder: ReadonlyMap<number, number>,
): number[] {
  const handling = view.handling;
  if (!handling || !liftsHandling(view)) return [];
  if (handling.zone === "library") {
    const library = view.players[handling.side].library;
    return handling.count ? library.slice(-handling.count).reverse() : [];
  }
  const cards = view.objects.filter(
    (o) => o.owner === handling.side && o.zone === "hand",
  );
  return inOrder(cards, handOrder).map((o) => o.slot);
}

/** Limited library looks lift the same backs into a compact group at screen right. */
function handlingPlacements(
  view: PlayerView,
  cards: ObjectView[],
): Placement[] {
  const handling = view.handling;
  if (!handling || cards.length === 0) return [];
  const side = handling.side;
  const opening = view.turn.number === 0;
  const library = handling.zone === "library";
  const columns = Math.min(library ? 5 : 7, cards.length);
  const packet = Math.ceil(cards.length / columns);
  const pile = library
    ? pileLook(side, PILES[side].library, view.players[side].library.length)
    : undefined;
  const width = pile ? pile.width * 1.95 : opening ? 72 : 104;
  const step = width * (library ? 0.27 : 0.72);
  const center = library ? fromRight(1445) : fromMiddle(opening ? 465 : 800);
  const top = library ? 144 : opening ? 8 : side === "you" ? 575 : 222;
  return cards.map((card, i) => {
    const column = i % columns;
    const depth = Math.floor(i / columns);
    const dx = column - (columns - 1) / 2;
    return {
      slot: card.slot,
      zone: card.zone,
      look: "handling",
      x: center - width / 2 + dx * step + depth,
      y: top + Math.abs(dx) * 3 + depth * 1.5,
      width: FULL_WIDTH,
      span: FULL_WIDTH,
      scale: width / FULL_WIDTH,
      rotate: library ? 0 : dx * 2,
      skew: 0,
      opacity: 1,
      z: SHOWCASE_Z + column * packet + packet - depth,
      rim: depth > 0,
      back: card.zone === "library" || card.owner === "opponent",
    };
  });
}

/** The parked layers and their positions are also the start/end points for card flights. A
 * borrowed card keeps a home for its handoff, while the cards left on the table settle. */
function pileCards(
  side: Side,
  zone: ZonePile["zone"],
  cards: ObjectView[],
  borrowed: ReadonlySet<number>,
  aimed: ReadonlySet<number>,
  library?: {
    top?: number;
    shuffle?: boolean;
    scryReturn?: Spotlight["scryReturn"];
  },
): Placement[] {
  const full = pileLook(side, PILES[side][zone], cards.length);
  const remaining = cards.filter((o) => !borrowed.has(o.slot)).length;
  const rest = pileLook(side, PILES[side][zone], remaining);
  let depthLeft = remaining;
  return cards.map((o, i) => {
    const away = borrowed.has(o.slot);
    const depth = away ? cards.length - 1 - i : --depthLeft;
    const at = onPile(away ? full : rest, side, depth);
    if (library?.scryReturn?.bottom.has(o.slot)) {
      // Approach the lower edge from the outside of the pile. The final flight
      // keeps the pile's depth order, so these backs slide under its rim.
      const spread = Math.min(10, (cards.length - 1 - depth) * 3);
      return {
        ...at,
        slot: o.slot,
        zone: o.zone,
        look: "scry-bottom",
        width: FULL_WIDTH,
        span: FULL_WIDTH,
        opacity: 1,
        z: 80 - Math.min(depth, 79),
        back: true,
        x: full.x + full.width * 1.1 + spread,
        y:
          side === "opponent"
            ? full.y + (full.width / CARD_ASPECT) * 0.9 + spread
            : at.y,
      };
    }
    const raised = !library && aimed.has(o.slot) && depth > 0;
    // A shuffle moves two packets toward the open table, keeping all faces down.
    const split =
      library?.shuffle && remaining > 1 ? Math.ceil(remaining / 2) : 0;
    const cut = depth < split;
    const lift = cut
      ? -12
      : raised
        ? ((side === "you" ? -1 : 1) * rest.width * PILE_RISE) / CARD_ASPECT
        : 0;
    // Top-bound cards may still be in the air. Keep the back underneath them painted
    // until they cover it, instead of showing the solid edge of a buried card.
    const top = library?.scryReturn?.top;
    const visibleDepth = top?.has(o.slot) ? depth : depth - (top?.size ?? 0);
    const hidden = visibleDepth >= 64 && !raised;
    return {
      ...at,
      slot: o.slot,
      zone: o.zone,
      look: hidden ? "hidden" : "pile",
      width: FULL_WIDTH,
      span: FULL_WIDTH,
      opacity: hidden ? 0 : 1,
      z: raised ? 92 : (cut ? 96 : 80) - Math.min(depth, 79),
      rim: visibleDepth > 1 && !raised && depth !== split,
      back:
        !!library &&
        (library.shuffle || (!away && o.slot !== library.top) || !o.name),
      librarySide: library ? side : undefined,
      lendingFace: !!library && away && !!o.name,
      x: at.x + (cut ? (side === "you" ? 1 : -1) * rest.width * 0.68 : 0),
      y: at.y + lift,
    };
  });
}

/** Every object's placement. The hand is tucked under a screen over the table or raised for a
 * choice of its cards (`hold`); the permanents' rows keep clear of the game log while it is
 * `logOpen` over them; the cards in a `spotlight` are shown apart from where they lie.
 * Hovering changes none of it (CardLayer raises a hovered hand card). */
export function layout(
  view: PlayerView,
  hold: HandHold,
  logOpen: boolean,
  spotlight: Spotlight,
  order: ReadonlyMap<number, number>,
  borrowed: ReadonlySet<number>,
  piles: readonly ZonePile[],
  handling: readonly number[],
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
  const groups = new Map(
    (["opponent", "you"] as const).map((side) => {
      const controlled = permanents.filter((o) => o.controller === side);
      return [
        side,
        sideGroups(
          controlled.filter((o) => !behindHost.has(o.slot)),
          beneath,
          attached,
          named,
          controlled.filter((o) => o.types.includes("land")).length >=
            LANDS_PILED,
        ),
      ];
    }),
  );
  const zoom = battlefieldZoom([...groups], beneath, attached, logOpen);
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
        view.stack.length,
      ),
      yours.filter(backRow).map(footprint),
    ),
  );
  // A targeted buried card rises toward the middle so its arrow meets the right face.
  const aimed = new Set(
    view.stack
      .at(-1)
      ?.targets.flatMap((t) => (t.kind === "player" ? [] : [t.slot])) ?? [],
  );
  for (const { side, zone, cards } of piles) {
    out.push(...pileCards(side, zone, cards, borrowed, aimed));
  }
  const handled = handling.flatMap((slot) => {
    const object = bySlot.get(slot);
    return object && object.zone === view.handling?.zone && !borrowed.has(slot)
      ? [object]
      : [];
  });
  const lifted = new Set([
    ...borrowed,
    ...handled.map((o) => o.slot),
    ...(spotlight.scryReturn?.bottom ?? []),
  ]);
  for (const side of ["opponent", "you"] as const) {
    const cards = view.players[side].library.flatMap((slot) => {
      const object = bySlot.get(slot);
      return object && !inHand.has(slot) ? [object] : [];
    });
    out.push(
      ...pileCards(side, "library", cards, lifted, aimed, {
        // A disclosed card stays face down when it becomes a deeper layer. Do not
        // expose a different top just because a packet is temporarily off the pile.
        top: view.players[side].library.at(-1),
        shuffle: spotlight.shuffle?.has(side),
        scryReturn:
          spotlight.scryReturn?.side === side
            ? spotlight.scryReturn
            : undefined,
      }),
    );
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
  out.push(...handlingPlacements(view, handled));
  return new Map(out.map((p) => [p.slot, p]));
}

/** Cards in a showcase are drawn over the permanents and the stack. */
const SHOWCASE_Z = 800;

/** The stack's items are drawn over everything else on the table, the top item in front. */
export const STACK_Z = 400;
