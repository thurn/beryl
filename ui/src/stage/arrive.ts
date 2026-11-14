// The opening hand the player kept drops from where the opening screen showed it into the
// hand on the table, as Arena's does: the screen over the table vanishes, and the hand, still
// fanned in the middle of the screen, falls into place at the near edge in about an eighth of
// a second, slowing as it lands (3SEgIYXJgdQ 13:33.95, 2VW6ZzmS8U4 6:56.25 and 14:07.75).
// The opening screen (screens/Opening.tsx) notes where each of its cards was as it closes,
// and the board starts each card of its hand there (`ScriptStage.arrive`).
import { type RefObject, useLayoutEffect, useRef } from "react";
import { easeOut } from "motion";
import { useReducedMotion } from "motion/react";
import { CARD_ASPECT } from "../components/Card";
import type { Name } from "./layout.ts";
import { ALOFT, type Pose } from "./pose.ts";
import type { ScriptStage } from "./stage.ts";
import { SNAP, type Timing } from "./tracks.ts";

/** How the kept hand drops from the opening screen into place: Arena's is in the hand four
 * frames of 30 after the screen goes, most of the way there after two. */
export const KEPT: Timing = {
  duration: 130,
  delay: 0,
  stagger: 0,
  ease: easeOut,
};

/** One card of the opening screen's hand as it closed, on the page: its middle, how wide it
 * was drawn and how far it was turned clockwise, in degrees. */
interface Noted {
  slot: number;
  x: number;
  y: number;
  width: number;
  rotation: number;
}

let noted: Noted[] = [];

/** How far `el` is turned clockwise on the page, in degrees, by its own transform and its
 * ancestors' (the desktop opening turns each card's holder to fan the hand). */
function rotationOf(el: Element | null): number {
  let degrees = 0;
  for (; el; el = el.parentElement) {
    const transform = getComputedStyle(el).transform;
    if (!transform || transform === "none") continue;
    const m = new DOMMatrixReadOnly(transform);
    degrees += (Math.atan2(m.b, m.a) * 180) / Math.PI;
  }
  return degrees;
}

/** Notes where each card of the opening's hand under `root` is on the page, as the opening
 * closes: where the kept hand starts its drop. */
export function noteOpeningHand(root: Element) {
  noted = [...root.querySelectorAll<HTMLElement>(".opening-hand > [data-slot]")]
    .map((el) => {
      const box = el.getBoundingClientRect();
      const rotation = rotationOf(el);
      const a = (rotation * Math.PI) / 180;
      // The box around a card turned by `a` is w|cos a| + h|sin a| wide, h = w / CARD_ASPECT.
      const width =
        box.width /
        (Math.abs(Math.cos(a)) + Math.abs(Math.sin(a)) / CARD_ASPECT);
      return {
        slot: Number(el.dataset.slot),
        x: box.left + box.width / 2,
        y: box.top + box.height / 2,
        width,
        rotation,
      };
    })
    .filter((card) => card.width > 0);
}

/** Takes what the opening last noted, as the poses on the stage drawn in `layer` that its
 * cards had; nothing if the opening noted nothing since it was last taken. */
export function takeOpeningHand(layer: HTMLElement): Map<Name, Pose> {
  const cards = noted;
  noted = [];
  const box = layer.getBoundingClientRect();
  // The layer is as large as the stage and scaled with it (game/Stage.tsx).
  const scale = box.width / layer.offsetWidth || 1;
  return new Map(
    cards.map((card) => {
      const width = card.width / scale;
      const height = width / CARD_ASPECT;
      const a = (card.rotation * Math.PI) / 180;
      const [cos, sin] = [Math.cos(a), Math.sin(a)];
      // A pose is turned about its top-left corner: the middle less the turned half size.
      const x = (card.x - box.left) / scale - (cos * width - sin * height) / 2;
      const y = (card.y - box.top) / scale - (sin * width + cos * height) / 2;
      const pose: Pose = {
        x,
        y,
        width,
        rotation: card.rotation,
        skew: 0,
        turn: 0,
        opacity: 1,
        depth: ALOFT,
        face: "front",
      };
      return [card.slot, pose];
    }),
  );
}

/** Drops the kept hand from the opening screen into place on the stage drawn in `layer`, in
 * the commit that closes the opening (`opening` turns false). Call it after the board lays the
 * table out for that commit, so that the hand drops to where it now rests. */
export function useKeptHand(
  stage: ScriptStage,
  layer: RefObject<HTMLElement | null>,
  opening: boolean,
) {
  const reduce = useReducedMotion() ?? false;
  const was = useRef(opening);
  useLayoutEffect(() => {
    const closed = was.current && !opening;
    was.current = opening;
    if (!closed || !layer.current) return;
    stage.arrive(takeOpeningHand(layer.current), reduce ? SNAP : KEPT);
  }, [stage, layer, opening, reduce]);
}
