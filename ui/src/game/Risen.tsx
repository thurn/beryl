// Cards a question raises from the table: each grows out of where its card lies, so the
// player sees which card it is and where it came from. `useRiseFrom` springs an element
// out of the table card in a slot as it appears; `Risen` draws a card enlarged above its own
// place, as a "you may" move's card above the graveyard it lies in.
import { useContext, useLayoutEffect, useRef, type RefObject } from "react";
import { animate, motion, useReducedMotion } from "motion/react";
import { CARD_ASPECT, Card } from "../components/Card";
import type { ObjectView } from "../generated/protocol";
import { useFace } from "./faces";
import { StageScale } from "./Stage";
import { STAGE } from "./layout";

const SPRING = { type: "spring", stiffness: 300, damping: 28 } as const;

/** The table's card in one of `slots`, the first one drawn: what a card off the table grows
 * out of (the card itself, or the permanent it lies exiled beneath). */
function tableCard(slots: (number | null | undefined)[]): Element | undefined {
  for (const slot of slots) {
    if (slot == null) continue;
    const e = document.querySelector(`.card-layer [data-slot="${slot}"]`);
    if (e && e.getBoundingClientRect().width > 0) return e;
  }
  return undefined;
}

/** Springs the element `ref` holds out of the table card in the first of `slots` drawn, as
 * it mounts: from that card's place and size to its own. */
export function useRiseFrom(
  ref: RefObject<HTMLElement | null>,
  slots: (number | null | undefined)[],
) {
  const scale = useContext(StageScale);
  const reduce = useReducedMotion() ?? false;
  const key = slots.join(",");
  useLayoutEffect(() => {
    const e = ref.current;
    const from = tableCard(slots);
    if (!e || !from || reduce) return;
    const a = from.getBoundingClientRect();
    const b = e.getBoundingClientRect();
    if (b.width === 0) return;
    // Screen pixels to stage pixels, the transform's units inside the scaled stage.
    const dx = (a.left + a.width / 2 - (b.left + b.width / 2)) / scale;
    const dy = (a.top + a.height / 2 - (b.top + b.height / 2)) / scale;
    const s = Math.max(0.15, a.width / b.width);
    const run = animate(
      e,
      { x: [dx, 0], y: [dy, 0], scale: [s, 1], opacity: [0.5, 1] },
      SPRING,
    );
    return () => run.stop();
    // Once, as it mounts, for the slots it rises from.
  }, [key]);
}

/** Where a risen card stands: the place of the table card it is (its center and top, and
 * its width there), in stage pixels. */
export interface Place {
  x: number;
  y: number;
  width: number;
}

/** The width a risen card is drawn at. */
export const RISEN_WIDTH = 230;

/** A card shown large above its own place on the table, grown out of it. */
export function Risen({
  object,
  at,
  highlight,
}: {
  object: ObjectView;
  at: Place;
  highlight?: "source" | "target";
}) {
  const ref = useRef<HTMLDivElement>(null);
  const face = useFace(object);
  const reduce = useReducedMotion() ?? false;
  const height = RISEN_WIDTH / CARD_ASPECT;
  // Above its place, kept on the stage.
  const left = Math.min(
    Math.max(at.x - RISEN_WIDTH / 2, 16),
    STAGE.width - 16 - RISEN_WIDTH,
  );
  const top = Math.max(at.y - height - 14, 16);
  const scale = at.width / RISEN_WIDTH;
  return (
    <motion.div
      ref={ref}
      className="risen-card"
      data-risen={object.slot}
      style={{ left, top, width: RISEN_WIDTH, transformOrigin: "50% 100%" }}
      initial={
        reduce
          ? false
          : {
              y: height + 14,
              x: at.x - (left + RISEN_WIDTH / 2),
              scale,
              opacity: 0.5,
            }
      }
      animate={{ x: 0, y: 0, scale: 1, opacity: 1 }}
      transition={SPRING}
    >
      <Card card={face} width={RISEN_WIDTH} highlight={highlight} />
    </motion.div>
  );
}
