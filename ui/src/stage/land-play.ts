import type { Resting, StageLayout } from "./layout.ts";
import { between, type Pose } from "./pose.ts";
import type { Path } from "./tracks.ts";

/** Where the forward swing is farthest right and when it reaches its lowest point, as shares
 * of the flight. In Arena the land reaches this part of the table whether it leaves near
 * the center or from the left of the fan (`AhTcvQ-lQQQ 23:51.50`, `UrGalw_f35w 16:46.33`,
 * `pE4XSNi-nZc 20:13.60`). */
const SWING = { right: 0.43, low: 0.5, shrink: 0.62, lean: 28 };

export const opponentLandTurn = (width: number, height: number) => ({
  x: 0.67 * width,
  y: 0.3 * height,
  width: 0.09 * width,
});

const smooth = (progress: number) => progress ** 2 * (3 - 2 * progress);
const mix = (from: number, to: number, progress: number) =>
  from + (to - from) * progress;

/** A played land is face up as it leaves the opponent's hand. Its center passes to the right
 * of the opponent's life badge before gliding back into its slot. Using a point on the table
 * for the turn keeps the swing wide when the hand slot is already close to the lands. */
export function playedLand(
  layout: StageLayout,
  before: Resting,
  start: Pose,
  resting: Resting,
): Path {
  const origin = layout.middle(before, start);
  return (progress, rest) => {
    const end = layout.middle(resting, rest);
    const turn = layout.opponentLandTurn();
    const outbound = Math.min(1, progress / SWING.right);
    const returning = Math.max(0, (progress - SWING.right) / (1 - SWING.right));
    const down = Math.min(1, progress / SWING.low);
    const rise = Math.max(0, (progress - SWING.low) / (1 - SWING.low));
    const x = mix(
      mix(origin.x, turn.x, smooth(outbound ** 0.7)),
      end.x,
      smooth(returning ** 1.2),
    );
    const y = mix(
      mix(origin.y, turn.y, Math.sin((down ** 0.88 * Math.PI) / 2)),
      end.y,
      rise ** 2,
    );
    const angle = start.rotation - rest.rotation;
    const upright = angle - 180 * Math.round(angle / 180);
    const height = Math.sin(Math.PI * progress);
    // The card stays wide across most of the table and shrinks as it enters the land row.
    const shrinking = Math.max(
      0,
      (progress - SWING.shrink) / (1 - SWING.shrink),
    );
    const broad = Math.max(start.width, turn.width);
    const width = mix(
      mix(start.width, broad, smooth(outbound)),
      rest.width,
      smooth(shrinking),
    );
    const pose: Pose = {
      ...between(start, rest, returning, true),
      width,
      rotation:
        rest.rotation +
        upright * (1 - outbound) +
        SWING.lean * Math.sin(2 * Math.PI * outbound),
      flat: 1 - 0.28 * height,
      shadow: {
        x: x - start.width * 1.3 * height,
        y: y + start.width * 1.5 * height,
        width: 0.8 * width,
        opacity: 0.55 * height,
      },
    };
    const center = layout.middle(resting, pose);
    return { ...pose, x: pose.x + x - center.x, y: pose.y + y - center.y };
  };
}
