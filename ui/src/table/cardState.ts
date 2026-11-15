import type {
  CardTokenLink,
  ObjectView,
  PlayerView,
  TableCard,
} from "../generated/protocol";
import { counterName } from "../game/log.ts";
import { clamp } from "./ask.ts";

export const counterSuggestions = [
  "+1/+1",
  "−1/−1",
  "loyalty",
  "charge",
  "lore",
  "stun",
];

export const normalizedCounter = (name: string) =>
  name.trim().replaceAll("−", "-");

export function counterNames(view: PlayerView, slot?: number) {
  return (
    (slot === undefined
      ? undefined
      : view.table?.cardCommands?.[slot]?.counterNames) ??
    (["charge", "time", "counter"] as const)
  );
}

export function counterValue(object: ObjectView | undefined, name: string) {
  return (
    object?.counters.find(
      ({ kind }) => normalizedCounter(counterName(kind)) === name,
    )?.count ??
    object?.namedCounters?.find((counter) => counter.name === name)?.count ??
    0
  );
}

export const MIN_INT = -2147483648;
export const MAX_INT = 2147483647;
export const statValue = (value: number) => clamp(value, MIN_INT, MAX_INT);

export function baseStats(view: PlayerView, slot?: number) {
  return (
    (slot === undefined
      ? undefined
      : view.table?.cardCommands?.[slot]?.baseStats) ?? {
      power: 0,
      toughness: 0,
    }
  );
}

export interface TokenChoice {
  card: TableCard;
  note: string | null;
}

export const storeToken = ({
  name,
  scryfallId,
}: CardTokenLink): TokenChoice => ({
  card: { token: { name, scryfall_id: scryfallId } },
  note: null,
});
