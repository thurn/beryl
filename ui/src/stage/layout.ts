// What the stage asks of a layout: where everything on a table rests, and the fixed places
// motion runs between. The stage knows no geometry of its own (plan K25); the desktop's layout
// is desktop.ts, and a phone's is its own.
import type {
  ObjectView,
  PlayerView,
  Side,
  StackItem,
  ZoneName,
} from "../generated/protocol";
import type { Look } from "../game/placement";
import type { Pose } from "./pose.ts";
import type { Table } from "./table.ts";

/** What names a node, as scripts name what a step is about: a card or token by its slot, an
 * ability on the stack by the number its `emerge` gave it (script/mod.rs, Names). */
export type Name = number | `a${number}`;

/** The name of the ability a script numbered `id`. */
export const abilityName = (id: number): Name => `a${id}`;

/** An ability on the stack. */
export type StackAbility = Extract<StackItem, { kind: "ability" }>;

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

/** One thing at rest: where it lies, how it is dressed, and what its face is drawn from. A
 * card's is its record, as drawn (an emblem is drawn as a card of its own); an ability's is
 * its stack item and the record of its source, while the table has one. */
export type Resting = { pose: Pose; dress: Dress } & (
  | { object: ObjectView; ability?: undefined; source?: undefined }
  | { ability: StackAbility; source?: ObjectView; object?: undefined }
);

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
  /** Everything the table draws, by name: its objects, and the abilities on its stack. */
  rest(table: Table): Map<Name, Resting>;
  anchors(table: PlayerView): Anchors;
  /** Where an ability starts as it lifts out of its source, which is drawn at `source`: a
   * small copy on it, which then travels to `rest`, its place on the stack. */
  emerging(source: Pose, rest: Pose): Pose;
}
