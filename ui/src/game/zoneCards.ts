import type {
  ManualPiles,
  ObjectView,
  PlayerView,
  Side,
} from "../generated/protocol";

/** A hand, exile pile or sideboard in the order the table uses. Engine views
 * supply their hand and exile in encounter order. */
export function zoneCards(
  view: PlayerView,
  side: Side,
  zone: keyof ManualPiles,
): ObjectView[] {
  const cards = view.objects.filter(
    (o) => (o.zoneSide ?? o.owner) === side && o.zone === zone,
  );
  const order = view.table?.orders[side][zone];
  if (!order) return cards;
  const bySlot = new Map(cards.map((o) => [o.slot, o]));
  return order.flatMap((slot) => bySlot.get(slot) ?? []);
}
