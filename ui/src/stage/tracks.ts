// The tracks a step gives what it moves: where an entry is `progress` of the way through the
// step, given where it now rests, which a later step may have moved. Each is a function of
// the poses it is made from and of progress alone, so a pose is the same however its stage
// time is reached.
import type { Act } from "../generated/protocol";
import { abilityName, type Name } from "./layout.ts";
import { ALOFT, between, type Pose } from "./pose.ts";
import type { Timing } from "./stage.ts";

/** An entry's pose `progress` of the way along its track, resting at `rest`. */
export type Path = (progress: number, rest: Pose) => Pose;

/** How far through its flight a card that leaves face down and lands face up turns over:
 * Arena's opponent's spell is edge-on a little past half way (data/arena-motion.json, fly,
 * "opponent spell cast": 16:47.25 to 16:47.47 of 400 ms). */
export const TURN = 0.55;

/** The glide to its resting place of an entry that was at `start`: `aloft` for what a step
 * is about, and at its resting depth for what makes room. With `turns`, it shows its back
 * until it is `TURN` of the way there. */
export const glide =
  (start: Pose, ease: Timing["ease"], aloft: boolean, turns = false): Path =>
  (progress, rest) => {
    const pose = between(start, rest, ease(progress), aloft);
    return turns && progress < TURN ? { ...pose, face: "back" } : pose;
  };

/** An entry that is at its resting place from the first, and comes to be seen there. */
export const fade =
  (ease: Timing["ease"]): Path =>
  (progress, rest) => ({ ...rest, opacity: rest.opacity * ease(progress) });

/** An entry that goes from sight where it was at `start`, and never travels. */
export const dissolve =
  (start: Pose, ease: Timing["ease"]): Path =>
  (progress) => ({ ...start, opacity: start.opacity * (1 - ease(progress)) });

/** How a card held up on its way is shared out over its step: the share of the time it
 * flies in, and the share it is held. Arena's draw at a turn's start flies in for about 600
 * ms, holds 200 to 240 and drops for 230 to 300 (data/arena-motion.json, fly, "draw":
 * 20:28.4 and 16:52.4). */
export const HELD = { in: 0.55, hold: 0.2 };

/** When, as a share of its step, a card held up has turned over: `TURN` of its way in. */
export const HELD_TURN = HELD.in * TURN;

/** The way of a card that is held up to be read before it lands: from `start` to where
 * `held` puts it for its resting place, turning over on the way, a hold there, and the drop
 * to where it rests. */
export const heldUp =
  (start: Pose, ease: Timing["ease"], held: (rest: Pose) => Pose): Path =>
  (progress, rest) => {
    const up = { ...held(rest), depth: ALOFT + rest.depth };
    const dropping =
      (progress - HELD.in - HELD.hold) / (1 - HELD.in - HELD.hold);
    if (dropping > 0) return between(up, rest, ease(dropping), true);
    const flying = Math.min(1, progress / HELD.in);
    const pose = {
      ...between(start, up, ease(flying), false),
      depth: up.depth,
    };
    return start.face === "back" && progress < HELD_TURN
      ? { ...pose, face: "back" }
      : pose;
  };

/** What a step's act is about, if it is something on the table: its card, or the ability it
 * puts on the stack or takes from it. */
export function subjectOf(act: Act): Name | undefined {
  if (act.verb === "emerge") return abilityName(act.id);
  if (act.verb === "vanish" && act.subject.kind !== "player") {
    return act.subject.kind === "object"
      ? act.subject.slot
      : abilityName(act.subject.id);
  }
  return "object" in act ? act.object?.slot : undefined;
}

/** Whether `act` is your draw of a card you see: the one flight in which a card is held up
 * to be read (UI Conventions, Verbs, fly). */
export const drawsToRead = (act: Act) =>
  act.verb === "fly" &&
  act.why === "draw" &&
  act.from === "library" &&
  act.object.zone === "hand" &&
  act.object.owner === "you" &&
  act.object.name !== null;
