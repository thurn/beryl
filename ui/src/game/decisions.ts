// What the human can do on the board at their open decision: which cards they can act on and
// how, which they can pick, and what the prompt asks. Pure functions of the decision.
import type {
  Candidate,
  CandidateIndex,
  DecisionView,
  Kind,
  PlayerView,
  Side,
} from "../generated/protocol";

/** One thing a card offers: the candidate's index and its label. */
export interface Act {
  index: CandidateIndex;
  label: string;
  option: Candidate;
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
 * one, architecture §7.2). A drag says the block target after the blocker it declares; an
 * overlay says a whole scry. */
export interface Intent {
  kind: Kind;
  wants(option: Candidate): boolean;
  alike?(option: Candidate): boolean;
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
  decision.candidates.forEach(({ label, option }, index) => {
    const slot = slotOf(option);
    if (slot !== undefined) {
      bySlot.set(slot, [...(bySlot.get(slot) ?? []), { index, label, option }]);
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
