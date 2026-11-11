// The labels the turn controls and the combat banner show for each step.
import type { PassTo, PlayerView, Step } from "../generated/protocol";

export const STEP_NAMES: Record<Step, string> = {
  untap: "Untap",
  upkeep: "Upkeep",
  draw: "Draw",
  precombatMain: "Main 1",
  beginningOfCombat: "Beginning of combat",
  declareAttackers: "Attackers",
  declareBlockers: "Blockers",
  firstStrikeDamage: "First-strike damage",
  combatDamage: "Combat damage",
  endOfCombat: "End of combat",
  postcombatMain: "Main 2",
  end: "End step",
  cleanup: "Cleanup",
};

/** The turn button at a halt of the session's (`DecisionView.halt`), where passing is all the
 * player can do: ending their own turn from its first main phase. */
export const HALT_BUTTON = { label: "End Turn", caption: "Nothing to play" };

/** The turn button for passing priority (decision D13): where the pass goes, as the engine
 * says (`DecisionView.passTo`). */
export function passButton(
  view: PlayerView,
  to: PassTo,
): { label: string; caption: string } {
  switch (to.kind) {
    case "resolve":
      return { label: "Resolve", caption: "" };
    case "combatDamage":
      // combat.png's Resolve Damage.
      return { label: "Resolve Damage", caption: "Combat damage" };
    case "step":
      return {
        label: "Next",
        caption: `To ${to.step === "end" ? "End Step" : STEP_NAMES[to.step]}`,
      };
    case "nextTurn":
      return view.turn.active === "you"
        ? { label: "End Turn", caption: "To Opponent's Turn" }
        : { label: "Next", caption: "To Your Turn" };
  }
}

/** The step banner during combat: what is being declared, what has been, or the damage. */
export function combatBanner(
  view: PlayerView,
): { title: string; detail: string } | undefined {
  const step = view.turn.step;
  const status = view.status;
  const kind = status.kind === "deciding" ? status.decision.kind : undefined;
  const fighting = view.objects.some((o) => o.combat?.kind === "attacking");
  const detail =
    kind === "attacker" || kind === "attackTarget"
      ? "Declare attackers"
      : kind === "blocker" || kind === "blockTarget"
        ? "Declare blockers"
        : !fighting
          ? undefined
          : step === "declareAttackers"
            ? "Attackers declared"
            : step === "declareBlockers"
              ? "Blockers declared"
              : step === "firstStrikeDamage" || step === "combatDamage"
                ? "Combat damage"
                : undefined;
  return detail && { title: "Combat", detail };
}
