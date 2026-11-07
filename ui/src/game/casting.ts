// Casting a card in one step. The engine asks a cast one pick at a time: which cast (with
// kicker or another optional or alternative cost, each its own candidate, architecture §7.2),
// then X, then a payment plan where plans differ in what they consume. The cast step takes the
// cast, its X and its payment together, lights what the payment taps, and plans the picks that
// say them, which the game screen sends as each decision arrives. Pure functions of the acts.
import type { Payment } from "../generated/protocol";
import type { Act, Intent } from "./decisions";

/** What a cast step has settled: the cast, its X if it asks one, and its payment plan if the
 * player chose one over the engine's default. */
export interface CastChoice {
  act: Act;
  x?: number;
  plan?: number;
}

/** A card's casts among its acts. */
export const castActs = (acts: Act[]) =>
  acts.filter((a) => a.option.kind === "cast");

/** The values a cast's X may take, ascending; none when it asks no X
 * ([CR 601.2b](https://yawgatog.com/resources/magic-rules/#R6012b): "the player announces
 * the value of that variable"). */
export function xValues(act: Act): number[] {
  const xs = act.payments.flatMap((p) => (p.x === null ? [] : [p.x]));
  return [...new Set(xs)].sort((a, b) => a - b);
}

/** Whether casting from `casts` asks more than the drop: which cast, or X. */
export const asksCastStep = (casts: Act[]) =>
  casts.length > 1 || casts.some((a) => xValues(a).length > 0);

/** A cast with its least X and the engine's default payment. */
export const firstChoice = (act: Act): CastChoice => ({
  act,
  x: xValues(act)[0],
});

/** The ways a choice's cast can be paid at its X: one, or several plans to choose from. */
export const plansOf = (choice: CastChoice): Payment[] =>
  choice.act.payments.filter((p) => (p.x ?? undefined) === choice.x);

/** The payment a choice makes: its plan, or else the engine's default, the first. */
export function paymentOf(choice: CastChoice): Payment | undefined {
  const plans = plansOf(choice);
  return plans.find((p) => p.plan === (choice.plan ?? null)) ?? plans[0];
}

/** The permanents a choice's payment taps. */
export const tapsOf = (choice: CastChoice): number[] =>
  paymentOf(choice)?.taps ?? [];

/** The picks after the cast's own that say its X and its payment plan. */
export function castPlan(choice: CastChoice): Intent[] {
  const plan: Intent[] = [];
  const { x } = choice;
  if (x !== undefined) {
    plan.push({
      kind: "x",
      wants: (o) => o.kind === "number" && o.n === x,
    });
  }
  const pays = paymentOf(choice)?.plan;
  if (pays !== null && pays !== undefined) {
    // Targets come between X and the payment (CR 601.2c to 601.2g): the plan waits for them.
    plan.push({
      kind: "paymentPlan",
      wants: (o) => o.kind === "plan" && o.index === pays,
      waits: true,
    });
  }
  return plan;
}

/** A cast's name in the cast step: "Cast" for the card as printed, or what the cast adds, as
 * its label says after the card's name ("Cast <card> with kicker {X}" is "Kicker {X}"). */
export function castName(act: Act, card: string): string {
  const rest = act.label.replace(/^Cast /, "");
  if (rest === card) return "Cast";
  const extra = rest.startsWith(`${card} with `)
    ? rest.slice(card.length + " with ".length)
    : rest;
  return extra.charAt(0).toUpperCase() + extra.slice(1);
}

/** Names of permanents as a list that counts repeats: two lands of one name and an artifact
 * read "2 <land>s, <artifact>". For a payment plan the engine asks about outside the cast
 * step. */
export function tapList(names: string[]): string {
  const counts = new Map<string, number>();
  for (const n of names) counts.set(n, (counts.get(n) ?? 0) + 1);
  return [...counts]
    .map(([name, n]) => (n === 1 ? name : `${n} ${plural(name)}`))
    .join(", ");
}

const plural = (name: string) =>
  /(s|x|ch|sh)$/.test(name) ? `${name}es` : `${name}s`;
