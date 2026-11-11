// The card an optional move asks about, enlarged above the place it came from.
import { useRef } from "react";
import { CARD_ASPECT, Card } from "../components/Card";
import type { ObjectView } from "../generated/protocol";
import { useBorrowedCard } from "./choiceFlight";
import { useFace } from "./faces";
import { STAGE } from "./perspective";

/** Where a risen card stands: the place of the table card it is (its center and top, and
 * its width there), in stage pixels. */
export interface Place {
  x: number;
  y: number;
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
  useBorrowedCard(ref, object.slot);
  const height = RISEN_WIDTH / CARD_ASPECT;
  // Above its place, kept on the stage.
  const left = Math.min(
    Math.max(at.x - RISEN_WIDTH / 2, 16),
    STAGE.width - 16 - RISEN_WIDTH,
  );
  const top = Math.max(at.y - height - 14, 16);
  return (
    <div
      ref={ref}
      className="risen-card"
      data-risen={object.slot}
      style={{ left, top, width: RISEN_WIDTH }}
    >
      <Card card={face} width={RISEN_WIDTH} highlight={highlight} />
    </div>
  );
}
