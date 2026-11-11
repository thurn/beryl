// What the stage asks of a layout: where everything on a table rests, and the fixed places
// motion runs between. The stage knows no geometry of its own (plan K25); the desktop's layout
// is desktop.ts, and a phone's is its own.
import type {
  ObjectView,
  PlayerView,
  Side,
  ZoneName,
} from "../generated/protocol";
import type { Look } from "../game/layout";
import type { Pose } from "./pose.ts";

/** How an object's node is dressed where it rests: what its face is drawn from, besides its
 * record. A step that changes none of it leaves the face alone. */
export interface Dress {
  zone: ZoneName;
  look: Look;
  /** The frame's own width, which its pose's width scales. */
  frame: number;
  /** Drawn as a card back whatever the record knows. */
  back: boolean;
  /** Only its paper edge shows, under the top of a pile. */
  rim: boolean;
  /** On the top card of a pile of identical permanents, how many it holds. */
  count?: number;
}

/** One object at rest: its record, as drawn (an emblem is drawn as a card of its own), where
 * it lies and how it is dressed. */
export interface Resting {
  object: ObjectView;
  pose: Pose;
  dress: Dress;
}

/** A point on the stage. */
export interface Point {
  x: number;
  y: number;
}

/** A place a card can be drawn at: its top-left corner and its width there. */
export interface Place extends Point {
  width: number;
}

/** The places that are not an object's own: where motion starts from and ends at. */
export interface Anchors {
  /** The top card of a pile. */
  pile(side: Side, zone: "library" | "graveyard" | "exile"): Place;
  /** The stack's item at `index`, bottom first, of `count`. */
  stack(index: number, count: number): Place;
  /** The corner of a player's life orb, and its middle. */
  life(side: Side): { corner: Point; middle: Point };
  /** The corner of a player's unspent mana. */
  pool(side: Side): Point;
}

export interface StageLayout {
  /** Every object the table draws, by slot. */
  rest(table: PlayerView): Map<number, Resting>;
  anchors(table: PlayerView): Anchors;
}
