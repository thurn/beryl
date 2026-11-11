// Where the inspector stands beside the table for a card pointed at on it, and the inspector
// drawn there with what is attached to the card: the same on the old board and the stage's
// (UI Conventions, "Hover inspection").
import type {
  ObjectView,
  PlayerView,
  RefusalView,
} from "../generated/protocol";
import { fromMiddle, STAGE } from "./perspective";
import {
  Inspect,
  INSPECT_WIDTH,
  inspectorBox,
  type InspectPlace,
} from "./Piles";
import type { Box } from "./placement";
import { STACK } from "./stackLayout";

/** Where the inspector stands: beside the table on the side away from the card at `x`, never over a
 * life orb, which lies between the two sides. While the stack or a prompt holds the table's
 * right (`taken`), it is shown smaller in the column between the life orbs and the stack
 * instead. It never covers a card glowing as a target (`targets`, their boxes), which the
 * player may be about to pick: where its side would, it takes the other, and where both would,
 * it shrinks toward a side's outer edge until it clears them. Failing that, it covers as little
 * of them as it can. */
export function inspectorPlace(
  x: number,
  taken: boolean,
  targets: Box[],
): InspectPlace {
  const right: InspectPlace = taken
    ? betweenOrbsAndStack()
    : { left: STAGE.width - INSPECT_WIDTH - 24 };
  const sides = x >= STAGE.width / 2 ? [LEFT, right] : [right, LEFT];
  const covered = (place: InspectPlace) => {
    const box = inspectorBox(place);
    return targets.reduce((sum, t) => {
      const w = Math.min(box.right, t.right) - Math.max(box.left, t.left);
      const h = Math.min(box.bottom, t.bottom) - Math.max(box.top, t.top);
      return sum + (w > 0 && h > 0 ? w * h : 0);
    }, 0);
  };
  for (const side of sides) {
    const full = side.width ?? INSPECT_WIDTH;
    for (let width = full; width >= INSPECT_MIN_WIDTH; width -= 10) {
      // The left side keeps its left edge; the others their right one, by the stack or the
      // stage's edge.
      const place = {
        ...side,
        left: side === LEFT ? side.left : side.left + full - width,
        width,
      };
      if (covered(place) === 0) return place;
    }
  }
  return sides.reduce((a, b) => (covered(b) < covered(a) ? b : a));
}

/** The table's left, under the badges. */
export const LEFT: InspectPlace = { left: 24 };
/** How narrow the inspector gets to clear a target: a scan's rules text still reads. */
export const INSPECT_MIN_WIDTH = 230;

/** Right of the life orbs (x 670 to 930 on the base stage, held to its middle), left of the
 * stack (STACK.x, held to its right edge), and below the opponent's orb and above yours (y 159
 * and 618), with room for the artist's line. */
export const betweenOrbsAndStack = (): InspectPlace => ({
  left: fromMiddle(940),
  top: 172,
  width: STACK.x - 54 - fromMiddle(940),
});

/** The inspector for `object`, where `place` puts it (inspectorPlace), with what is attached
 * to it. */
export function InspectAt({
  view,
  object,
  place,
  lit,
  refusal,
}: {
  view: PlayerView;
  object?: ObjectView;
  place: InspectPlace;
  lit?: number;
  refusal?: RefusalView;
}) {
  const by =
    refusal?.kind === "timing" && refusal.by !== null
      ? view.objects.find((o) => o.slot === refusal.by)?.name
      : undefined;
  return (
    <Inspect
      object={object}
      attached={view.objects.filter(
        (o) => o.zone === "battlefield" && o.attachedTo === object?.slot,
      )}
      place={place}
      lit={lit}
      refusal={refusal && { reason: refusal, by: by ?? undefined }}
    />
  );
}
