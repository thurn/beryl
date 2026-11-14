// The ways of a card you draw (enter.ts): held up beside the stack to be read and dropped into
// your hand, or, one of several, sweeping past that place without stopping. Arena's flies
// from the library right across the table to a place beside its stack and comes down into the
// right end of the hand (`vPdoSA5H5xU 17:46.2`, `SswBO8aOOgc 20:28.5`, `WQiYBv23buw 29:14.2`).
import { ALOFT, between, type Pose } from "./pose.ts";
import { bowed } from "./bows.ts";
import {
  DRAW_TURN,
  flatAt,
  type Path,
  type Timing,
  type Turning,
} from "./tracks.ts";

/** The same of a card held up, as shares of its flight in (`HELD.in`): Arena's is edge-on
 * 150 to 250 ms after it leaves, half way across the table, of about 600
 * (`SswBO8aOOgc 20:28.53` to `28.70`, `vPdoSA5H5xU 17:46.17` to `46.43`). */
const HELD_TURNING: Turning = { at: 0.3, each: 0.12 };

/** How a card held up on its way is shared out over its step: the share of the time it
 * flies in, and the share it is held. Arena's draw at a turn's start flies in for about 600
 * ms, holds 200 to 240 and drops for 230 to 300 (data/arena-motion.json, fly, "draw":
 * 20:28.4 and 16:52.4). */
export const HELD = { in: 0.55, hold: 0.2 };

/** When, as a share of its step, a card held up has turned over (`HELD_TURNING`). */
export const HELD_TURN = HELD.in * HELD_TURNING.at;

/** How a card held up leaves the straight line on its way in: it rises early and comes level
 * into its place, as Arena's climbs from the library and then crosses the table
 * (`vPdoSA5H5xU 17:46.23` to `46.83`). And how far toward the hand it sinks while it is
 * held, never quite still, before it drops (`46.83` to `47.10`). */
const HELD_WAY = { bow: { share: 0.12, peak: 0.6 }, sinks: 0.08 };

/** The way of a card that is held up to be read before it lands: from `start` to where
 * `held` puts it for its resting place, bowed `toward` its player's edge of the table's far
 * side and turning over on the way; a hold there, sinking a little; and the drop to where it
 * rests, slow at first and quickening into the hand. */
export const heldUp =
  (
    start: Pose,
    ease: Timing["ease"],
    held: (rest: Pose) => Pose,
    toward = 1,
  ): Path =>
  (progress, rest) => {
    const up = { ...held(rest), depth: ALOFT + rest.depth };
    if (progress >= HELD.in) {
      // Held and then let go, as one motion: it sinks evenly while it is held, and leaves
      // that pace quickening, with no jolt between the two.
      const after = (progress - HELD.in) / (1 - HELD.in);
      const hold = HELD.hold / (1 - HELD.in);
      const { sinks } = HELD_WAY;
      const drop = Math.max(0, (after - hold) / (1 - hold));
      const pace = (sinks * (1 - hold)) / (hold * (1 - sinks));
      const down =
        after < hold
          ? sinks * (after / hold)
          : sinks + (1 - sinks) * (pace * drop + (1 - pace) * drop * drop);
      return between(up, rest, Math.min(1, down), true);
    }
    const flying = progress / HELD.in;
    const through = ease(flying);
    const straight = between(start, up, through, false);
    const bow = { ...HELD_WAY.bow, toward: -toward };
    const pose = {
      ...bowed(straight, start, up, through, bow),
      depth: up.depth,
    };
    if (start.face !== "back") return pose;
    return {
      ...pose,
      face: progress < HELD_TURN ? "back" : pose.face,
      flat: flatAt(flying, HELD_TURNING),
    };
  };

/** The way of a card that passes by a place on its way and never stops there: one curve
 * from `start` to where it rests that goes as far across the table, as far up it and as
 * large as where `by` puts it for that resting place, and no farther, turning over as it
 * leaves if it left face down. Each of several cards drawn together, which sweeps across the
 * table to beside the stack and down into the hand as a card drawn alone does, with no hold
 * (Arena's: `WQiYBv23buw 29:14.2`). */
export const swept =
  (start: Pose, ease: Timing["ease"], by: (rest: Pose) => Pose): Path =>
  (progress, rest) => {
    const through = ease(progress);
    const via = by(rest);
    // A curve of the second degree from `a` to `c`: where `b` lies beyond both it turns
    // back there, and where it lies between them it never turns.
    const on = (a: number, b: number, c: number) => {
      const beyond = (b - a) * (b - c);
      const pull = beyond > 0 ? b + Math.sign(b - a) * Math.sqrt(beyond) : b;
      return (
        (1 - through) ** 2 * a +
        2 * through * (1 - through) * pull +
        through ** 2 * c
      );
    };
    const pose = {
      ...between(start, rest, through, true),
      x: on(start.x, via.x, rest.x),
      y: on(start.y, via.y, rest.y),
      width: on(start.width, via.width, rest.width),
    };
    if (start.face !== "back") return pose;
    return {
      ...pose,
      face: progress < DRAW_TURN.at ? "back" : pose.face,
      flat: flatAt(progress, DRAW_TURN),
    };
  };
