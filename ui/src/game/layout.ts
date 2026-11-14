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
import { FULL_WIDTH, footprint } from "./placement";
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
import { fromMiddle, fromRight } from "./perspective";
import { stackPlace } from "./stackLayout";
import type { ZonePile } from "./zoneViews";
import {
  attachments,
  emblemObjects,
  held,
  playableElsewhere,
} from "./zoneViews";

/** How deep in a pile a card is drawn: none deeper shows. */
const HIDDEN_DEPTH = 64;
/** How far a targeted card under a pile's top rises out of it, in its own heights. */
const PILE_RISE = 0.6;

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
  const pile = library ? pileLook(side, PILES[side].library) : undefined;
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
  library?: { top?: number },
): Placement[] {
  const look = pileLook(side, PILES[side][zone]);
  const remaining = cards.filter((o) => !borrowed.has(o.slot)).length;
  let depthLeft = remaining;
  return cards.map((o, i) => {
    const away = borrowed.has(o.slot);
    const depth = away ? cards.length - 1 - i : --depthLeft;
    const at = onPile(look, side, depth);
    const raised = !library && aimed.has(o.slot) && depth > 0;
    const lift = raised
      ? ((side === "you" ? -1 : 1) * look.width * PILE_RISE) / CARD_ASPECT
      : 0;
    const hidden = depth >= HIDDEN_DEPTH && !raised;
    return {
      ...at,
      slot: o.slot,
      zone: o.zone,
      look: hidden ? "hidden" : "pile",
      width: FULL_WIDTH,
      span: FULL_WIDTH,
      opacity: hidden ? 0 : 1,
      z: raised ? 92 : 80 - Math.min(depth, 79),
      rim: depth > 1 && !raised,
      stacked: !away && depth < Math.min(remaining - 1, HIDDEN_DEPTH - 1),
      back: !!library && ((!away && o.slot !== library.top) || !o.name),
      librarySide: library ? side : undefined,
      lendingFace: !!library && away && !!o.name,
      x: at.x,
      y: at.y + lift,
    };
  });
}

/** Every object's placement. The hand is tucked under a screen over the table or raised for a
 * choice of its cards (`hold`); the permanents' rows keep clear of the game log while it is
 * `logOpen` over them. Hovering changes none of it. */
export function layout(
  view: PlayerView,
  hold: HandHold,
  logOpen: boolean,
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

  out.push(...heldPlacements(owned("opponent", "hand")));
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
        [...owned("you", "hand"), ...elsewhere],
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
  const lifted = new Set([...borrowed, ...handled.map((o) => o.slot)]);
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
  out.push(...handlingPlacements(view, handled));
  return new Map(out.map((p) => [p.slot, p]));
}

/** Cards lifted to be looked at are drawn over the permanents and the stack. */
const SHOWCASE_Z = 800;

/** The stack's items are drawn over everything else on the table, the top item in front. */
export const STACK_Z = 400;
