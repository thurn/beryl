// How the stage plays each step, as the session says. The session times every step of the
// scripts it sends (beryl_session's script/timing.rs, from the verbs' values and effects in
// data/ui-conventions.json) and sends one `StepTiming` for each: `timed` attaches each to its
// step as a script enters the stage, in the stage's own terms. The steps the stage makes
// itself while a prompt is open (prompt.ts: its marks, its arrows and the cards it holds up)
// are timed by a short table of their own that the session also gives (`own_timings`), which
// main.tsx asks for once before anything is drawn and gives to `ownTimings`. The stage then
// only lays each beat out in time (beat.ts `laid`).
import { easeIn, easeInOut, easeOut } from "motion";
import conventions from "../../../data/ui-conventions.json" with { type: "json" };
import type {
  Curve,
  OwnStep,
  OwnTiming,
  OwnTimings,
  Side,
  StepEffect,
  StepTiming,
} from "../generated/protocol";
import type { Name } from "./layout.ts";
import type { Act, Played, Script, Step } from "./table.ts";
import type { Effect, Layer, On } from "./effects.ts";
import { SNAP, type Timing } from "./tracks.ts";
import { namedBy } from "./spans.ts";

/** The curves the data names, as progress from 0 to 1 (CSS's of the same names); `none` is a
 * verb in which nothing travels, which changes evenly. */
const CURVES: Record<string, Timing["ease"]> = {
  linear: SNAP.ease,
  none: SNAP.ease,
  "ease-in": easeIn,
  "ease-out": easeOut,
  "ease-in-out": easeInOut,
};

/** Every step at once: reduced motion. */
export const snap = (): Timing => SNAP;

/** How a held card let go travels back to where it rests: as a card flies, by the `fly`
 * verb's values in the data. */
export const RELEASE: Timing = (() => {
  const fly = conventions.verbs.find((verb) => verb.name === "fly")?.values;
  if (!fly) return SNAP;
  return {
    duration: fly.duration ?? 0,
    delay: fly.delay ?? 0,
    stagger: fly.stagger ?? 0,
    ease: fly.curve ? (CURVES[fly.curve] ?? SNAP.ease) : SNAP.ease,
  };
})();

/** The easing of each curve the session names. */
const EASES: Record<Curve, Timing["ease"]> = {
  linear: CURVES.linear!,
  easeIn: CURVES["ease-in"]!,
  easeOut: CURVES["ease-out"]!,
  easeInOut: CURVES["ease-in-out"]!,
};

/** Which layer each effect is drawn in, by its name in the data. */
const LAYERS = new Map(
  conventions.effects.map((effect) => [effect.name, effect.layer as Layer]),
);

/** Whether `on` names a player rather than something on the table. */
const isPlayer = (on: On): on is Side => on === "you" || on === "opponent";

/** The effect `sent` in the stage's terms; none for one the data does not name. */
function effectOf(sent: StepEffect): Effect | undefined {
  const layer = LAYERS.get(sent.name);
  if (layer === undefined) return undefined;
  return {
    name: sent.name,
    layer,
    when: sent.when,
    on: sent.on === undefined ? undefined : namedBy(sent.on),
    into: sent.into,
    duration: sent.duration,
    delay: sent.delay,
    ease: EASES[sent.curve],
  };
}

/** The step timing `sent` in the stage's terms, waiting `after` the things it names. */
function playedOf(sent: StepTiming, after = sent.after.map(namedBy)): Played {
  return {
    case: sent.case,
    timing: {
      duration: sent.duration,
      delay: sent.delay,
      stagger: sent.stagger,
      ease: EASES[sent.curve],
      each: sent.each,
      with: sent.with,
      after: after.filter((on): on is Name => !isPlayer(on)),
      hold: sent.hold,
      leads: sent.leads,
      passes: sent.passes,
      alone: sent.alone,
      opens: sent.opens,
      ahead: sent.ahead,
      closes: sent.closes,
    },
    effects: sent.effects.flatMap((effect) => effectOf(effect) ?? []),
  };
}

/** `script` with each step's timing attached to it (`Step.played`), as the session sent it;
 * `script` itself where it carries none, as one already attached or the stage's own does. */
export function timed<S extends Script>(script: S): S {
  const { timing } = script;
  if (timing === undefined) return script;
  let at = 0;
  const beats = script.beats.map((beat) =>
    beat.map((step) => {
      const sent = timing[at++];
      return sent ? { ...step, played: playedOf(sent) } : step;
    }),
  );
  return { ...script, beats, timing: undefined };
}

/** The shape of a step the stage makes itself, by which the session's table times it. */
function shapeOf(act: Act): OwnStep | undefined {
  switch (act.verb) {
    case "mark":
      return act.span === "end"
        ? { kind: "markEnds" }
        : { kind: "markBegins", mark: act.mark };
    case "point":
      return act.span === "end"
        ? { kind: "arrowEnds" }
        : { kind: "arrowBegins", arrow: act.arrow };
    case "lift":
      if (!("tray" in act)) return undefined;
      return act.span === "end"
        ? { kind: "trayLetGo" }
        : { kind: "trayLifts", sorted: act.sorted ?? false };
    default:
      return undefined;
  }
}

/** One key for each shape (`OwnStep`), whatever order its fields arrive in. */
const keyOf = (step: OwnStep) => JSON.stringify(step, Object.keys(step).sort());

/** The timing of each shape of step the stage makes itself, once it has been given it. */
let own = new Map<string, OwnTiming>();

/** Times the stage's own steps by `table` from now on. */
export function ownTimings(table: OwnTimings) {
  own = new Map(table.entries.map((entry) => [keyOf(entry.step), entry]));
}

/** A step with no timing to play: over as it begins, with no effect. */
const UNTIMED: Played = { timing: SNAP, effects: [] };

/** How `step` is played: as the session sent it, or for a step the stage made itself, as
 * the session's table of them says. An arrow's waits for the steps of its beat that move its
 * ends where the table says so (`OwnTiming.afterEnds`). */
export function played(step: Step): Played {
  if (step.played) return step.played;
  const shape = shapeOf(step.act);
  const entry = shape && own.get(keyOf(shape));
  if (!entry) return UNTIMED;
  const { act } = step;
  const ends =
    entry.afterEnds && act.verb === "point"
      ? [namedBy(act.from), namedBy(act.to)]
      : undefined;
  return playedOf(entry.timing, ends);
}
