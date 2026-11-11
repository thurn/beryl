// Where the stack's items and a showcase's cards are drawn.
import type { Side } from "../generated/protocol";
import { CARD_ASPECT } from "../components/Card";
import { STAGE, fromRight } from "./perspective";

/** The stack, spells and abilities alike, large at the right of the table as in
 * docs/ui_mockups/trigger_on_stack_with_target.png: the top item in front where the mockup
 * draws its trigger, each item beneath it peeking out above by its name bar, closer together
 * the taller the stack. It stands between the two back rows, whose other permanents reach under
 * its column (`ROWS`), so neither side's are covered by it. `x` and `width` bound it; `foot` is
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
      (count - 1 - index) * step * scale * 0.4,
    y: foot - (height + (count - 1 - index) * step) * scale,
    width,
  };
}

/** A showcase: cards up to `width` wide, `gap` apart, in a row no wider than `reach`, held
 * over the pile they are going to (or the permanent that holds them) and kept `margin` inside
 * the stage, so the eye follows them from where they came to where they land. */
export const SHOWCASE = {
  width: 220,
  reach: 900,
  gap: 18,
  margin: 24,
};

/** Cards a beat of the game shows apart from where they lie (beats.ts): those `shown` face up
 * in a row over the table on their way to a pile, and those the opponent just `drawn`, dipping
 * out of their hand at the table's edge, where it would otherwise hide them, before they
 * settle in it. */
export interface Spotlight {
  shown?: ReadonlySet<number>;
  drawn?: ReadonlySet<number>;
  /** Libraries lifted into two face-down packets during a shuffle. */
  shuffle?: ReadonlySet<Side>;
  /** Which cards are still in flight to the two ends of a scrying player's library. */
  scryReturn?: {
    side: Side;
    top: ReadonlySet<number>;
    bottom: ReadonlySet<number>;
  };
  /** Mana reaches this permanent while its ability is still being paid for. */
  payingSource?: number;
}

/** How far below the opponent's hand a card they just drew dips into view. */
export const DRAWN_DIP = 120;

/** Where the card at `index` of `count` in a showcase over the point `over` is drawn: the row
 * centered on it, as far as the stage's edges allow. */
export function showcasePlace(
  index: number,
  count: number,
  over: { x: number; y: number },
) {
  const width = Math.min(
    SHOWCASE.width,
    (SHOWCASE.reach - SHOWCASE.gap * (count - 1)) / count,
  );
  const height = width / CARD_ASPECT;
  const row = count * width + (count - 1) * SHOWCASE.gap;
  const within = (at: number, size: number, stage: number) =>
    Math.min(
      Math.max(at - size / 2, SHOWCASE.margin),
      stage - SHOWCASE.margin - size,
    );
  return {
    x: within(over.x, row, STAGE.width) + index * (width + SHOWCASE.gap),
    y: within(over.y, height, STAGE.height),
    width,
  };
}
