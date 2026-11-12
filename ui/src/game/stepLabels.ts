// The labels the turn controls and the combat banner show for each step.
import type { PlayerView, Step } from "../generated/protocol";

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
