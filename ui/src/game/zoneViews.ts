// What the table draws for a player's zones apart from the battlefield: emblems,
// attachments, linked cards and the piles a view shows.
import type { ObjectView, PlayerView, Side } from "../generated/protocol";
import type { PileLook } from "./pileLook";
import { PILES, pileLook } from "./pileLook";
import { inOrder } from "./placement";
import { isLand } from "./battlefield";

/** The slot each of a side's emblems is drawn under: one no object has, counting down from -1,
 * the two sides' taking turns, so that each emblem keeps its own as more are made. */
export const emblemSlot = (side: Side, index: number) =>
  -1 - (2 * index + (side === "you" ? 1 : 0));

/** Whether a slot is an emblem's (`emblemSlot`), which nothing can be done with. */
export const isEmblem = (slot: number) => slot < 0;

/** Each player's emblems ([CR 114.1](https://yawgatog.com/resources/magic-rules/#R1141): "An
 * emblem is a marker used to represent an object that has one or more abilities, but usually no other
 * characteristics") as objects to draw: each a card, under its `emblemSlot`, in its player's
 * back row after their other permanents, where pointing at it shows it whole. The table draws
 * the command zone there ([CR 114.1](https://yawgatog.com/resources/magic-rules/#R1141): "Some
 * effects put emblems into the command zone"). */
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
  count?: number;
  look: PileLook;
}

/** Only identities already disclosed by the session may be browsed in a library. */
export function knownLibraries(view: PlayerView): ZonePile[] {
  const objects = new Map(view.objects.map((o) => [o.slot, o]));
  return (["you", "opponent"] as const).map((side) => ({
    side,
    zone: "library",
    count: view.players[side].library.length,
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
export function zonePiles(
  view: PlayerView,
  exileOrder: ReadonlyMap<number, number>,
): ZonePile[] {
  const bySlot = new Map(view.objects.map((o) => [o.slot, o]));
  const holding = new Set(view.battlefield);
  const inHand = new Set(playableElsewhere(view));
  return (["opponent", "you"] as const).flatMap((side) => {
    const graveyard = view.players[side].graveyard.flatMap((slot) =>
      inHand.has(slot) ? [] : (bySlot.get(slot) ?? []),
    );
    const exile = inOrder(
      view.objects.filter(
        (o) =>
          o.owner === side &&
          o.zone === "exile" &&
          !inHand.has(o.slot) &&
          held(o, holding) === undefined,
      ),
      exileOrder,
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
