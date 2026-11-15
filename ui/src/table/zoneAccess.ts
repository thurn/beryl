import type {
  ObjectView,
  PlayerView,
  Side,
  ZoneName,
  ZoneRef,
} from "../generated/protocol";

type HiddenZone = "library" | "hand" | "sideboard";

export function isHiddenZone(zone: string): zone is HiddenZone {
  return zone === "library" || zone === "hand" || zone === "sideboard";
}

export function canUseZone(view: PlayerView, side: Side, zone: ZoneName) {
  const table = view.table;
  if (!table || (side === "opponent" && table.seats !== 2)) return false;
  return (
    side === "you" ||
    !isHiddenZone(zone) ||
    table.acting.some((grant) => grant.side === side && grant.zone === zone)
  );
}

export function canMoveInto(
  view: PlayerView,
  { side, zone }: ZoneRef,
  card: Pick<ObjectView, "owner" | "zone" | "zoneSide">,
) {
  return (
    canUseZone(view, side, zone) ||
    (view.table?.seats === 2 &&
      (card.owner === side || (card.zone === zone && card.zoneSide === side)))
  );
}
