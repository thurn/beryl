// What the stage asks of a layout: where everything on a table rests, and the fixed places
// motion runs between. The stage knows no geometry of its own (plan K25); the desktop's layout
// is desktop.ts, and a phone's is its own.
import type {
  ObjectView,
  PaidCost,
  PlayerView,
  Side,
  StackItem,
  ZoneName,
} from "../generated/protocol";
import type { Look } from "../game/placement";
import type { Pose } from "./pose.ts";
import type { Table } from "./table.ts";

/** What names a node, as scripts name what a step is about: a card or token by its slot, an
 * ability on the stack by the number its `emerge` gave it (script/mod.rs, Names). One name is
 * the stage's own: the face of a card held up to be read, in the Revealed Cards pile, by the
 * slot of the card it is the face of (table.ts `Revealed`). */
export type Name = number | `a${number}` | `r${number}`;

/** The name of the ability a script numbered `id`. */
export const abilityName = (id: number): Name => `a${id}`;

/** The name of the face the Revealed Cards pile holds of the card in `slot`. */
export const revealedName = (slot: number): Name => `r${slot}`;

/** Whether `name` is of a face in the Revealed Cards pile. */
export const isRevealed = (name: Name): name is `r${number}` =>
  typeof name === "string" && name.startsWith("r");

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
  /** How far a card of your hand rises under the pointer, where the layout raises it and by
   * less than it would with nothing over it (game/placement.ts `RAISE`). */
  lift?: number;
  /** On a spell on the stack, what its card does not say of how it was cast: the X it was
   * announced with and the alternative or additional costs paid for it. */
  cast?: { x: number; paid: PaidCost[] };
}

/** One thing at rest: where it lies, how it is dressed, and what its face is drawn from. A
 * card's is its record, as drawn (an emblem is drawn as a card of its own); an ability's is
 * its stack item and the record of its source, while the table has one. */
export type Resting = { pose: Pose; dress: Dress } & (
  | { object: ObjectView; ability?: undefined; source?: undefined }
  | { ability: StackAbility; source?: ObjectView; object?: undefined }
);

/** What a resting thing's face is drawn from. */
const faceOf = ({ object, ability, source }: Resting) =>
  object ? { object } : { ability, source };

/** Whether two resting things have one face. */
export const sameFace = (a: Resting, b: Resting) =>
  JSON.stringify(faceOf(a)) === JSON.stringify(faceOf(b));

/** Whether two nodes are dressed alike. */
export const sameDress = (a: Dress, b: Dress) =>
  a.zone === b.zone &&
  a.look === b.look &&
  a.frame === b.frame &&
  a.back === b.back &&
  a.rim === b.rim &&
  a.count === b.count &&
  a.lift === b.lift &&
  JSON.stringify(a.cast) === JSON.stringify(b.cast);

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
  /** Everything the table draws, by name: its objects, and the abilities on its stack. What
   * is `leaving` the table has where it goes, but it is still seen where it was (a `vanish`
   * under way): nothing else rests as if it had arrived, so a pile it goes to shows the top
   * it had. */
  rest(table: Table, leaving?: ReadonlySet<Name>): Map<Name, Resting>;
  anchors(table: PlayerView): Anchors;
  /** Where an ability starts as it lifts out of its source, which is drawn at `source`: a
   * small copy on it, which then travels to `rest`, its place on the stack. */
  emerging(source: Pose, rest: Pose): Pose;
  /** Where a card you draw is held up to be read before it drops into your hand, where it
   * will lie at `rest`; one of several drawn together passes it without stopping. */
  drawn(rest: Pose): Pose;
  /** The middle of the card `resting` is dressed as, were it drawn at `pose`: what a card
   * turns about as it taps, grows about as it turns over, and strikes at. */
  middle(resting: Resting, pose: Pose): Point;
  /** Half the width and half the height of the card `resting` is dressed as, as it lies at
   * `pose`: how far it reaches from its middle, which a strike at it stops by. */
  half(resting: Resting, pose: Pose): Point;
  /** How far past its frame the tip of the chevron an attacker wears is, in its card's
   * widths, where the line to what it attacks starts: nothing in a layout whose attackers
   * wear none. */
  chevron: number;
}
