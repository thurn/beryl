import type { Side, TableAction } from "../ipc";
import type { TableClient } from "./client";

const seat = (side: Side) => (side === "you" ? 0 : 1);

export function registerClicks(
  client: TableClient,
  report: (error: unknown) => void,
) {
  const send = (
    action: TableAction,
    source?: Parameters<TableClient["send"]>[1],
  ) => void client.send(action, source).catch(report);

  for (const kind of ["permanent", "their_permanent"] as const) {
    client.onClick(kind, ({ action, object, name }) => {
      if (!action || !object) return;
      send(
        {
          tap: {
            tapped: !object.tapped,
            objects: [action.reference],
            in: action.zone,
          },
        },
        name,
      );
    });
  }

  for (const kind of ["hand_card", "viewer_card", "stack_entry"] as const) {
    client.onClick(kind, ({ action, name }, { shift }, element) => {
      if (!action) return;
      const faceDown = kind !== "stack_entry" && shift;
      send(
        {
          move: {
            to:
              kind === "stack_entry"
                ? "resolve"
                : faceDown
                  ? "battlefield"
                  : "play",
            ...(faceDown ? { face: "down" } : {}),
            objects: [action.reference],
            in: action.zone,
          },
        },
        element ?? name,
      );
    });
  }

  client.onClick("library", ({ side }, _keys, element) =>
    send({ draw: { seat: seat(side), count: 1 } }, element),
  );
  for (const kind of ["graveyard", "exile"] as const) {
    client.onClick(kind, ({ open }) => open?.());
  }
  client.onClick("counter", ({ side, counter }, _keys, element) =>
    send(
      { player_counter: { seat: seat(side), name: counter, add: 1 } },
      element,
    ),
  );
  client.onSecondary((target, _at, element) => {
    if (target?.kind !== "counter") return;
    send(
      {
        player_counter: {
          seat: seat(target.side),
          name: target.counter,
          add: -1,
        },
      },
      element,
    );
    return true;
  });
}
