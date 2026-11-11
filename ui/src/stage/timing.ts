// How each step is played, from the design language's data (data/ui-conventions.json, `verbs`):
// a verb's duration, delay and stagger in stage milliseconds and its curve, the medians of
// Arena's own (data/arena-motion.json, plan K27). Where Arena's instances are of several cases
// (a draw, a discard), a step of a case is played with that case's own (`cases`), and with the
// verb's where the case has none. A verb with a number not taken has none, and one with no
// duration snaps.
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

interface Values {
  duration: number | null;
  delay: number | null;
  stagger: number | null;
  curve: string | null;
}

/** The timing `values` give, each number or curve not taken being `over`'s. */
const timing = (values: Values, over?: Timing): Timing => ({
  duration: values.duration ?? over?.duration ?? 0,
  delay: values.delay ?? over?.delay ?? 0,
  stagger: values.stagger ?? over?.stagger ?? 0,
  ease: values.curve
    ? (CURVES[values.curve] ?? SNAP.ease)
    : (over?.ease ?? SNAP.ease),
});

const verbs = new Map(
  conventions.verbs.map((verb): [string, Timing] => [
    verb.name,
    timing(verb.values),
  ]),
);

/** Each verb's cases by Arena's names for them, each with its own timing. */
const cases = new Map(
  conventions.verbs.map((verb): [string, Map<string, Timing>] => {
    const own: Record<string, Values | undefined> =
      ("cases" in verb && verb.cases) || {};
    const whole = verbs.get(verb.name);
    return [
      verb.name,
      new Map(
        Object.entries(own).flatMap(([name, values]) =>
          values ? [[name, timing(values, whole)] as const] : [],
        ),
      ),
    ];
  }),
);

/** Arena's name for the case the step `act` is of (data/arena-motion.json's `case`; `by
 * motion` reads a capture's steps the same, tools' motion.py `case_of`): none for a case
 * Arena's instances do not have, which is played with its verb's values. */
export function caseOf(act: Act): string | undefined {
  switch (act.verb) {
    case "fly":
      switch (`${act.from}>${act.object.zone}`) {
        case "hand>stack":
          return act.object.controller === "you"
            ? "own spell cast"
            : "opponent spell cast";
        case "stack>battlefield":
          return "permanent resolves";
        case "hand>battlefield":
          return "land played";
        case "hand>graveyard":
          return "discard";
        case "library>hand":
          return "draw";
        default:
          return undefined;
      }
    case "appear":
      if (act.from === "nowhere") return "token";
      return act.why === "draw" ? "opening hand" : undefined;
    case "emerge":
      return act.item.kind === "ability" && act.item.triggered
        ? "triggered ability"
        : "activated ability";
    case "vanish":
      if (act.subject.kind === "ability") return "ability finished";
      // Where it goes says more than why: a spell an effect returns is bounced.
      if (act.object?.zone === "hand") return "bounced";
      if (act.object?.zone === "exile") return "exiled";
      switch (act.why) {
        case "destroy":
        case "stateBased":
          return "destroyed";
        case "counter":
          return "countered";
        case "resolve":
          return "resolved instant or sorcery";
        case "sacrifice":
          return "sacrificed";
        default:
          return undefined;
      }
    default:
      return undefined;
  }
}

/** How long after one card of a batch begins to leave for exile the next does (UI
 * Conventions, "A batch exile": one at a time, about 0.3 s apart). Arena's instances are of
 * single cards, so its data has no stagger for the case. */
const BATCH_EXILE = 300;

/** The data's timing of the step `act`: its case's, or its verb's. An ability that has
 * finished is no verb's median: it is removed at once (UI Conventions, "A finished ability";
 * Arena's is gone between two frames). */
export function timingOf(act: Act): Timing {
  if (act.verb === "vanish" && act.subject.kind === "ability") return SNAP;
  const found = caseOf(act);
  const own =
    (found === undefined ? undefined : cases.get(act.verb)?.get(found)) ??
    verbs.get(act.verb) ??
    SNAP;
  return found === "exiled" ? { ...own, stagger: BATCH_EXILE } : own;
}

/** Every step at once: reduced motion. */
export const snap = (): Timing => SNAP;

/** How a held card let go travels back to where it rests: as a card flies. */
export const RELEASE: Timing = verbs.get("fly") ?? SNAP;
