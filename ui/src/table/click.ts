import type { TableAction, TableObjectAction } from "../ipc";
import { sameZone, type CardTarget, type TableClient } from "./client";
import type { TableCommands } from "./TableMenu";

export function registerClicks(client: TableClient, commands: TableCommands) {
  const report = commands.reportError;
  const send = (
    action: TableAction,
    source?: Parameters<TableClient["send"]>[1],
  ) => void client.send(action, source).catch(report);

  const clicked = (target: CardTarget) => {
    const cards = client.selection.has(target.name)
      ? [...client.selection].map((name) => client.card(name, true))
      : [target];
    return cards.flatMap((card) =>
      card?.action ? [{ action: card.action, object: card.object }] : [],
    );
  };

  for (const kind of ["permanent", "their_permanent"] as const) {
    client.onClick(kind, (target) => {
      if (!target.action || !target.object) return;
      const permanents = clicked(target).filter(
        (card) => card.action.zone === "battlefield",
      );
      send(
        {
          tap: {
            tapped: permanents.some(({ object }) => object?.tapped === false),
            objects: permanents.map(({ action }) => action.reference),
            in: "battlefield",
          },
        },
        target.name,
      );
    });
  }

  for (const kind of ["hand_card", "viewer_card", "stack_entry"] as const) {
    client.onClick(kind, (target, { shift }, element) => {
      if (!target.action) return;
      const resolving = kind === "stack_entry";
      const faceDown = !resolving && shift;
      const groups: Array<{
        zone: TableObjectAction["zone"];
        objects: TableObjectAction["reference"][];
      }> = [];
      for (const { action } of clicked(target)) {
        if (
          resolving ? action.zone !== "stack" : typeof action.zone !== "object"
        )
          continue;
        const group = groups.find(({ zone }) => sameZone(zone, action.zone));
        if (group) group.objects.push(action.reference);
        else groups.push({ zone: action.zone, objects: [action.reference] });
      }
      const moves: TableAction[] = groups.map(({ zone, objects }) => ({
        move: {
          to: resolving ? "resolve" : faceDown ? "battlefield" : "play",
          ...(faceDown ? { face: "down" } : {}),
          objects,
          in: zone,
        },
      }));
      const [first] = moves;
      if (!first) return;
      send(
        moves.length === 1 ? first : { batch: { actions: moves } },
        element ?? target.name,
      );
    });
  }

  client.onClick("library", (target, _keys, element) => {
    void commands.runAt("library.draw", target, element).catch(report);
  });
  for (const kind of ["graveyard", "exile"] as const) {
    client.onClick(kind, ({ open }) => open?.());
  }
}
