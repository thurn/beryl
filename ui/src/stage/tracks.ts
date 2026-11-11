// The tracks a step gives what it moves: where an entry is `progress` of the way through the
// step, given where it now rests, which a later step may have moved. Each is a function of
// the poses it is made from and of progress alone, so a pose is the same however its stage
// time is reached.
import type { Act } from "../generated/protocol";
import { abilityName, type Name, type Point } from "./layout.ts";
import { ALOFT, between, type Pose } from "./pose.ts";
import type { Timing } from "./stage.ts";

/** An entry's pose `progress` of the way along its track, resting at `rest`. */
export type Path = (progress: number, rest: Pose) => Pose;

/** How far through its flight a card that leaves face down and lands face up turns over:
 * Arena's opponent's spell is edge-on a little past half way (data/arena-motion.json, fly,
 * "opponent spell cast": 16:47.25 to 16:47.47 of 400 ms). */
export const TURN = 0.55;

/** The glide to its resting place of an entry that was at `start`: `aloft` for what a step
 * is about, and at its resting depth for what makes room. With `turns`, it shows its back
 * until it is `TURN` of the way there. */
export const glide =
  (start: Pose, ease: Timing["ease"], aloft: boolean, turns = false): Path =>
  (progress, rest) => {
    const pose = between(start, rest, ease(progress), aloft);
    return turns && progress < TURN ? { ...pose, face: "back" } : pose;
  };

/** An entry that is at its resting place from the first, and comes to be seen there. */
export const fade =
  (ease: Timing["ease"]): Path =>
  (progress, rest) => ({ ...rest, opacity: rest.opacity * ease(progress) });

/** An entry that goes from sight where it was at `start`, and never travels. */
export const dissolve =
  (start: Pose, ease: Timing["ease"]): Path =>
  (progress) => ({ ...start, opacity: start.opacity * (1 - ease(progress)) });

/** How a card held up on its way is shared out over its step: the share of the time it
 * flies in, and the share it is held. Arena's draw at a turn's start flies in for about 600
 * ms, holds 200 to 240 and drops for 230 to 300 (data/arena-motion.json, fly, "draw":
 * 20:28.4 and 16:52.4). */
export const HELD = { in: 0.55, hold: 0.2 };

/** When, as a share of its step, a card held up has turned over: `TURN` of its way in. */
export const HELD_TURN = HELD.in * TURN;

/** The way of a card that is held up to be read before it lands: from `start` to where
 * `held` puts it for its resting place, turning over on the way, a hold there, and the drop
 * to where it rests. */
export const heldUp =
  (start: Pose, ease: Timing["ease"], held: (rest: Pose) => Pose): Path =>
  (progress, rest) => {
    const up = { ...held(rest), depth: ALOFT + rest.depth };
    const dropping =
      (progress - HELD.in - HELD.hold) / (1 - HELD.in - HELD.hold);
    if (dropping > 0) return between(up, rest, ease(dropping), true);
    const flying = Math.min(1, progress / HELD.in);
    const pose = {
      ...between(start, up, ease(flying), false),
      depth: up.depth,
    };
    return start.face === "back" && progress < HELD_TURN
      ? { ...pose, face: "back" }
      : pose;
  };

/** A permanent turning as it taps or untaps (UI Conventions, exception 2). It is dressed as
 * its record now says, lying or upright, and `unturned` is where that dress is drawn with its
 * card a quarter back, so that the card lies exactly as it did as the step began: from there
 * the card turns about its own middle while that middle goes to where it rests, at the depth
 * it rests at. */
export const turned =
  (unturned: Pose, ease: Timing["ease"]): Path =>
  (progress, rest) =>
    between(unturned, rest, ease(progress), false);

const into = (progress: number) => progress * progress;
const out = (progress: number) => 1 - (1 - progress) * (1 - progress);

/** How a strike is shared out and how far it goes. Arena's attacker pulls back a little,
 * dashes at the avatar or its blocker and drifts back more slowly (data/arena-motion.json,
 * strike: "small pull back, then a straight dash ..., then a drift back"); a strike that
 * begins at 22:25.4 lands at 25.80 of 650 to 800 ms. `wind` and `hit` are the shares of the
 * step by which it has pulled back and has landed; `back` and `reach` how far it then is
 * along the way from its place to what it strikes. The shake and flashes over Arena's
 * strikes left its tracker nothing to measure the distances by. */
export const STRIKE = { wind: 0.15, hit: 0.45, back: 0.08, reach: 0.75 };

/** How far along its way to what it strikes a creature is, `progress` through its strike. */
export function struck(progress: number): number {
  const { wind, hit, back, reach } = STRIKE;
  if (progress < wind) return -back * out(progress / wind);
  if (progress < hit) {
    return -back + (reach + back) * into((progress - wind) / (hit - wind));
  }
  return reach * (1 - out(Math.min(1, (progress - hit) / (1 - hit))));
}

/** A creature's lunge at what it deals combat damage to, `toward` away from it as the step
 * begins, and its drift back to where it rests: over everything, and never turned or sized. */
export const lunge =
  (toward: Point): Path =>
  (progress, rest) => {
    const along = struck(progress);
    return {
      ...rest,
      x: rest.x + toward.x * along,
      y: rest.y + toward.y * along,
      depth: ALOFT + rest.depth,
    };
  };

/** How a card that turns over where it lies is shared out and how large it grows (UI
 * Conventions, "A card turning face up": it "enlarges in place and flips"): it has grown by
 * `grown` of the step, shows its new face from the middle, and begins to settle at `settle`. */
export const FLIP = { size: 1.3, grown: 0.3, turn: 0.5, settle: 0.7 };

/** A card turning over in place: it grows about `middle` of itself, shows the side it showed
 * as the step began until `FLIP.turn`, and settles back to where it rests. `about` gives a
 * pose `size` times as large about that middle. */
export const turnedOver =
  (
    start: Pose,
    ease: Timing["ease"],
    about: (pose: Pose, size: number) => Pose,
  ): Path =>
  (progress, rest) => {
    const { size, grown, turn, settle } = FLIP;
    const share =
      progress < grown
        ? progress / grown
        : progress < settle
          ? 1
          : 1 - (progress - settle) / (1 - settle);
    const pose = about(rest, 1 + (size - 1) * ease(Math.max(0, share)));
    return {
      ...pose,
      depth: ALOFT + rest.depth,
      face: progress < turn ? start.face : rest.face,
    };
  };

/** What a step's act moves, if that is something on the table: the creature that strikes,
 * or else what the act is about. */
export const moverOf = (act: Act): Name | undefined =>
  act.verb === "strike" ? act.source : subjectOf(act);

/** What a step's act is about, if it is something on the table: its card, or the ability it
 * puts on the stack or takes from it. */
export function subjectOf(act: Act): Name | undefined {
  if (act.verb === "emerge") return abilityName(act.id);
  if (act.verb === "vanish" && act.subject.kind !== "player") {
    return act.subject.kind === "object"
      ? act.subject.slot
      : abilityName(act.subject.id);
  }
  return "object" in act ? act.object?.slot : undefined;
}

/** Whether `act` is your draw of a card you see: the one flight in which a card is held up
 * to be read (UI Conventions, Verbs, fly). */
export const drawsToRead = (act: Act) =>
  act.verb === "fly" &&
  act.why === "draw" &&
  act.from === "library" &&
  act.object.zone === "hand" &&
  act.object.owner === "you" &&
  act.object.name !== null;

/** An entry's motion over a step: where it is `progress` of the way through, given where it
 * now rests, which a later step may have moved. */
export interface Track {
  from: number;
  to: number;
  pose: Path;
  /** How far along it turns face up, if it leaves face down for a place where its face
   * shows: until then it is dressed as a card back. */
  turn?: number;
  /** The track of the step that is about it. A later step that moves where it rests does
   * not take it over: it ends there instead. */
  own?: boolean;
  /** A card let go where its drop was taken, kept there with no end of its own until what the
   * drop did has played: a step that moves it starts from there, and otherwise it glides home
   * when the script is over. */
  stays?: boolean;
}

/** How far along `track` stage time `at` is. */
export const along = (track: Track, at: number) =>
  Math.max(0, (at - track.from) / (track.to - track.from));

/** Whether `track` has yet to turn its card face up at stage time `at`. */
export const turning = (track: Track | undefined, at: number) =>
  track?.turn !== undefined && at < track.to && along(track, at) < track.turn;
