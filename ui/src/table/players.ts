import type { PlayerView } from "../ipc";
import type { TableClient, TableTarget } from "./client";
import type { TableCommands } from "./TableMenu";

const manaColors = {
  W: "white",
  U: "blue",
  B: "black",
  R: "red",
  G: "green",
  C: "colorless",
} as const;

export function manaColor(name: string) {
  if (Object.hasOwn(manaColors, name))
    return manaColors[name as keyof typeof manaColors];
}

export function playerCounterValue(
  view: PlayerView,
  side: "you" | "opponent",
  name: string,
) {
  const player = view.players[side];
  const color = manaColor(name);
  return color
    ? player.mana[color]
    : (player.namedCounters?.find((counter) => counter.name === name)?.count ??
        0);
}

export function registerPlayers(client: TableClient, commands: TableCommands) {
  const run = (id: string, target: TableTarget, element?: HTMLElement) => {
    void commands.runAt(id, target, element).catch(commands.reportError);
  };
  const adjust = (
    target: Extract<TableTarget, { kind: "counter" }>,
    change: "add" | "remove",
    element?: HTMLElement,
  ) => {
    const kind = manaColor(target.counter) ? "mana" : "counter";
    run(`player.${kind}_${change}`, target, element);
  };

  client.onClick("life", (target, _, element) =>
    run("life.set", target, element),
  );
  client.onClick("counter", (target, _, element) =>
    adjust(target, "add", element),
  );
  client.onSecondary((target, _, element) => {
    if (target?.kind !== "counter") return;
    adjust(target, "remove", element);
    return true;
  });
}
