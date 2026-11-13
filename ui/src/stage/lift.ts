// The flight of a permanent from the stack to the battlefield (tracks.ts `glide` plays it):
// thrown up the screen, across at that height, and down onto its place, as Arena's is.
import type { Pose } from "./pose.ts";
import type { Act } from "./table.ts";

/** How a permanent that resolves leaves the stack for its place, as Arena's does for either
 * player (`UrGalw_f35w 17:01.13`, `pE4XSNi-nZc 23:02.54` and `21:20.03`, filmed whole at 30
 * frames a second). It does not slide along a line. In its first frame off the stack it is
 * already smaller, `width` of the spell's width, and it is thrown up the screen: within
 * `risen` of its time it is `height` of the spell's width above the straight line, it stays
 * that high until `held` of its time while it crosses most of the way, and then it drops
 * onto its place, shrinking to its size there as it comes down. It also leaves faster than
 * the verb's curve alone: `launch` sharpens the curve, so that it is half way across after
 * a quarter of its time. */
export const LIFT = {
  height: 0.95,
  risen: 0.2,
  held: 0.57,
  width: 0.8,
  shrunk: 0.06,
  launch: 1.4,
};

/** Whether the flight of the step `act` is a permanent's from the stack to the battlefield,
 * which is thrown up and drops onto its place (`LIFT`). */
export const lifts = (act: Act) =>
  act.verb === "fly" &&
  act.from === "stack" &&
  act.object.zone === "battlefield";

/** How high above its straight line a permanent leaving the stack is, `progress` of its time
 * through, as a share of its greatest height: up quickly, level, then down evenly. */
export function liftAt(progress: number): number {
  const at = Math.min(1, Math.max(0, progress));
  if (at < LIFT.risen) return 1 - (1 - at / LIFT.risen) ** 2;
  return at <= LIFT.held ? 1 : 1 - (at - LIFT.held) / (1 - LIFT.held);
}

/** `pose`, which is on the straight way from `start` to `rest`, as a permanent leaving the
 * stack has it `progress` of its time through: lifted up the screen, and as wide as its
 * height above the table makes it. */
export function lifted(
  pose: Pose,
  start: Pose,
  rest: Pose,
  progress: number,
): Pose {
  const high = liftAt(progress);
  // It never rises past the table's far edge, which is the top of the stage: a place in the
  // opponent's rows has less room over it than Arena's camera leaves.
  const room = Math.max(0, Math.min(start.y, rest.y));
  // It is the spell's own width as it leaves, smaller within two frames, and from there
  // comes down to its width at rest as it descends.
  const aloft = LIFT.width * start.width;
  const down = (at: number) =>
    rest.width + (aloft - rest.width) * (1 - at ** 1.5);
  const wide =
    progress < LIFT.shrunk
      ? start.width +
        (down(LIFT.shrunk) - start.width) * (progress / LIFT.shrunk)
      : down(progress);
  return {
    ...pose,
    // A pose is placed by its top-left corner: it stays about its middle as it narrows.
    x: pose.x + (pose.width - wide) / 2,
    y: pose.y - Math.min(LIFT.height * start.width, room) * high,
    width: wide,
  };
}
