// The Revealed Cards pile (UI Conventions, Verbs, showcase): the faces of the cards held up
// for both players to read (table.ts `Revealed`), which a layout rests in a small fan at its
// top left. Arena's gather there under a "Revealed Cards" pill, the last revealed on top and
// each earlier one's edge showing at its left (`I3Te0m1bBFw 22:08.8`). Geometry alone: the
// pill and the browser the pile opens into are the chrome's.
import type { ObjectView } from "../generated/protocol";
import { revealedName, type Dress, type Name, type Resting } from "./layout.ts";
import type { Place } from "./layout.ts";
import type { Revealed } from "./table.ts";

/** Where a layout rests the pile: its first card's place, how far each later card lies to
 * the right of the one under it, how far the last may lie from the first, the depth the
 * first is drawn at, and how its cards are dressed. */
export interface PileSpace extends Place {
  step: number;
  reach: number;
  depth: number;
  dress: Pick<Dress, "look" | "frame">;
}

/** The faces of `revealed` the player has not `cleared`, in the order they lie. */
export const lying = (
  revealed: Revealed | undefined,
  cleared?: ReadonlySet<ObjectView>,
): ObjectView[] =>
  (revealed?.cards ?? []).filter((object) => !cleared?.has(object));

/** Rests in `rest` the faces of `revealed` that are not `cleared`, fanned from `space`:
 * upright and face up, the steps between them closing up where there are many. */
export function fan(
  rest: Map<Name, Resting>,
  revealed: Revealed | undefined,
  space: PileSpace,
  cleared?: ReadonlySet<ObjectView>,
) {
  const cards = lying(revealed, cleared);
  const step = Math.min(
    space.step,
    space.reach / Math.max(1, cards.length - 1),
  );
  cards.forEach((object, i) => {
    rest.set(revealedName(object.slot), {
      object,
      pose: {
        x: space.x + i * step,
        y: space.y,
        width: space.width,
        rotation: 0,
        skew: 0,
        turn: 0,
        opacity: 1,
        depth: space.depth + i,
        face: "front",
      },
      // A face of the pile is in no zone: its card lies where the table has it.
      dress: { ...space.dress, zone: "nowhere", back: false, rim: false },
    });
  });
}
