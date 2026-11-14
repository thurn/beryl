// The pointer on the stage: the one way the stage tells what draws around it (React: the
// inspector here, the prompt shapes after it) what the player's pointer did to its nodes. Three
// things are told (`PointerEvents`): the pointer coming onto a node and leaving it, a click on
// one, and a card picked up and let go over another node or a player.
//
// The surface turns the browser's events into the calls below, in stage pixels and by the
// names the stage gives its nodes (dom.ts); nothing here reads the page, so a test makes the
// same calls. What a click or a drop means is not decided here: whoever listens decides, and
// says which cards may be picked up at all (`holdable`).
//
// A card picked up is the one pose the pointer drives and not the clock. It enters the stage's
// model as data (`held`: which node, where the press began and where the pointer is now), and
// the stage computes the node's pose from that as it computes every other (`heldPose`), so
// `ScriptStage.pose` returns it and a step that moves the card starts from where it is held.
import type { Side } from "../generated/protocol";
import type { Name, Point } from "./layout.ts";
import { ALOFT, type Pose } from "./pose.ts";

/** What a held card was let go over: another node, or a player's life total. */
export type Under = { name: Name } | { player: Side };

/** A held card let go. */
export interface Drop {
  name: Name;
  /** Where the press that picked it up began, and where the pointer let go. */
  start: Point;
  at: Point;
  /** The card's pose as it was let go. */
  pose: Pose;
  onto?: Under;
}

/** What the stage tells of the pointer. Each is told at most one listener's answer: what the
 * last to `listen` says. */
export interface PointerEvents {
  /** The pointer came onto the node `name`; with none, it left the one it was on. None is
   * told while a card is held, and a card let go is not told to be under the pointer until
   * it comes to rest (`Pointer.arrived`). */
  hover?(name: Name | undefined): void;
  /** A press let go where it began, on `name`. */
  click?(name: Name, at: Point): void;
  /** A card was picked up, or with none the held one was let go or dropped. */
  lift?(name: Name | undefined): void;
  /** A held card was let go. Returns whether the drop was taken: a taken card stays where it
   * was let go until what the drop did plays, and any other glides back to where it rests. */
  drop?(drop: Drop): boolean | void;
}

/** The card the pointer holds. */
export interface Held {
  name: Name;
  /** Where the press began, and where the pointer is. */
  start: Point;
  at: Point;
}

/** What the stage does as a card is held (stage.ts): each is called after `held` changed. */
export interface Holding {
  /** `name` was picked up. */
  lifted(name: Name): void;
  /** The pointer moved with it. */
  moved(name: Name): void;
  /** It was let go where `pose` has it; `taken` as the drop's listener said. */
  dropped(name: Name, pose: Pose | undefined, taken: boolean): void;
  /** The pose of `name` now, or none if the stage holds no such node. */
  pose(name: Name): Pose | null;
}

/** How far a press travels before it picks its card up, in stage pixels. */
export const DRAG_THRESHOLD = 6;

/** How far over everything, what flies included, a held card is drawn. */
const HELD_DEPTH = 1000;

/** The pose of a card picked up at `from` by a press at `start`, with the pointer now at `at`:
 * upright and unleaned over everything, the point of it that was pressed still under the
 * pointer. A pose turns about its top-left corner (pose.ts), so that corner is found by
 * turning the pressed point back. */
export function heldPose(from: Pose, start: Point, at: Point): Pose {
  const turn = (-from.rotation * Math.PI) / 180;
  const dx = start.x - from.x;
  const dy = start.y - from.y;
  return {
    ...from,
    x: at.x - (dx * Math.cos(turn) - dy * Math.sin(turn)),
    y: at.y - (dx * Math.sin(turn) + dy * Math.cos(turn)),
    rotation: 0,
    skew: 0,
    depth: HELD_DEPTH,
  };
}

export class Pointer {
  /** Which nodes a press may pick up: none until whoever answers drops says. */
  holdable: (name: Name) => boolean = () => false;
  readonly #holding: Holding;
  #listener: PointerEvents = {};
  #over: Name | undefined;
  #press: { name: Name; start: Point; at: Point; moved: boolean } | undefined;
  #held: Held | undefined;
  /** A card on its way that has yet to come to rest: one let go, or one the pointer came
   * onto as it flew, such as a card you draw, which is in your hand while it flies there.
   * Hovering it would raise it out of the fan as it rests there while it is still elsewhere
   * (StageBoard.tsx), turning it upright from the fan's lean although it flies upright, so
   * it is not told to be hovered until it arrives. */
  #settling: Name | undefined;
  /** What `hover` was last told. */
  #told: Name | undefined;

  constructor(holding: Holding) {
    this.#holding = holding;
  }

  /** Tells `listener` what the pointer does from now on, in place of whoever listened; returns
   * how to stop. */
  listen(listener: PointerEvents) {
    this.#listener = listener;
    return () => {
      if (this.#listener === listener) this.#listener = {};
    };
  }

  /** The card the pointer holds, if it holds one. */
  get held(): Held | undefined {
    return this.#held;
  }

  /** The node the pointer is on, as last told. */
  get hovered(): Name | undefined {
    return this.#told;
  }

  /** The card let go that has yet to come to rest, if one has. */
  get settling(): Name | undefined {
    return this.#settling;
  }

  /** The pointer is on the node `name` now, or on none. */
  over(name: Name | undefined) {
    if (name === this.#over) return;
    this.#over = name;
    // Drawn over everything at rest, it is flying (pose.ts `ALOFT`).
    const depth =
      name === undefined ? 0 : (this.#holding.pose(name)?.depth ?? 0);
    if (this.#settling === undefined && depth >= ALOFT) this.#settling = name;
    this.#tell();
  }

  /** Whether the card let go has come to rest now, as `moving` says it is not moving: if
   * it has, it may be hovered from now on (`arrived`). */
  settle(moving: (name: Name) => boolean): boolean {
    const name = this.#settling;
    if (name === undefined || moving(name)) return false;
    this.arrived(name);
    return true;
  }

  /** The card `name`, let go or flown, is at rest: the pointer may hover it now. It moved
   * from under the still pointer, which sends no event, so the pointer is not taken to be on
   * it any more: whoever draws the stage says afresh what the pointer is on (dom.ts
   * `repoint`). */
  arrived(name: Name) {
    if (this.#settling !== name) return;
    this.#settling = undefined;
    if (this.#over === name) this.#over = undefined;
    this.#tell();
  }

  /** Tells the listener what the pointer hovers now, if that changed: nothing while a card is
   * held, nor a card let go that has yet to come to rest. */
  #tell() {
    const over =
      this.#held || this.#over === this.#settling ? undefined : this.#over;
    if (over === this.#told) return;
    this.#told = over;
    this.#listener.hover?.(over);
  }

  /** The nodes `a` and `b` trade places on the stage (`ScriptStage.trade`): what the pointer
   * is on, presses, holds or waits to see come to rest as one is the other from now on, and
   * the listener hears what it hovers by its new name. */
  trade(a: Name, b: Name) {
    const other = (name: Name) => (name === a ? b : name === b ? a : name);
    if (this.#over !== undefined) this.#over = other(this.#over);
    if (this.#settling !== undefined) this.#settling = other(this.#settling);
    if (this.#press) this.#press.name = other(this.#press.name);
    if (this.#held)
      this.#held = { ...this.#held, name: other(this.#held.name) };
    this.#tell();
  }

  /** A press began on `name`, at `at`. */
  press(name: Name, at: Point) {
    this.cancel();
    this.#press = { name, start: at, at, moved: false };
  }

  /** The pointer moved to `at` with its press down: past the threshold, it picks up a card
   * that may be picked up, which then follows it. */
  move(at: Point) {
    const press = this.#press;
    if (!press) return;
    press.at = at;
    const { name, start } = press;
    if (this.#held) {
      this.#held = { name, start, at };
      this.#holding.moved(name);
      return;
    }
    if (Math.hypot(at.x - start.x, at.y - start.y) < DRAG_THRESHOLD) return;
    press.moved = true;
    if (!this.holdable(name) || !this.#holding.pose(name)) return;
    // The card is in hand, not under the pointer: it is hovered no longer.
    this.#held = { name, start, at };
    this.#settling = undefined;
    this.#tell();
    this.#holding.lifted(name);
    this.#listener.lift?.(name);
  }

  /** The press was let go at `at`, over `onto`: a click where it began, or the held card's
   * drop. A press that wandered without picking anything up is neither. */
  release(at: Point, onto?: Under) {
    const press = this.#press;
    // A WebView may deliver the release at a new point without an intervening move (for
    // example, a simulator's injected touch). Treat that final distance as movement too.
    if (press && (press.at.x !== at.x || press.at.y !== at.y)) this.move(at);
    this.#press = undefined;
    if (!press) return;
    const held = this.#held;
    if (!held) {
      if (!press.moved) this.#listener.click?.(press.name, at);
      return;
    }
    this.#held = { ...held, at };
    const pose = this.#holding.pose(held.name) ?? undefined;
    this.#held = undefined;
    this.#listener.lift?.(undefined);
    const taken =
      pose !== undefined &&
      this.#listener.drop?.({
        name: held.name,
        start: held.start,
        at,
        pose,
        onto,
      }) === true;
    this.#letGo(held.name, pose, taken);
  }

  /** The held card `name` was let go where `pose` has it. Taken or not, it is on its way
   * somewhere (home, or where the play it made puts it), and it is not hovered until the
   * stage says it is at rest; the pointer may be on something else by now, which is told
   * afresh. */
  #letGo(name: Name, pose: Pose | undefined, taken: boolean) {
    this.#settling = name;
    this.#holding.dropped(name, pose, taken);
    this.#tell();
  }

  /** The press ended with no release (the browser took the pointer, the node went): a held
   * card goes back to where it rests, and nothing is clicked or dropped. */
  cancel() {
    const held = this.#held;
    this.#press = undefined;
    if (!held) return;
    const pose = this.#holding.pose(held.name) ?? undefined;
    this.#held = undefined;
    this.#listener.lift?.(undefined);
    this.#letGo(held.name, pose, false);
  }

  /** The node `name` is gone from the stage: it is neither hovered nor held any more. */
  gone(name: Name) {
    if (this.#over === name) this.#over = undefined;
    if (this.#settling === name) this.#settling = undefined;
    if (this.#press?.name === name) this.cancel();
    this.#tell();
  }
}
