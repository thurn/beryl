// Phase stops: the steps where the player stops with the stack empty under the smart stops,
// on their own turns and the opponent's (`StopsSetting.yours`, `.theirs`), set by clicking the
// phase tracker's icons as on Arena. The browser remembers them from game to game.
import type { Phase } from "../components/Controls";
import type { Step, StopsSetting } from "../ipc";

/** The steps a phase icon stands for. Combat's are those after attackers and after blockers
 * are declared, the smart stops' own; beginning's, those the player gets priority in. */
export const PHASE_STEPS: Record<Phase, Step[]> = {
  beginning: ["upkeep", "draw"],
  main1: ["precombatMain"],
  combat: ["declareAttackers", "declareBlockers"],
  main2: ["postcombatMain"],
  end: ["end"],
};

/** The phases with a stop among `steps`. */
export function stoppedPhases(steps: Step[]): Set<Phase> {
  const on = new Set<Phase>();
  for (const [phase, inPhase] of Object.entries(PHASE_STEPS)) {
    if (inPhase.some((s) => steps.includes(s))) on.add(phase as Phase);
  }
  return on;
}

/** `steps` with `phase`'s stops cleared if it has any, or set if not. */
export function togglePhase(steps: Step[], phase: Phase): Step[] {
  const inPhase = PHASE_STEPS[phase];
  return inPhase.some((s) => steps.includes(s))
    ? steps.filter((s) => !inPhase.includes(s))
    : [...steps, ...inPhase];
}

export function sameStops(a: StopsSetting, b: StopsSetting): boolean {
  const same = (x: Step[], y: Step[]) =>
    x.length === y.length && x.every((s) => y.includes(s));
  return (
    a.fullControl === b.fullControl &&
    same(a.yours, b.yours) &&
    same(a.theirs, b.theirs)
  );
}

/** Where the browser remembers the player's stops. */
const SAVED = "beryl.stops";

/** The stops the player last chose here, if the browser kept them. */
export function savedStops(): StopsSetting | undefined {
  try {
    const saved: unknown = JSON.parse(localStorage.getItem(SAVED) ?? "null");
    if (typeof saved !== "object" || saved === null) return undefined;
    const { fullControl, yours, theirs } = saved as Record<string, unknown>;
    const steps = (x: unknown): x is Step[] =>
      Array.isArray(x) && x.every((s) => typeof s === "string");
    if (typeof fullControl !== "boolean" || !steps(yours) || !steps(theirs)) {
      return undefined;
    }
    return { fullControl, yours, theirs };
  } catch {
    return undefined;
  }
}

export function saveStops(stops: StopsSetting) {
  try {
    localStorage.setItem(SAVED, JSON.stringify(stops));
  } catch {
    // A browser that keeps nothing forgets them with the game.
  }
}
