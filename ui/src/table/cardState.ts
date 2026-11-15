import type {
  CardTokenLink,
  CardType,
  ObjectView,
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

const unnamed = new Set([
  "a",
  "an",
  "the",
  "each",
  "that",
  "those",
  "many",
  "one",
  "two",
  "three",
  "four",
  "five",
  "six",
  "seven",
  "eight",
  "nine",
  "ten",
]);

export const normalizedCounter = (name: string) =>
  name.trim().replaceAll("−", "-");

/** The three quick slots follow the card's types and the order of its printed counter names. */
export function counterSlots(
  types: readonly CardType[] = [],
  text = "",
): [string, string, string] {
  if (types.includes("creature")) return ["+1/+1", "-1/-1", "counter"];
  const names = [
    ...normalizedCounter(text).matchAll(
      /(?<![\w+-])([+-]\d+\/[+-]\d+|[a-z]+) counters?\b/gi,
    ),
  ]
    .map((match) => match[1]!.toLowerCase())
    .filter((name) => !unnamed.has(name));
  const [first, second] = new Set([
    ...(types.includes("planeswalker") ? ["loyalty"] : []),
    ...names,
    "charge",
    "time",
  ]);
  return [first!, second!, "counter"];
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

export function baseStats(object?: ObjectView) {
  if (!object?.pt) return { power: 0, toughness: 0 };
  const bonus = counterValue(object, "+1/+1") - counterValue(object, "-1/-1");
  return {
    power: statValue(object.pt.power - bonus),
    toughness: statValue(object.pt.toughness - bonus),
  };
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
