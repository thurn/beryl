import type { TableAction, TableFlowOption } from "../generated/protocol";
import type { MenuCommand, MenuContext } from "./menuRows";

export function cardFlows({ target, view, cards }: MenuContext) {
  return "name" in target &&
    target.object &&
    target.action &&
    cards.length === 1 &&
    view.table?.assists?.libraryFlows &&
    view.players.you.library.length
    ? (view.table.cardCommands?.[target.object.slot]?.flows ?? [])
    : [];
}

export const flowCondition = (kind: TableFlowOption["kind"]) => `card_${kind}`;

export function flowLabel({ kind, count }: TableFlowOption): string {
  switch (kind) {
    case "scry":
      return `Scry ${count}`;
    case "surveil":
      return `Surveil ${count}`;
    case "look_at_top":
      return `Look at top ${count} ${count === 1 ? "card" : "cards"}`;
  }
}

export function usesLookSort(context: MenuContext): boolean {
  const { target, view } = context;
  return (
    target.kind === "library" &&
    target.side === "you" &&
    view.table?.assists?.libraryFlows === true
  );
}

export function flowAction(
  entry: MenuCommand,
  context: MenuContext,
  number: number | undefined,
): TableAction | undefined {
  const { target, view } = context;
  const seat = view.table?.seat ?? 0;
  if (entry.flow && "name" in target && target.action) {
    const { kind, count } = entry.flow;
    const { reference, zone } = target.action;
    return {
      flow: { seat, flow: kind, count, source: { id: reference, in: zone } },
    };
  }
  if (entry.id === "library.look_top" && usesLookSort(context) && number)
    return { flow: { seat, flow: "look_at_top", count: number } };
}
