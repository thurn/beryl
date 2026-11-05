// What the human can do on the board at their open decision: which cards they can act on and
// how, which they can pick, and what the prompt asks. Pure functions of the decision.
import type {
  Candidate,
  CandidateIndex,
  DecisionView,
  Kind,
  ObjectView,
  Payment,
  PlayerView,
  Side,
} from "../generated/protocol";

/** One thing a card offers: the candidate's index and its label; for a cast or an activation,
 * the ways it can be paid; for a blocker, the attackers it may block. */
export interface Act {
  index: CandidateIndex;
  label: string;
  option: Candidate;
  payments: Payment[];
  blocks: number[];
}

export interface Interactions {
  /** Each card's candidates, by slot. */
  bySlot: Map<number, Act[]>;
  /** The players the decision can name. */
  players: Map<Side, CandidateIndex>;
  /** Passing priority, at a priority decision. */
  pass?: CandidateIndex;
  /** Finishing a declaration or a division (attackers, blockers, combat damage). */
  done?: CandidateIndex;
  /** At a decision among things (targets, cards): a card with one candidate is picked by
   * clicking it, so it glows cyan; elsewhere clicking a card lists its candidates. */
  picking: boolean;
  /** The card the choice is for, glowing gold: the spell or ability on top of the stack. */
  source?: number;
}

/** A pick a later decision should get: one of its kind, the candidate `wants` names or,
 * failing that, one `alike` names (a card the engine offers in place of an interchangeable
 * one, architecture §7.2); `picked` hears which it was. An overlay says a whole scry; the
 * board, a whole combat declaration. A pick that `waits` stays planned while the player
 * answers other decisions before it (a cast's payment, after the targets they pick on the
 * board), until priority returns. An `optional` pick is passed over, as `skipped` hears, when
 * the engine does not ask its decision, having made the pick itself (a blocker's attacker
 * when it may block only one). */
export interface Intent {
  kind: Kind;
  wants(option: Candidate): boolean;
  alike?(option: Candidate): boolean;
  picked?(option: Candidate): void;
  skipped?(): void;
  waits?: boolean;
  optional?: boolean;
}

/** The candidate of `decision` that `intent` names, if it is of the intent's kind. */
export function intended(
  decision: DecisionView,
  intent: Intent,
): CandidateIndex | undefined {
  if (decision.kind !== intent.kind) return undefined;
  const options = decision.candidates.map((c) => c.option);
  let index = options.findIndex((o) => intent.wants(o));
  if (index < 0 && intent.alike) {
    index = options.findIndex((o) => intent.alike!(o));
  }
  return index < 0 ? undefined : index;
}

/** What `plan` picks at `decision`: its next intent, past optional ones the engine did not
 * ask, with the candidate it names, if any (which it is told of), and the intents after it. */
export function follow(
  decision: DecisionView,
  plan: Intent[],
): { intent?: Intent; index?: CandidateIndex; rest: Intent[] } {
  let i = 0;
  while (plan[i]?.optional && plan[i]!.kind !== decision.kind) {
    plan[i]!.skipped?.();
    i += 1;
  }
  const intent = plan[i];
  const index = intent && intended(decision, intent);
  if (intent && index !== undefined) {
    intent.picked?.(decision.candidates[index]!.option);
  }
  return { intent, index, rest: plan.slice(i + 1) };
}

/** Whether two objects are interchangeable, as the engine offers one candidate for both
 * (architecture §7.2): the same card, or token, in the same zone and state. */
export function twins(a: ObjectView, b: ObjectView): boolean {
  return (
    a.slot !== b.slot &&
    a.name !== null &&
    a.name === b.name &&
    a.zone === b.zone &&
    a.controller === b.controller &&
    a.token === b.token &&
    a.tapped === b.tapped &&
    a.summoningSick === b.summoningSick &&
    a.damage === b.damage &&
    a.attachedTo === b.attachedTo &&
    JSON.stringify([a.pt, a.counters, a.combat]) ===
      JSON.stringify([b.pt, b.counters, b.combat])
  );
}

/** Whether two candidates are the same answer. */
export const optionsMatch = (a: Candidate, b: Candidate) =>
  JSON.stringify(a) === JSON.stringify(b);

/** The card a candidate is about, if any. */
export function slotOf(option: Candidate): number | undefined {
  switch (option.kind) {
    case "object":
    case "playLand":
    case "attack":
    case "block":
    case "lethal":
    case "cast":
    case "activate":
    case "mana":
      return option.slot;
    case "defender":
      return option.planeswalker ?? undefined;
    default:
      return undefined;
  }
}

export function interactions(view: PlayerView): Interactions {
  const status = view.status;
  const bySlot = new Map<number, Act[]>();
  const players = new Map<Side, CandidateIndex>();
  if (status.kind !== "deciding") {
    return { bySlot, players, picking: false };
  }
  const decision = status.decision;
  let pass: CandidateIndex | undefined;
  let done: CandidateIndex | undefined;
  decision.candidates.forEach(({ label, option, payments, blocks }, index) => {
    const slot = slotOf(option);
    if (slot !== undefined) {
      const act = { index, label, option, payments, blocks };
      bySlot.set(slot, [...(bySlot.get(slot) ?? []), act]);
    } else if (option.kind === "player") {
      players.set(option.player, index);
    } else if (option.kind === "defender") {
      // Attacking no planeswalker is attacking the defending player (CR 508.1b).
      players.set("opponent", index);
    } else if (option.kind === "pass") {
      pass = index;
    } else if (option.kind === "done") {
      done = index;
    }
  });
  // Interchangeable cards collapse to one candidate (architecture §7.2): of two basic lands
  // of a name in your hand the engine offers one, of two untapped Insect tokens one attacker.
  // Either card offers it.
  const offered = view.objects.filter((o) => bySlot.has(o.slot));
  for (const o of view.objects) {
    if (bySlot.has(o.slot) || (o.zone !== "hand" && o.zone !== "battlefield")) {
      continue;
    }
    const twin = offered.find((t) => twins(o, t));
    if (twin) bySlot.set(o.slot, bySlot.get(twin.slot)!);
  }
  return {
    bySlot,
    players,
    pass,
    done,
    picking: decision.kind !== "priority",
    source: decision.source ?? undefined,
  };
}

/** The combat declarations: made on the board, finished with the turn button. */
export const isCombat = (kind: Kind) =>
  kind === "attacker" ||
  kind === "attackTarget" ||
  kind === "blocker" ||
  kind === "blockTarget" ||
  kind === "damageDivision";

/** The decisions before the first turn, which the opening screen asks over the table. */
export const isOpening = (kind: Kind) =>
  kind === "playOrDraw" || kind === "keepOrMulligan" || kind === "putOnBottom";

/** Whether a hand card's candidates include playing or casting it: what dropping it on the
 * battlefield does. */
export const playActs = (acts: Act[]) =>
  acts.filter((a) => a.option.kind === "cast" || a.option.kind === "playLand");

const PROMPTS: Partial<Record<Kind, string>> = {
  playOrDraw: "Play or draw?",
  keepOrMulligan: "Keep or mulligan?",
  putOnBottom: "Put a card on the bottom of your library",
  discard: "Discard a card",
  legendRule: "Choose the legend to keep",
  triggerOrder: "Put a trigger on the stack",
  payLife: "Pay life?",
  attacker: "Declare attackers",
  attackTarget: "Choose what to attack",
  blocker: "Declare blockers",
  blockTarget: "Choose what to block",
  damageDivision: "Assign combat damage",
  x: "Choose X",
  targetCount: "How many targets?",
  target: "Choose a target",
  paymentPlan: "Choose how to pay",
  manaPayment: "Pay the mana",
  castOffer: "Cast it?",
  yesNo: "Choose",
  chooseObject: "Choose a card",
  crew: "Choose a creature to crew with",
  chooseUpTo: "Choose cards",
  waterbend: "Tap for waterbend?",
};

/** The prompt's title: what the decision asks, and for whom when there is a source. */
export function prompt(decision: DecisionView, source?: string): string {
  const ask = PROMPTS[decision.kind] ?? "Choose";
  return source && decision.kind === "target" ? `${ask} for ${source}` : ask;
}
