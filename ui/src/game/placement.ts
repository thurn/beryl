// What a placed card is: its look, its Placement, its footprint on the stage, and the
// helpers that turn a spot on the table into a Placement's position.
import type { Side, ZoneName } from "../generated/protocol";
import { CARD_ASPECT } from "../components/Card";
import { depthScale, lean } from "./perspective";

export const CREATURE_WIDTH = 128;
export const LAND_WIDTH = 100;
/** Each copy beneath a land stack's top card, as a fraction of the card's width. */
export const LAND_STEP = { x: 0.103, y: 0.022 };
/** How far each card a permanent holds in exile peeks out from behind it, to the left and toward
 * its controller (down for yours, up for the opponent's), as a fraction of the permanent's
 * untapped width and height: what it exiled until it leaves (CR 610.3) and what its abilities
 * find "exiled with" it (CR 607.2a), mirrored across the table. A click on one opens them all.
 * Each Aura or Equipment attached to a permanent peeks out from behind it as far, up and to the
 * right on either side, so its name bar always shows. */
export const BENEATH_STEP = { x: 0.3, y: 0.17 };

/** The hand's cards: each card's top center lies on an arc over the near edge, and the fan
 * turns each a little more clockwise than the one on its left. */
export const HAND = {
  width: 182,
  spacing: 172,
  top: 726.5,
  sag: 6,
  turn: 4,
  bias: 0.6,
};

/** How the hand card the pointer is over rises out of the fan: straightened, lifted and a
 * little enlarged about its top center. The card's node
 * stays where the fan puts it (stage/raised.ts), so hovering moves nothing else and the pointer keeps the card.
 * A card rises less where it would reach over your lands (`Placement.lift`). */
export const RAISE = { lift: 70, scale: 1.06 };

/** How a placed card is drawn. */
export type Look =
  | "hand" // a full frame in your hand
  | "held" // the opponent's hand: a back, or the face of a card you know
  | "field" // a battlefield frame: nonland permanents
  | "land" // a battlefield frame in a land pile
  | "beneath" // a battlefield frame tucked behind the permanent holding it in exile
  | "attached" // a battlefield frame tucked behind the permanent it is attached to
  | "stack" // a full frame on the stack
  | "pile" // a full frame on a graveyard or exile pile, which the pile's button covers
  | "shown" // a full frame face up over the table, on its way to a pile (SHOWCASE)
  | "handling" // cards lifted while the opponent handles them
  | "hidden"; // fully occluded by the rest of a pile

/** One card on the stage: its element's top-left corner at (x, y), then scaled, turned and
 * sheared about that corner, in that order (Motion's transform order). */
export interface Placement {
  slot: number;
  zone: ZoneName;
  look: Look;
  /** The frame's width before scaling. */
  width: number;
  /** The width it takes on the table before scaling: a tapped frame's turned height. */
  span: number;
  x: number;
  y: number;
  scale: number;
  rotate: number;
  skew: number;
  opacity: number;
  z: number;
  /** Under the top card, only the physical rim is painted when the card is at rest. */
  rim?: boolean;
  /** Face down in this library, unless it is the disclosed top card. */
  back?: boolean;
  librarySide?: Side;
  /** A local tray owns the projected face, even if the card comes from under the top. */
  lendingFace?: boolean;
  /** On the top card of a pile of identical permanents, how many it holds. */
  count?: number;
  /** On each card of a pile of identical permanents, the pile's slots, top first: what a choice
   * takes from the pile it takes from the top. */
  pile?: number[];
  /** On a hand card, how far it rises when the pointer is over it (RAISE). */
  lift?: number;
}

/** A whole board: every object's placement, by slot. */
export type Layout = Map<number, Placement>;

/** An upright rectangle on the stage, in stage pixels. */
export interface Box {
  left: number;
  top: number;
  right: number;
  bottom: number;
}

/** The box a placed card covers, its turn aside: as wide as it takes (`span`) and, a tapped
 * battlefield card, as tall as its frame is wide (lying); any other a whole card. Its lean
 * shears its near end sideways (Motion's skewX about its top-left corner). */
export function footprint(p: Placement): Box {
  const field =
    p.look === "field" ||
    p.look === "land" ||
    p.look === "beneath" ||
    p.look === "attached";
  const lies = field && p.span > p.width;
  const height = (lies ? p.width : p.width / CARD_ASPECT) * p.scale;
  const shear = Math.tan((p.skew * Math.PI) / 180) * height;
  const right = p.x + p.span * p.scale;
  return {
    left: Math.min(p.x, p.x + shear),
    top: p.y,
    right: Math.max(right, right + shear),
    bottom: p.y + height,
  };
}

/** The hand's full frames, and the width of everything drawn from a full frame. */
export const FULL_WIDTH = HAND.width;
export const BACK_WIDTH = 105;

/** Places a thing `width` wide (before its `scale`, its depth's by default) lying on the table
 * at (x, y), or reaching `rise` above it (before scaling) while scaled and leaning as if it lay
 * there. */
export function onTable(
  x: number,
  y: number,
  width: number,
  rise = 0,
  scale = depthScale(y),
) {
  return {
    x,
    y: y - rise * scale,
    scale,
    rotate: 0,
    skew: lean(x + (width * scale) / 2, y),
  };
}

/** Places an upright element `width`×`height` whose top-left corner would be (left, top),
 * turned `rotate` degrees about the point (cx, cy) of its own box: the corner moves so that
 * turning about the element's corner, as placements do, lands it where turning about that
 * point would. */
export function turned(
  left: number,
  top: number,
  rotate: number,
  [cx, cy]: [number, number],
  translateY = 0,
) {
  const a = (rotate * Math.PI) / 180;
  const [cos, sin] = [Math.cos(a), Math.sin(a)];
  // c - R c + R (0, translateY)
  return {
    x: left + cx - (cos * cx - sin * cy) - sin * translateY,
    y: top + cy - (sin * cx + cos * cy) + cos * translateY,
    scale: 1,
    rotate,
    skew: 0,
  };
}
