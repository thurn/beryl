// How the hand card the pointer is over is raised out of the fan (stage/dom.ts `raise`; UI
// Conventions, "Hover inspection"). It imports nothing, so tests run it with no browser.
import type { Box } from "./placement";

/** How the hand card the pointer is over rises out of the fan, as Arena's does: straightened
 * and grown `scale` times about its top center, standing `floor` above the stage's bottom
 * edge. Arena's raised hand card, filmed whole (SswBO8aOOgc 21:02.65, IEXcjBn3BXk 18:06.70),
 * is 0.29 to 0.31 of the table's height wide, its top 0.58 of the way down the table and its
 * foot just inside the screen's bottom edge. That is 1.8 times one of Arena's cards at rest in
 * the hand, and 1.5 times one of Beryl's, which are drawn larger (placement.ts `HAND.width`). The card's node stays where the fan puts it (stage/raised.ts), so
 * hovering moves nothing else and the pointer keeps the card. Where the pointer keeps it
 * reaches `lift` above the card's top in the fan, and less where that would come over your
 * lands (`Placement.lift`), which stay yours to point at under the raised card. */
export const RAISE = { lift: 70, scale: 1.5, floor: 6 };

/** How the hand card at `pose` is raised (RAISE): how many times as large it grows about its
 * top center once turned upright, and how far it rises straight up the screen, in stage
 * pixels, for its foot to stand `RAISE.floor` above the bottom of a stage `height` tall. Its
 * frame is `pose.width` wide and turned `pose.rotation` degrees about its top-left corner. It
 * grows less, down to its size in the fan, where it would come over any of the `clear` boxes:
 * the cards a prompt asks the player to pick from, which a card shown whole never covers. */
export function raisedPose(
  pose: { x: number; y: number; width: number; rotation: number },
  height: number,
  clear: readonly Box[] = [],
): { scale: number; rise: number } {
  const a = (pose.rotation * Math.PI) / 180;
  const middle = pose.x + (Math.cos(a) * pose.width) / 2;
  const top = pose.y + (Math.sin(a) * pose.width) / 2;
  const tall = (pose.width * 88) / 63;
  const foot = height - RAISE.floor;
  // The highest its top may stand, below every box it would come over.
  const half = (pose.width * RAISE.scale) / 2;
  const highest = Math.max(
    -Infinity,
    ...clear
      .filter((box) => box.right >= middle - half && box.left <= middle + half)
      .map((box) => box.bottom + CLEAR_MARGIN),
  );
  // Then as large as fits between there and its foot, and with no room even for its size in
  // the fan, as high as it may stand short of its foot.
  const scale = Math.max(1, Math.min(RAISE.scale, (foot - highest) / tall));
  const rise = Math.min(top + scale * tall - foot, top - highest);
  return { scale, rise: Math.max(0, rise) };
}

/** How far a raised hand card stays below a card it keeps clear of (`raisedPose`). */
const CLEAR_MARGIN = 4;
