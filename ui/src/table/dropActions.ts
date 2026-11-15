import { battlefieldRow } from "../game/battlefield";
import type { Point } from "../stage/layout";
import type {
  ObjectView,
  Side,
  TableAction,
  TableObjectAction,
  TablePosition,
  TableSelection,
  TableObjectRef,
  TableZone,
} from "../generated/protocol";
import { sameZone, type TableClient } from "./client";
import type { Destination } from "./dropZones";
import { canMoveInto, isHiddenZone } from "./zoneAccess";

export interface DragCard {
  object: ObjectView;
  action: TableObjectAction;
}

const seat = (client: TableClient, side: Side) => {
  const local = client.view?.table?.seat ?? 0;
  return side === "you" ? local : 1 - local;
};

export function available(
  client: TableClient,
  to: Destination,
  cards: readonly DragCard[],
) {
  if (to.side === "opponent" && client.view?.table?.seats !== 2) {
    return to.kind === "stack";
  }
  if (!isHiddenZone(to.kind)) return true;
  const view = client.view;
  const destination = { zone: to.kind, side: to.side };
  return (
    !!view &&
    cards.every(({ object }) => canMoveInto(view, destination, object))
  );
}

function zoneOf(client: TableClient, { kind, side }: Destination): TableZone {
  switch (kind) {
    case "stack":
      return kind;
    case "hand":
    case "library":
    case "graveyard":
    case "exile":
    case "sideboard":
      return { [kind]: seat(client, side) } as TableZone;
    default:
      return "battlefield";
  }
}

function collect(cards: readonly DragCard[]) {
  const groups: TableSelection<TableObjectRef>[] = [];
  for (const { action } of cards) {
    const last = groups.at(-1);
    if (last && sameZone(last.in, action.zone))
      last.objects.push(action.reference);
    else groups.push({ in: action.zone, objects: [action.reference] });
  }
  return groups;
}

function placement(
  client: TableClient,
  cards: readonly DragCard[],
  to: Destination,
  at: Point,
  hidden: boolean,
) {
  const actions: TableAction[] = [];
  const host = to.card?.action;
  const commands = client.view?.table?.cardCommands;
  const attach =
    !hidden && host
      ? cards.filter(({ object }) => commands?.[object.slot]?.autoAttach)
      : [];
  const remaining = cards.filter((card) => !attach.includes(card));
  const entering = remaining.filter(
    ({ action }) => action.zone !== "battlefield",
  );
  const arrivals: TableAction[] = collect(entering).map((selected) => ({
    move: {
      ...selected,
      to: "battlefield",
      ...(hidden ? { face: "down" } : {}),
    },
  }));
  if (host) {
    for (const selected of collect(attach)) {
      arrivals.push({
        attach: { ...selected, host: { id: host.reference, in: host.zone } },
      });
    }
  }

  const permanents = remaining.filter(
    ({ action }) => action.zone === "battlefield",
  );
  const moving = (subset: readonly DragCard[]) => ({
    objects: subset.map(({ action }) => action.reference),
    in: "battlefield" as const,
  });
  const shift = hidden
    ? permanents.filter(({ object }) => object.face !== "down")
    : [];
  if (shift.length) actions.push({ face: { ...moving(shift), face: "down" } });

  const unattach = permanents.filter(
    ({ object }) =>
      object.attachedTo !== null && object.attachedTo !== to.card?.object?.slot,
  );
  if (unattach.length) actions.push({ unattach: moving(unattach) });

  const transfer = permanents.filter(
    ({ object }) => object.controller !== to.side,
  );
  if (transfer.length)
    actions.push({ give: { ...moving(transfer), to: seat(client, to.side) } });

  const local = permanents.filter(
    ({ object }) => object.controller === to.side && object.attachedTo === null,
  );
  const row = to.kind;
  if (row !== "creatures" && row !== "lands" && row !== "others")
    return [...actions, ...arrivals];
  const pin = local.filter(({ object }) => battlefieldRow(object) !== row);
  if (pin.length) actions.push({ pin: { ...moving(pin), row } });

  const reorder = local.filter(({ object }) => battlefieldRow(object) === row);
  const order =
    client.view?.battlefield.filter(
      (slot) => !reorder.some(({ object }) => object.slot === slot),
    ) ?? [];
  if (reorder.length) {
    const neighbors = order
      .flatMap((slot, index) => {
        const object = client.card(slot)?.object;
        const node = client.surface?.node(slot);
        if (
          !object ||
          !node ||
          object.attachedTo !== null ||
          object.controller !== to.side ||
          battlefieldRow(object) !== row
        )
          return [];
        const { left, right } = client.surface!.bounds(node);
        return [{ index, x: (left + right) / 2 }];
      })
      .sort((a, b) => a.x - b.x);
    const before = neighbors.find(({ x }) => at.x < x);
    const last = neighbors.at(-1);
    if (before || last) {
      const nth = before?.index ?? last!.index + 1;
      actions.push({ reorder: { ...moving(reorder), position: { nth } } });
    }
  }
  // Reorder uses the current battlefield indices. Add cards from other zones afterward.
  return [...actions, ...arrivals];
}

export function dropAction(
  client: TableClient,
  cards: readonly DragCard[],
  to: Destination,
  at: Point,
  hidden: boolean,
): TableAction | undefined {
  const zone = zoneOf(client, to);
  if (zone === "battlefield")
    return batch(placement(client, cards, to, at, hidden));
  const position: TablePosition =
    to.position ?? (to.kind === "hand" ? "bottom" : "top");
  const groups = collect(cards);
  // The chosen order is top first. Each move inserts its group at the same end.
  if (position === "top") groups.reverse();
  return batch(
    groups.map((selection) => ({
      move: {
        ...selection,
        to: zone,
        position,
        ...(hidden ? { face: "down" } : {}),
      },
    })),
  );
}

function batch(actions: TableAction[]): TableAction | undefined {
  return actions.length > 1 ? { batch: { actions } } : actions[0];
}
