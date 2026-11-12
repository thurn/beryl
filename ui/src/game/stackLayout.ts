// Where the stack's items and a showcase's cards are drawn.
import { CARD_ASPECT } from "../components/Card";
import { fromRight } from "./perspective";

/** The stack, spells and abilities alike, large at the right of the table as in
 * docs/ui_mockups/trigger_on_stack_with_target.png: the top item in front where the mockup
 * draws its trigger, each item beneath it peeking out above by its name bar and to its left by
 * a strip (`lean`), closer together the taller the stack. Arena shows a third of the card
 * beneath beside the one over it, and a countered spell crumbles there (UI Conventions, "Two
 * items on the stack", "A countered spell"). Beryl's strip is a quarter of the card up to
 * five items, so a spell leaving from under another is seen as it goes, and the bottom item
 * of the tallest stack still stands right of the permanents' rows (`ROOM.end`). It stands
 * between the two back rows, whose other permanents reach under its column (`ROWS`), so
 * neither side's are covered by it. `x` and `width` bound it; `foot` is
 * the top item's foot. As wide as keeps a resolving permanent's flight to its slot within
 * Arena's shrink (data/arena-motion.json, fly's "permanent resolves": the card on the stack is
 * 2.3 to 2.8 times its size at rest), the smaller of a non-creature in the back row of a
 * crowded table included. */
export const STACK = {
  // Held to the stage's right edge.
  get x() {
    return fromRight(1284);
  },
  foot: 566,
  width: 204,
  step: 40,
  reach: 170,
  // How far each item beneath lies to the left, for each pixel it peeks out above.
  lean: 1.2,
};

/** How high the stack may reach: below the opponent's back row at its full size. */
const STACK_TOP = 234;

/** Where the stack item at `index` (bottom first) of `count` is drawn. The top item's foot stays
 * put, clear of your back row and of the turn controls and the prompts over them; a stack too
 * tall to fit under the opponent's back row shrinks, all its items alike, about its column's
 * middle. */
export function stackPlace(index: number, count: number) {
  const step = count > 1 ? Math.min(STACK.step, STACK.reach / (count - 1)) : 0;
  const height = STACK.width / CARD_ASPECT;
  const foot = STACK.foot;
  const scale = Math.min(1, (foot - STACK_TOP) / (height + (count - 1) * step));
  const width = STACK.width * scale;
  return {
    x:
      STACK.x +
      (STACK.width - width) / 2 -
      (count - 1 - index) * step * scale * STACK.lean,
    y: foot - (height + (count - 1 - index) * step) * scale,
    width,
  };
}
