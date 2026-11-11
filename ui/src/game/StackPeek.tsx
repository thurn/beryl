// The stack's element ids and the strips its lower items peek out by, which arrows are tied
// to: the same on the old board and the stage's.
import type { StackItem as StackItemView } from "../generated/protocol";
import { cardId } from "./CardLayer";
import { stackPlace } from "./stackLayout";

/** The element id of the ability on the stack at `index`. */
export const stackId = (index: number) => `stack-${index}`;

/** The element id of the stack item at `index`: its card for a spell. */
export const itemId = (item: StackItemView, index: number) =>
  item.kind === "spell" ? cardId(item.slot) : stackId(index);

/** The element id of the strip of the stack item at `index` that peeks out from under the
 * next. */
export const peekId = (index: number) => `stack-peek-${index}`;

/** The strip of the stack item at `index` of `count` that the next one leaves uncovered: its
 * name bar (STACK), which is all of it the eye can tie an arrow to. Unseen, part of the item's
 * element `within` for the arrows. */
export function StackPeek({
  index,
  count,
  within,
  nextItem,
}: {
  index: number;
  count: number;
  within: string;
  nextItem: string;
}) {
  const place = stackPlace(index, count);
  const next = stackPlace(index + 1, count);
  return (
    <div
      id={peekId(index)}
      className="hud stack-peek"
      data-within={within}
      data-stack-next={nextItem}
      style={{
        left: place.x,
        top: place.y,
        width: place.width,
        height: next.y - place.y,
      }}
    />
  );
}
