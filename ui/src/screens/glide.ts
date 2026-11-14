// Cards of the opening that change place on a click glide from where they were to where they
// are: a card picked for the bottom of the library goes down from the hand into its slot, one
// clicked again goes back up, and the cards left in the hand slide to their new places.
import { useLayoutEffect, useRef } from "react";

/** How long a card takes between the hand and the slots below it. Arena's card, let go over
 * the library, settles into it in about 0.2 s, slowing as it arrives (2VW6ZzmS8U4 8:05.47 to
 * 8:05.67); Beryl's goes about twice as far, from the hand down to its slot. */
const CROSS_MS = 250;
/** How long the cards that stay in the hand take to their new places, as a hand on the table
 * closes up: about a sixth of a second, slowing into them. */
const SHIFT_MS = 170;
const EASE_OUT = "cubic-bezier(0.2, 0.7, 0.3, 1)";

/** Where a card is on the screen: its middle, its width before any transform, and how far
 * the box around it is turned (a card in the desktop's fan leans). */
type Place = {
  x: number;
  y: number;
  width: number;
  height: number;
  turn: number;
};

function placeOf(el: HTMLElement): Place {
  const box = el.getBoundingClientRect();
  const parent = el.parentElement;
  const matrix = parent
    ? new DOMMatrix(getComputedStyle(parent).transform)
    : new DOMMatrix();
  return {
    x: box.left + box.width / 2,
    y: box.top + box.height / 2,
    width: el.offsetWidth,
    height: el.offsetHeight,
    turn: Math.atan2(matrix.b, matrix.a),
  };
}

/** Tracks the opening's cards by slot. `track(slot)` is the ref for a card's element, wherever
 * it lies; call `before()` just before a change of state that moves cards, and each tracked
 * card that ends up somewhere else glides there from where it was. */
export function useGlide() {
  const nodes = useRef(new Map<number, HTMLElement>());
  const refs = useRef(new Map<number, (el: HTMLElement | null) => void>());
  const pending = useRef<Map<number, Place>>(undefined);

  useLayoutEffect(() => {
    const was = pending.current;
    if (!was) return;
    pending.current = undefined;
    if (matchMedia("(prefers-reduced-motion: reduce)").matches) return;
    for (const [slot, el] of nodes.current) {
      const from = was.get(slot);
      if (!from) continue;
      for (const a of el.getAnimations()) a.cancel();
      const to = placeOf(el);
      const [dx, dy] = [from.x - to.x, from.y - to.y];
      const scale = from.width / to.width;
      const turn = from.turn - to.turn;
      if (
        Math.abs(dx) < 0.5 &&
        Math.abs(dy) < 0.5 &&
        Math.abs(scale - 1) < 0.01 &&
        Math.abs(turn) < 0.002
      )
        continue;
      // The element's transform is in its parent's frame, which may lean.
      const [c, s] = [Math.cos(-to.turn), Math.sin(-to.turn)];
      const [x, y] = [dx * c - dy * s, dx * s + dy * c];
      // A card that changes rows passes over the others.
      const crossing = Math.abs(dy) > to.height / 2;
      const lifted = crossing ? (el.parentElement ?? el) : undefined;
      if (lifted) lifted.style.zIndex = "1";
      const glide = el.animate(
        [
          {
            transform: `translate(${x}px, ${y}px) rotate(${turn}rad) scale(${scale})`,
          },
          { transform: "none" },
        ],
        { duration: crossing ? CROSS_MS : SHIFT_MS, easing: EASE_OUT },
      );
      if (lifted) {
        const settle = () => {
          lifted.style.zIndex = "";
        };
        glide.onfinish = settle;
        glide.oncancel = settle;
      }
    }
  });

  return {
    track(slot: number) {
      let ref = refs.current.get(slot);
      if (!ref) {
        // React detaches a moved card's old element before it attaches the new one.
        ref = (el) => {
          if (el) nodes.current.set(slot, el);
          else nodes.current.delete(slot);
        };
        refs.current.set(slot, ref);
      }
      return ref;
    },
    before() {
      pending.current = new Map(
        [...nodes.current].map(([slot, el]) => [slot, placeOf(el)]),
      );
    },
  };
}
