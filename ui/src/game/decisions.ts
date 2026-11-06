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
  StackItem,
} from "../generated/protocol";

/** One thing a card or a player offers: the candidate's index and its label; for a cast or an
 * activation, the ways it can be paid; for a blocker, the attackers it may block; for an
 * activation of the card's own ability, which of its activated abilities it is; and the picks
 * it plans after its own (the target itself, for a count of targets taken with it). */
export interface Act {
  index: CandidateIndex;
  label: string;
  option: Candidate;
  payments: Payment[];
  blocks: number[];
  nth: number | null;
  then?: Intent[];
}

export interface Interactions {
  /** Each card's candidates, by slot. */
  bySlot: Map<number, Act[]>;
  /** The players the decision can name. */
  players: Map<Side, Act>;
  /** Choosing no target, at a count of up to one target taken on the board with its target. */
  skip?: CandidateIndex;
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

/** The view with what is resolving back on top of its stack. The engine takes a spell or
 * ability off the stack as it begins to resolve, and a question its resolution asks names it
 * as `resolving`; it is still on the stack until it finishes (CR 608.2n: "As the final part of
 * an instant or sorcery spell's resolution, the spell is put into its owner's graveyard"), and
 * the player answering sees what asks, as a spell asking whether they pay its tax. */
export function withResolving(view: PlayerView): PlayerView {
  if (view.status.kind !== "deciding") return view;
  const resolving = view.status.decision.resolving;
  if (!resolving) return view;
  const same = (item: StackItem) =>
    resolving.kind === "spell"
      ? item.kind === "spell" && item.slot === resolving.slot
      : JSON.stringify(item) === JSON.stringify(resolving);
  if (view.stack.some(same)) return view;
  return { ...view, stack: [...view.stack, resolving] };
}

/** The objects a spell or ability on the stack names, a target or an ability's source. The
 * engine never offers a target for its twin (architecture §7.2), and the table shows each
 * apart, so its glow and arrow stay in view. */
export function referenced(view: PlayerView): Set<number> {
  const out = new Set<number>();
  for (const item of view.stack) {
    if (item.kind === "ability" && item.source !== null) out.add(item.source);
    for (const t of item.targets) if (t.kind === "object") out.add(t.slot);
  }
  return out;
}

/** Whether two objects are interchangeable, as the engine offers one candidate for both
 * (architecture §7.2): the same card, or token, in the same zone and state, with the same
 * owner and types. */
export function twins(a: ObjectView, b: ObjectView): boolean {
  return (
    a.slot !== b.slot &&
    a.name !== null &&
    a.name === b.name &&
    a.facing === b.facing &&
    a.zone === b.zone &&
    a.owner === b.owner &&
    a.controller === b.controller &&
    a.token === b.token &&
    a.tapped === b.tapped &&
    a.summoningSick === b.summoningSick &&
    a.damage === b.damage &&
    a.attachedTo === b.attachedTo &&
    JSON.stringify([a.types, a.pt, a.counters, a.combat]) ===
      JSON.stringify([b.types, b.pt, b.counters, b.combat])
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
    case "turnFaceUp":
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
  const players = new Map<Side, Act>();
  if (status.kind !== "deciding") {
    return { bySlot, players, picking: false };
  }
  const decision = status.decision;
  let pass: CandidateIndex | undefined;
  let done: CandidateIndex | undefined;
  decision.candidates.forEach(({ option, ...candidate }, index) => {
    const slot = slotOf(option);
    const act = { index, option, ...candidate };
    if (slot !== undefined) {
      bySlot.set(slot, [...(bySlot.get(slot) ?? []), act]);
    } else if (option.kind === "player") {
      players.set(option.player, act);
    } else if (option.kind === "defender") {
      // Attacking no planeswalker is attacking the defending player (CR 508.1b).
      players.set("opponent", act);
    } else if (option.kind === "pass") {
      pass = index;
    } else if (option.kind === "done") {
      done = index;
    }
  });
  const skip = upToOne(decision, bySlot, players);
  // Interchangeable cards collapse to one candidate (architecture §7.2): of two basic lands
  // of a name in your hand the engine offers one, of two untapped Insect tokens one attacker.
  // Either card offers it, unless the stack names one of them.
  const named = referenced(view);
  const offered = view.objects.filter(
    (o) => bySlot.has(o.slot) && !named.has(o.slot),
  );
  for (const o of view.objects) {
    if (
      bySlot.has(o.slot) ||
      named.has(o.slot) ||
      (o.zone !== "hand" && o.zone !== "battlefield")
    ) {
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
    skip,
    picking: decision.kind !== "priority",
    source: decision.source ?? undefined,
  };
}

/** A count of up to one target taken as the target itself
 * ([CR 601.2c](https://yawgatog.com/resources/magic-rules/#R6012c): "If the spell has a
 * variable number of targets, the player announces how many targets they will choose before
 * they announce those targets"): each target the count of one lets the spell choose is
 * picked on the board, announcing one and then it, and none is a skip. Adds the targets to
 * `bySlot` and `players`; returns the count of none, if the decision is such a count. */
function upToOne(
  decision: DecisionView,
  bySlot: Map<number, Act[]>,
  players: Map<Side, Act>,
): CandidateIndex | undefined {
  if (decision.kind !== "targetCount") return undefined;
  const count = (n: number) =>
    decision.candidates.findIndex(
      (c) => c.option.kind === "number" && c.option.n === n,
    );
  const [none, one] = [count(0), count(1)];
  if (decision.candidates.length !== 2 || none < 0 || one < 0) {
    return undefined;
  }
  const { option, ...candidate } = decision.candidates[one]!;
  for (const target of candidate.targets) {
    const named: Candidate =
      target.kind === "object"
        ? { kind: "object", slot: target.slot }
        : { kind: "player", player: target.player };
    const then: Intent[] = [
      { kind: "target", wants: (o) => optionsMatch(o, named) },
    ];
    const act = { index: one, option, ...candidate, then };
    if (target.kind === "object") bySlot.set(target.slot, [act]);
    else players.set(target.player, act);
  }
  return none;
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
  crew: "Crew",
  chooseUpTo: "Choose cards",
  waterbend: "Waterbend",
  exileCost: "Exile a card",
};

/** The prompt's title: what the decision asks, and for whom when there is a source. */
export function prompt(decision: DecisionView, source?: string): string {
  const { ask } = decision;
  switch (ask.kind) {
    case "untap":
      return "Untap";
    case "unlessPays":
      return `Pay ${ask.cost}?`;
    case "discardBasicLand":
      return `Discard a basic land or ${ask.count} cards`;
  }
  const asked = PROMPTS[decision.kind] ?? "Choose";
  return source && decision.kind === "target"
    ? `${asked} for ${source}`
    : asked;
}

/** The picks that put simultaneous triggers on the stack to resolve in `order`, the indexes of
 * `decision`'s triggers from the first to resolve, all but one of them
 * ([CR 405.2](https://yawgatog.com/resources/magic-rules/#R4052): "Each time an object is put on
 * the stack, it's put on top of all objects already there"): the trigger left out first, as it
 * resolves last, then the rest from the last to resolve up, the first to resolve going on
 * unasked as the last left. After the first, each names its trigger by its source and ability,
 * as the candidates' indexes shift, and waits through the targets of the one before. */
export function triggerPlan(
  decision: DecisionView,
  order: CandidateIndex[],
): Intent[] {
  const triggers = decision.candidates.map((c) => c.option);
  const last = triggers.findIndex((_, i) => !order.includes(i));
  const put = [last, ...order.slice(1).reverse()].map((i) => triggers[i]!);
  return put.map((option, i) =>
    i === 0
      ? { kind: "triggerOrder", wants: (o) => optionsMatch(o, option) }
      : {
          kind: "triggerOrder",
          wants: (o) =>
            o.kind === "trigger" &&
            option.kind === "trigger" &&
            o.source === option.source &&
            o.ability === option.ability,
          waits: true,
        },
  );
}

/** The picks that untap the permanents in `picked`, one at a time, then done: each the
 * permanent itself or, failing that, the twin the engine offers in its place (it offers one
 * of interchangeable permanents, architecture §7.2). */
export function untapPlan(
  picked: Iterable<number>,
  objects: Map<number, ObjectView>,
): Intent[] {
  const pick = (slot: number): Intent => ({
    kind: "chooseUpTo",
    wants: (o) => o.kind === "object" && o.slot === slot,
    alike: (o) => {
      const [a, b] = [objects.get(slot), objects.get(slotOf(o) ?? -1)];
      return a !== undefined && b !== undefined && twins(a, b);
    },
  });
  return [
    ...[...picked].map(pick),
    { kind: "chooseUpTo", wants: (o) => o.kind === "done" },
  ];
}
