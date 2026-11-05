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
  /** At a decision among things (targets, cards): a card with one candidate is picked by
   * clicking it, so it glows cyan; elsewhere clicking a card lists its candidates. */
  picking: boolean;
  /** The card the choice is for, glowing gold: the spell or ability on top of the stack. */
  source?: number;
}

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
  decision.candidates.forEach(({ label, option }, index) => {
    const slot = slotOf(option);
    if (slot !== undefined) {
      bySlot.set(slot, [...(bySlot.get(slot) ?? []), { index, label, option }]);
    } else if (option.kind === "player") {
      players.set(option.player, index);
    } else if (option.kind === "pass") {
      pass = index;
    }
  });
  const top = view.stack[view.stack.length - 1];
  const source =
    decision.kind === "priority" || !top
      ? undefined
      : top.kind === "spell"
        ? top.slot
        : (top.source ?? undefined);
  return {
    bySlot,
    players,
    pass,
    picking: decision.kind !== "priority",
    source,
  };
}

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
