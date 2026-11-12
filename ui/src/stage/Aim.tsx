// The line a declaration pulls toward the pointer (UI Conventions, "Attacking a planeswalker":
// "an arrow follows the cursor to the planeswalker"). While creatures the player has declared
// await what they are aimed at, a line runs from each to where the mouse is, in the look of
// the line the aim will draw; the click that aims them is the session's, whose reply draws
// that line as one of the stage's own (prompt.ts). This one is the pointer's trace and no
// part of the prompt's state: it is drawn in the stage's layer, among its lines, and goes
// with the click or as the pointer leaves the table. A touch has no pointer to follow.
// Where the stage's lines are painted as light the trace is too (dom.ts `aim`), from the
// edge of each creature that faces the pointer, and as the pointer moves, not as React next
// renders: it is the one line on the table that follows a hand.
import { useEffect, useState } from "react";
import { createPortal } from "react-dom";
import type { DomSurface } from "./dom.ts";
import type { Point } from "./layout.ts";
import { aimed } from "./light.ts";
import type { Asked } from "./prompt.ts";

const NONE: readonly number[] = [];

/** The middle of `node` in the coordinates of `layer`, whatever the layer is scaled by. */
function middle(node: Element, layer: HTMLElement): Point {
  const [box, frame] = [
    node.getBoundingClientRect(),
    layer.getBoundingClientRect(),
  ];
  const scale = frame.width / layer.offsetWidth || 1;
  return {
    x: (box.left + box.width / 2 - frame.left) / scale,
    y: (box.top + box.height / 2 - frame.top) / scale,
  };
}

/** Where the way from the middle of `node` toward `to` leaves its box, in the coordinates of
 * `layer`. */
function edge(node: Element, layer: HTMLElement, to: Point): Point {
  const [box, frame] = [
    node.getBoundingClientRect(),
    layer.getBoundingClientRect(),
  ];
  const scale = frame.width / layer.offsetWidth || 1;
  const from = middle(node, layer);
  const [x, y] = [to.x - from.x, to.y - from.y];
  const out = Math.max(
    Math.abs(x) / (box.width / 2 / scale || 1),
    Math.abs(y) / (box.height / 2 / scale || 1),
  );
  return out > 1 ? { x: from.x + x / out, y: from.y + y / out } : from;
}

export function Aim({
  asked: { prompt },
  layer,
  surface,
}: {
  asked: Asked;
  layer: HTMLElement | null;
  surface: DomSurface;
}) {
  const state = prompt?.state;
  const declares = state?.shape === "declare" ? state : undefined;
  const aiming = declares?.aiming ?? NONE;
  const waits = aiming.length > 0;
  const arrow = declares?.arrow ?? "attack";
  const [at, setAt] = useState<Point>();
  useEffect(() => {
    const table = layer?.parentElement;
    if (!waits || !layer || !table) return;
    const moved = (e: PointerEvent) => {
      if (e.pointerType !== "mouse") return;
      const frame = layer.getBoundingClientRect();
      const scale = frame.width / layer.offsetWidth || 1;
      const to = {
        x: (e.clientX - frame.left) / scale,
        y: (e.clientY - frame.top) / scale,
      };
      setAt(to);
      const from = aiming.flatMap((slot) => {
        const node = layer.querySelector(`[data-stage="${slot}"]`);
        return node ? [edge(node, layer, to)] : [];
      });
      surface.aim(aimed(from, to, arrow));
    };
    const left = () => {
      setAt(undefined);
      surface.aim([]);
    };
    table.addEventListener("pointermove", moved);
    table.addEventListener("pointerleave", left);
    return () => {
      table.removeEventListener("pointermove", moved);
      table.removeEventListener("pointerleave", left);
      left();
    };
  }, [waits, layer, surface, aiming, arrow]);
  if (!declares || !waits || !at || !layer) return null;
  return createPortal(
    // Under the cards, as the line the aim draws is.
    <svg className="stage-lines stage-lines-under" aria-hidden>
      {aiming.map((slot) => {
        const node = layer.querySelector(`[data-stage="${slot}"]`);
        if (!node) return null;
        const from = middle(node, layer);
        return (
          <path
            key={slot}
            className={`stage-line stage-line-${declares.arrow}`}
            d={`M ${from.x} ${from.y} L ${at.x} ${at.y}`}
          />
        );
      })}
    </svg>,
    layer,
  );
}
