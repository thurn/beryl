// Step stops: the steps where the player stops with the stack empty under the smart stops, on
// their own turns and the opponent's (`StopsSetting.yours`, `.theirs`), set by clicking the
// icons about each life orb (Steps.tsx) as on Arena. The browser remembers them from game to
// game.
import type { Step, StopsSetting } from "../ipc";
import { key } from "../profile";

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
    const saved: unknown = JSON.parse(
      localStorage.getItem(key(SAVED)) ?? "null",
    );
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
    localStorage.setItem(key(SAVED), JSON.stringify(stops));
  } catch {
    // A browser that keeps nothing forgets them with the game.
  }
}
