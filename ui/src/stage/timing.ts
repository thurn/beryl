// How each step is played, from the design language's data (data/ui-conventions.json, `verbs`):
// a verb's duration, delay and stagger in stage milliseconds and its curve, the medians of
// Arena's own (data/arena-motion.json, plan K27). A verb with a number not taken has none, and
// one with no duration snaps.
import { easeIn, easeInOut, easeOut } from "motion";
import conventions from "../../../data/ui-conventions.json";
import type { Act } from "../generated/protocol";
import { SNAP, type Timing } from "./stage.ts";

/** The curves the data names, as progress from 0 to 1 (CSS's of the same names); `none` is a
 * verb in which nothing travels, which changes evenly. */
const CURVES: Record<string, Timing["ease"]> = {
  linear: SNAP.ease,
  none: SNAP.ease,
  "ease-in": easeIn,
  "ease-out": easeOut,
  "ease-in-out": easeInOut,
};

const verbs = new Map(
  conventions.verbs.map(({ name, values }): [string, Timing] => [
    name,
    {
      duration: values.duration ?? 0,
      delay: values.delay ?? 0,
      stagger: values.stagger ?? 0,
      ease: CURVES[values.curve ?? "none"] ?? SNAP.ease,
    },
  ]),
);

/** The data's timing of the step `act`. An ability that has finished is no verb's median: it
 * is removed at once (UI Conventions, "A finished ability"; Arena's is gone between two
 * frames). */
export function timingOf(act: Act): Timing {
  if (act.verb === "vanish" && act.subject.kind === "ability") return SNAP;
  return verbs.get(act.verb) ?? SNAP;
}

/** Every step at once: reduced motion. */
export const snap = (): Timing => SNAP;

/** How a held card let go travels back to where it rests: as a card flies. */
export const RELEASE: Timing = verbs.get("fly") ?? SNAP;
