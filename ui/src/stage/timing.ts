// How each verb is played, from the design language's data (data/ui-conventions.json, `verbs`):
// a verb's duration, delay and stagger in stage milliseconds. One not yet measured from Arena
// (K27) has none, and snaps. No verb has a curve yet: every glide is linear until the first
// measured verb settles what a curve's number means.
import conventions from "../../../data/ui-conventions.json";
import type { Verb } from "../generated/protocol";
import { SNAP, type Timing } from "./stage.ts";

const verbs = new Map(
  conventions.verbs.map(({ name, values }): [string, Timing] => [
    name,
    {
      ...SNAP,
      duration: values.duration ?? 0,
      delay: values.delay ?? 0,
      stagger: values.stagger ?? 0,
    },
  ]),
);

/** The data's timing of `verb`. */
export const timingOf = (verb: Verb): Timing => verbs.get(verb) ?? SNAP;

/** Every verb at once: reduced motion. */
export const snap = (): Timing => SNAP;
