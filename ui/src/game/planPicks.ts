// A payment plan the engine asks outside the cast step, as when a face-down permanent is
// turned face up (CR 116.2b) or a cast's targets come between its X and its payment
// ([CR 601.2g](https://yawgatog.com/resources/magic-rules/#R6012g): "the player then has a
// chance to activate mana abilities"). Arena asks "Pay {cost}." with one Auto Pay and the
// permanents tapped on the table (`0VsYGs88NXA 20:46.0`), never a sentence per plan, so the
// plan is paid as the cast step pays under full control: Auto Pay takes the plan the session
// ranks first, and permanents picked on the table settle the plan that taps them all.
import { useState } from "react";
import { play as playSound } from "../audio/sound";
import type { CandidateIndex, DecisionView } from "../generated/protocol";

const NO_SLOTS: number[] = [];

export function usePlanPicks(decision: DecisionView | undefined) {
  const plans =
    decision?.kind === "paymentPlan"
      ? decision.candidates.flatMap((c, index) =>
          c.option.kind === "plan"
            ? [{ index, taps: c.payments[0]?.taps ?? NO_SLOTS }]
            : [],
        )
      : [];
  const key = decision && `${decision.id.session}:${decision.id.seq}`;
  const [picks, setPicks] = useState<{ key: string; selected: number[] }>();
  const selected =
    plans.length > 0 && picks && picks.key === key ? picks.selected : NO_SLOTS;
  const covers = (slots: number[]) =>
    plans.find((p) => slots.every((s) => p.taps.includes(s)));
  const chosen = covers(selected) ?? plans[0];
  return {
    active: chosen !== undefined,
    /** The permanents picked on the table. */
    selected,
    /** Picks or unpicks the permanent in `slot`, if a plan taps it; false if none does. A
     * pick that no plan taps along with the others is ignored. */
    toggle(slot: number): boolean {
      if (!key || !plans.some((p) => p.taps.includes(slot))) return false;
      const next = selected.includes(slot)
        ? selected.filter((s) => s !== slot)
        : [...selected, slot];
      if (covers(next)) {
        playSound("select");
        setPicks({ key, selected: next });
      }
      return true;
    },
    /** The prompt's Auto Pay: the plan the picks settle, previewing what it taps. */
    offer: (pick: (index: CandidateIndex) => void) =>
      chosen && {
        ready: true,
        taps: chosen.taps,
        autoPay: () => pick(chosen.index),
      },
  };
}
