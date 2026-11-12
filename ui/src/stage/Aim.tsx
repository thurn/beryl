// What a prompt pulls toward the pointer, of two kinds. A declaration's line (UI Conventions,
// "Attacking a planeswalker": "an arrow follows the cursor to the planeswalker"): while
// creatures the player has declared await what they are aimed at, a line runs from each to
// where the mouse is, in the look of the line the aim will draw. And a choice of targets'
// arrow (UI Conventions, "An arrow to the pointer"; Arena's ends on the creature under the
// cursor as its prompt opens, `IEXcjBn3BXk 23:13.17`): out of the spell or ability that asks,
// to the mouse, which becomes the arrow the choice will draw as the mouse comes onto
// something that may be chosen. The click is the session's, whose reply draws the line or
// arrow as one of the stage's own (prompt.ts). These are the pointer's trace and no part of
// the prompt's state: drawn in the stage's layer, and gone with the click or as the pointer
// leaves the table. A touch has no pointer to follow.
//
// Where the stage's lines are painted as light the trace is too (dom.ts `aim`), and as the
// pointer moves, not as React next renders: it is the one thing on the table that follows a
// hand. A browser with no WebGL draws a declaration's line as a path, and no arrow to the
// pointer for a choice of targets: the stage's own arrow still follows the click.
import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import type { DomSurface } from "./dom.ts";
import type { On } from "./effects.ts";
import type { Name, Point } from "./layout.ts";
import { aimed, dark, sought } from "./light.ts";
import type { Asked } from "./prompt.ts";
import { namedBy } from "./spans.ts";
import type { ScriptStage } from "./stage.ts";

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

/** What of the stage's lies under the window's point `x`, `y`: a node by its name, or a
 * player by their life total. */
function under(x: number, y: number): On | undefined {
  const hit = document
    .elementFromPoint(x, y)
    ?.closest<HTMLElement>("[data-stage], [data-life]");
  const said = hit?.dataset.stage;
  if (said !== undefined)
    return /^\d/.test(said) ? Number(said) : (said as Name);
  const life = hit?.dataset.life;
  return life === "you" || life === "opponent" ? life : undefined;
}

export function Aim({
  asked: { prompt, decision },
  layer,
  surface,
  stage,
}: {
  asked: Asked;
  layer: HTMLElement | null;
  surface: DomSurface;
  stage: ScriptStage;
}) {
  const state = prompt?.state;
  const declares = state?.shape === "declare" ? state : undefined;
  const aiming = declares?.aiming ?? NONE;
  const waits = aiming.length > 0;
  const arrow = declares?.arrow ?? "attack";
  // A choice of targets, and what asks for it, where the stage holds that.
  const source = prompt?.purpose === "target" ? decision?.source : undefined;
  const seeks = typeof source === "number";
  const marks = prompt?.marks;
  const [at, setAt] = useState<Point>();
  /** Where the mouse was last, in the window: a prompt that changes under a still mouse is
   * drawn from there. */
  const mouse = useRef<{ x: number; y: number } | undefined>(undefined);
  useEffect(() => {
    const table = layer?.parentElement;
    if ((!waits && !seeks) || !layer || !table) return;
    // What may be chosen, and whether it is the chooser's own; and what has been.
    const open = new Map<On, boolean>();
    const picked = new Set<On>();
    for (const { on, mark } of seeks ? (marks ?? []) : []) {
      const name = namedBy(on);
      const resting =
        name === "you" || name === "opponent"
          ? undefined
          : stage.shown.resting.get(name);
      const yours =
        name === "you" ||
        (resting?.object ?? resting?.ability)?.controller === "you";
      if (mark === "eligible") open.set(name, yours);
      else picked.add(name);
    }
    let lock: { on: On; since: number } | undefined;
    const drawn = (x: number, y: number) => {
      const frame = layer.getBoundingClientRect();
      const scale = frame.width / layer.offsetWidth || 1;
      const to = { x: (x - frame.left) / scale, y: (y - frame.top) / scale };
      if (!seeks) {
        setAt(to);
        const from = aiming.flatMap((slot) => {
          const node = layer.querySelector(`[data-stage="${slot}"]`);
          return node ? [edge(node, layer, to)] : [];
        });
        surface.aim((now) => ({
          under: { ...dark(now), ribbons: aimed(from, to, arrow) },
          over: dark(now),
        }));
        return;
      }
      const on = under(x, y);
      const friendly = on === undefined ? undefined : open.get(on);
      if (on === undefined || friendly === undefined) lock = undefined;
      else if (lock?.on !== on) lock = { on, since: stage.clock.now };
      // Nothing over a target already chosen: the stage's own arrow is there.
      if (on !== undefined && picked.has(on)) return surface.aim();
      const held = lock;
      surface.aim((now) => {
        const from = stage.reach(source);
        const target = held && stage.reach(held.on);
        const locked =
          held && target
            ? { to: target, friendly: friendly === true, since: held.since }
            : undefined;
        return {
          under: dark(now),
          over: from ? sought(now, from, to, locked) : dark(now),
        };
      });
    };
    const moved = (e: PointerEvent) => {
      if (e.pointerType !== "mouse") return;
      mouse.current = { x: e.clientX, y: e.clientY };
      drawn(e.clientX, e.clientY);
    };
    const left = () => {
      mouse.current = undefined;
      setAt(undefined);
      surface.aim();
    };
    table.addEventListener("pointermove", moved);
    table.addEventListener("pointerleave", left);
    if (mouse.current) drawn(mouse.current.x, mouse.current.y);
    return () => {
      table.removeEventListener("pointermove", moved);
      table.removeEventListener("pointerleave", left);
      setAt(undefined);
      surface.aim();
    };
  }, [waits, seeks, source, marks, layer, surface, stage, aiming, arrow]);
  // The mouse is followed whatever is asked, so that a prompt that opens under it is drawn.
  useEffect(() => {
    const table = layer?.parentElement;
    if (!table) return;
    const moved = (e: PointerEvent) => {
      if (e.pointerType === "mouse")
        mouse.current = { x: e.clientX, y: e.clientY };
    };
    const left = () => {
      mouse.current = undefined;
    };
    table.addEventListener("pointermove", moved);
    table.addEventListener("pointerleave", left);
    return () => {
      table.removeEventListener("pointermove", moved);
      table.removeEventListener("pointerleave", left);
    };
  }, [layer]);
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
