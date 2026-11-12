// A beat laid out in time (script/mod.rs, Shape: "a beat is the steps that happen together,
// in the order a stagger takes them"): when each of its steps begins and ends, from the stage
// time the beat before it ended. The times are a function of the script and of the design
// language's timings alone, worked out before any step of the beat plays, so a script's
// timeline is the same however the clock reaches it (stage.ts).
import type { Act, PlayerView, ScriptStep, Verb } from "../generated/protocol";
import type { Name } from "./layout.ts";
import { apply, type Table } from "./table.ts";
import { moverOf, type Timing } from "./tracks.ts";

/** The step playing now: its place in its script counting every step, its verb, its start
 * and end in stage milliseconds, and the step itself. */
export interface Playing {
  index: number;
  verb: Verb;
  from: number;
  to: number;
  step: ScriptStep;
}

/** One step of the beat under way. */
export interface Run extends Playing {
  /** When its turn in its beat came, which its delay counts from (`Timing`). */
  turn: number;
  ease(progress: number): number;
  each?: number;
  begun: boolean;
  /** The table as the step found it, once it has begun. */
  found?: Table;
  ended: boolean;
}

/** The steps of `beat` as they play from stage time `at`, over the `table` the first of them
 * finds, `played` steps of their script having come before; and when the beat is over, which
 * is when its last motion is. Each step is timed by the table it will find. A step begins its
 * delay after its turn comes, and never before the step before it; the next step's turn is a
 * stagger after. One played `with` the step before it takes that step's turn, and one that is
 * drawn between things at rest (`Timing.after`) waits for the steps of the beat that move
 * them. One that ends a span waits until the span has stood whole for its hold
 * (`Timing.hold`), `whole` saying since when the span a step ends has. */
export function laid(
  beat: readonly ScriptStep[],
  played: number,
  at: number,
  table: Table,
  timings: (act: Act, found?: PlayerView, before?: Act) => Timing,
  whole: (act: Act) => number | undefined,
): { steps: Run[]; end: number } {
  let [from, turn, last, end] = [at, at, at, at];
  /** When each thing a step of the beat moves has come to rest. */
  const landed = new Map<Name, number>();
  const steps = beat.map((step, i): Run => {
    const timing = timings(step.act, table.view, beat[i - 1]?.act);
    table = apply(table, step.act);
    const own = timing.with ? last : turn;
    const held = timing.hold === undefined ? undefined : whole(step.act);
    const rested = Math.max(
      timing.with ? from : own,
      ...(timing.after ?? []).map((name) => landed.get(name) ?? at),
      held === undefined ? at : held + timing.hold!,
    );
    from = Math.max(from, rested + timing.delay);
    if (!timing.with) turn = own + timing.stagger;
    last = own;
    const to = from + timing.duration;
    end = Math.max(end, to);
    const mover = moverOf(step.act);
    if (mover !== undefined) {
      landed.set(mover, Math.max(landed.get(mover) ?? at, to));
    }
    return {
      index: played + i,
      verb: step.act.verb,
      turn: own,
      from,
      to,
      step,
      ease: timing.ease,
      each: timing.each,
      begun: false,
      ended: false,
    };
  });
  return { steps, end };
}

/** A step beginning or ending, for what follows steps besides motion (`ScriptStage.follow`). */
export interface Edge {
  edge: "start" | "end";
  /** The stage time it happens at, whenever the tick that finds it runs. */
  at: number;
  /** The step's own start and end, in stage time, and when its turn in its beat came: it
   * begins its delay after, or as an earlier step of its beat does, whichever is later. */
  span: Readonly<Pick<Run, "from" | "to" | "turn">>;
  step: ScriptStep;
  /** The table as the step found it, before its act. */
  table: Table;
  /** The steps of its beat before it, in order. */
  earlier: readonly ScriptStep[];
}
