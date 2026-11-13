// What the opening's screen plans of its decisions (screens/Opening.tsx), and what the table's
// layout asks of a view's objects. Pure functions.
import type {
  Candidate,
  CandidateIndex,
  DecisionView,
  Kind,
  ObjectView,
  PlayerView,
  Purpose,
} from "../generated/protocol";

/** A pick a later decision should get: one of its kind, the candidate `wants` names or,
 * failing that, one `alike` names (a card the engine offers in place of an interchangeable
 * one, architecture §7.2); `picked` hears which it was. The opening says a mulligan's cards
 * for the bottom all at once. */
export interface Intent {
  kind: Kind;
  /** The purpose of the prompt the decision's pick must belong to (`PromptView`'s), for a
   * pick whose kind other prompts are asked in too: a cost's or a rule's pick is a
   * `chooseObject`, a `yesNo` or a `manaPayment` like an effect's. */
  purpose?: Purpose;
  wants(option: Candidate): boolean;
  alike?(option: Candidate): boolean;
  picked?(option: Candidate): void;
}

/** Whether `intent` is for `decision`: of its kind and, where it says, its purpose. */
const answers = (decision: DecisionView, intent: Intent) =>
  decision.kind === intent.kind &&
  (intent.purpose === undefined || decision.prompt.purpose === intent.purpose);

/** Whether `decision` aims attackers: asks what the creatures its prompt is about attack. */
export const aimsAttack = ({ prompt }: DecisionView) =>
  prompt.purpose === "attack" && prompt.part === "aim";

/** The candidate of `decision` that `intent` names, if it is of the intent's kind. */
function intended(
  decision: DecisionView,
  intent: Intent,
): CandidateIndex | undefined {
  if (!answers(decision, intent)) return undefined;
  const options = decision.candidates.map((c) => c.option);
  let index = options.findIndex((o) => intent.wants(o));
  if (index < 0 && intent.alike) {
    index = options.findIndex((o) => intent.alike!(o));
  }
  return index < 0 ? undefined : index;
}

/** What `plan` picks at `decision`: the candidate its first intent names, if any (which it
 * is told of), and the intents after it. */
export function follow(
  decision: DecisionView,
  plan: Intent[],
): { index?: CandidateIndex; rest: Intent[] } {
  const intent = plan[0];
  const index = intent && intended(decision, intent);
  if (intent && index !== undefined) {
    intent.picked?.(decision.candidates[index]!.option);
  }
  return { index, rest: plan.slice(1) };
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

/** The decisions before the first turn, which the opening screen asks over the table. */
export const isOpening = ({ kind, prompt }: DecisionView) =>
  kind === "playOrDraw" ||
  kind === "keepOrMulligan" ||
  prompt.purpose === "put-on-bottom";
