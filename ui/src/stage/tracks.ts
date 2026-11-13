// The tracks a step gives what it moves: where an entry is `progress` of the way through the
// step, given where it now rests, which a later step may have moved. Each is a function of
// the poses it is made from and of progress alone, so a pose is the same however its stage
// time is reached.
import type { Run } from "./beat.ts";
import type { Act } from "./table.ts";
import type { On } from "./effects.ts";
import { abilityName, type Name, type Point, type Resting } from "./layout.ts";
import { ALOFT, between, type Pose } from "./pose.ts";
import type { Table } from "./table.ts";

/** How a verb is played, in stage milliseconds: how long its motion takes, how long after its
 * turn comes it begins, how long the next step of its beat waits for it, and its curve from 0
 * to 1. The numbers are the design language's (data/ui-conventions.json, timing.ts). */
export interface Timing {
  duration: number;
  delay: number;
  stagger: number;
  ease(progress: number): number;
  /** For a step that moves several cards one after another (a lift), how long each moves. */
  each?: number;
  /** It is played with the step before it: its turn in the beat is that step's, it begins
   * its delay after that step begins, and the step after it waits no longer for the two than
   * for the first. The blow a creature deals back to the one that strikes it, and the life a
   * strike takes. */
  with?: boolean;
  /** It is drawn between these, at rest: it begins its delay no sooner than the steps of its
   * beat that move them have ended (beat.ts). An arrow from a spell on its way to the stack. */
  after?: readonly Name[];
  /** For a step that ends a span: it begins no sooner than this long after the span has
   * come in whole (beat.ts). An arrow whose spell resolves at once is still read. */
  hold?: number;
  /** What follows it in its beat begins no sooner than it is over (beat.ts). The step that
   * ends an arrow or a line of combat, over which the line drains away: nothing it lay
   * between moves until it has gone. */
  leads?: boolean;
  /** It passes over the table: its beat does not wait for it, and it plays on to its own
   * end over the beats, and the scripts, that follow. The turn's banner. */
  passes?: boolean;
  /** It has the table to itself: it begins no sooner than everything passing over the table
   * has gone (beat.ts). A coin, which is not thrown under the turn's banner. */
  alone?: boolean;
  /** What makes room for it does so in its delay: its act is applied as the delay begins
   * (`Run.opens`), and it is first seen as the delay ends, where the table has opened for
   * it. A token, which never shows over a neighbour still moving out of its way. */
  opens?: boolean;
  /** How long what it left room for takes to close up after it, in stage ms, where that is
   * no share of its own time (`CLOSES`): it takes none, a cut. A finished ability. */
  closes?: number;
  /** Its act is applied as the act of the step before it is, where that step is `ahead`
   * too, and it then waits its turn where it was (`Run.opens`): the cards of one draw are
   * all in the hand as the first leaves the library, so that the hand opens their places
   * once and no place moves under a card on its way. */
  ahead?: boolean;
}

/** No motion: the step is over in the tick that begins it. Reduced motion is every verb's. */
export const SNAP: Timing = {
  duration: 0,
  delay: 0,
  stagger: 0,
  ease: (progress) => progress,
};

/** An entry's pose `progress` of the way along its track, resting at `rest`. */
export type Path = (progress: number, rest: Pose) => Pose;

/** How far through its flight a card that leaves face down and lands face up is edge-on to
 * the eye: Arena's opponent's spell is, a little past half way (data/arena-motion.json, fly,
 * "opponent spell cast": 16:47.25 to 16:47.47 of 400 ms). */
export const TURN = 0.55;

/** How long a card that turns over on its way takes to come edge-on, and as long again to
 * lie flat with its face up, as a share of its flight; before and after, it lies flat. An
 * opponent's spell of 400 ms turns over in 88 (the motion review of fly, by-w8gv.21.47: a
 * swap of back for face between two frames is no turn-over). */
export const TURNING = 0.11;

/** When a card turns over on its way: how far through its flight it is edge-on, and how
 * long before and after that it turns, as shares of the flight. */
export interface Turning {
  at: number;
  each: number;
}

/** A flight's turn-over unless it is a draw's. */
const CAST: Turning = { at: TURN, each: TURNING };

/** A drawn card's: it turns over as it leaves the library, about half way to the place
 * beside the stack it flies to, and has its face up for the rest of its way. Arena's is
 * edge-on 100 to 150 ms after it leaves and face up by 170 to 220, of a flight of 700 to
 * 740 where it is not held up (`WQiYBv23buw 29:14.23` to `14.40`). */
export const DRAW_TURN: Turning = { at: 0.16, each: 0.08 };

/** Whether `act` is a card drawn: the flight from a library to its player's hand that the
 * game calls a draw, and no other way a card comes to a hand. */
export const draws = (act: Act) =>
  act.verb === "fly" &&
  act.why === "draw" &&
  act.from === "library" &&
  act.object.zone === "hand";

/** How a card on the flight of the step `act` turns over, if it does. */
export const turningOf = (act: Act): Turning => (draws(act) ? DRAW_TURN : CAST);

/** How much of its height a card that turns over on its way shows `through` of its flight
 * (`Pose.flat`): it turns evenly, top over bottom, half a turn in all, as a card tossed
 * across a table lands the other side up. */
export const flatAt = (through: number, { at, each }: Turning = CAST) =>
  Math.abs(
    Math.cos(
      (Math.PI / 2) * Math.min(2, Math.max(0, (through - at) / each + 1)),
    ),
  );

/** How a flight leaves the straight line between its ends: by `share` of that line where it
 * is farthest off it, `peak` of the way along (half way, if not said), leaning `lean`
 * degrees into the bow there and upright again as it lands. It bows `toward` the edge
 * of the table its card's controller sits at (1 down the stage, -1 up it): the outside of
 * the table, where it crosses the fewest cards. One that swings `back` also leaves away from
 * where it is going, across the table, by that share of how far across the line runs where
 * it is farthest off it: with a bow toward the middle of the table, the card comes out of a
 * hand into the open table and hooks round to its place. */
export interface Bow {
  share: number;
  peak?: number;
  lean?: number;
  back?: number;
  toward: number;
}

/** The bows of the flights that have one (the motion reviews of fly and emerge,
 * by-w8gv.21.47: "a card cast from hand is a thrown object, and here it slides along a
 * ruler line"): a spell thrown onto the stack, a permanent set down from it, which swings out
 * and settles into its row, and an ability lifting out of its source, farthest a little past
 * half way. An opponent's spell bows half as far, and later: their hand lies over the
 * table's far edge, and a full bow carried the card along that edge, half out of sight,
 * before it came down (the third review of fly, there). Arena's own spell bows 0.29 of its
 * line, and its ability's copy and its resolving permanent travel a curve
 * (data/arena-motion.json, fly and emerge). A card drawn that no one reads, the opponent's,
 * leaves the line toward the middle of the table, away from its player's edge. So does the
 * land an opponent plays, and farther: Arena's drops out of their hand down into the table
 * on the side away from their lands, tilted, and swings back along the table to its place
 * (`UrGalw_f35w 16:46.33`, `pE4XSNi-nZc 20:13.60`: farthest out about 0.4 of its time in,
 * between 0.12 and 0.39 of the table's width beyond where it left the hand and 0.18 to 0.27
 * of its height below it). */
export const BOWS = {
  cast: { share: 0.15 },
  theirs: { share: 0.08, peak: 0.6 },
  played: { share: 0.3, back: 1, peak: 0.6, lean: 20 },
  lifts: { share: 0.12, peak: 0.6 },
  drawn: { share: 0.2, peak: 0.42 },
};

/** How the flight of the step `act` bows, if it does, its card's controller sitting at the
 * edge `toward`. */
export function bowOf(act: Act, toward: number): Bow | undefined {
  if (act.verb === "emerge") return { ...BOWS.lifts, toward };
  if (act.verb !== "fly") return undefined;
  if (act.object.zone === "stack") {
    return { ...(toward < 0 ? BOWS.theirs : BOWS.cast), toward };
  }
  if (draws(act)) return { ...BOWS.drawn, toward: -toward };
  if (act.object.zone !== "battlefield") return undefined;
  // The player's own land glides straight from where it is let go (Arena's, `UrGalw_f35w
  // 16:35.42`).
  if (act.from === "hand") {
    return toward < 0 ? { ...BOWS.played, toward: -toward } : undefined;
  }
  return undefined;
}

/** How a permanent that resolves leaves the stack for its place, as Arena's does for either
 * player (`UrGalw_f35w 17:01.13`, `pE4XSNi-nZc 23:02.54` and `21:20.03`, filmed whole at 30
 * frames a second). It does not slide along a line. In its first frame off the stack it is
 * already smaller, `width` of the spell's width, and it is thrown up the screen: within
 * `risen` of its time it is `height` of the spell's width above the straight line, it stays
 * that high until `held` of its time while it crosses most of the way, and then it drops
 * onto its place, shrinking to its size there as it comes down. It also leaves faster than
 * the verb's curve alone: `launch` sharpens the curve, so that it is half way across after
 * a quarter of its time. */
export const LIFT = {
  height: 0.95,
  risen: 0.2,
  held: 0.57,
  width: 0.8,
  shrunk: 0.06,
  launch: 1.4,
};

/** Whether the flight of the step `act` is a permanent's from the stack to the battlefield,
 * which is thrown up and drops onto its place (`LIFT`). */
export const lifts = (act: Act) =>
  act.verb === "fly" &&
  act.from === "stack" &&
  act.object.zone === "battlefield";

/** How high above its straight line a permanent leaving the stack is, `progress` of its time
 * through, as a share of its greatest height: up quickly, level, then down evenly. */
export function liftAt(progress: number): number {
  const at = Math.min(1, Math.max(0, progress));
  if (at < LIFT.risen) return 1 - (1 - at / LIFT.risen) ** 2;
  return at <= LIFT.held ? 1 : 1 - (at - LIFT.held) / (1 - LIFT.held);
}

/** `pose`, which is on the straight way from `start` to `rest`, as a permanent leaving the
 * stack has it `progress` of its time through: lifted up the screen, and as wide as its
 * height above the table makes it. */
export function lifted(
  pose: Pose,
  start: Pose,
  rest: Pose,
  progress: number,
): Pose {
  const high = liftAt(progress);
  // It never rises past the table's far edge, which is the top of the stage: a place in the
  // opponent's rows has less room over it than Arena's camera leaves.
  const room = Math.max(0, Math.min(start.y, rest.y));
  // It is the spell's own width as it leaves, smaller within two frames, and from there
  // comes down to its width at rest as it descends.
  const aloft = LIFT.width * start.width;
  const down = (at: number) =>
    rest.width + (aloft - rest.width) * (1 - at ** 1.5);
  const wide =
    progress < LIFT.shrunk
      ? start.width +
        (down(LIFT.shrunk) - start.width) * (progress / LIFT.shrunk)
      : down(progress);
  return {
    ...pose,
    // A pose is placed by its top-left corner: it stays about its middle as it narrows.
    x: pose.x + (pose.width - wide) / 2,
    y: pose.y - Math.min(LIFT.height * start.width, room) * high,
    width: wide,
  };
}

/** `pose`, which is `through` of the straight way from `from` to `to`, as far off that way
 * as `bow` has it there: nothing at either end. */
export function bowed(
  pose: Pose,
  from: Pose,
  to: Pose,
  through: number,
  bow: Bow,
): Pose {
  const [across, down] = [to.x - from.x, to.y - from.y];
  // Of the two sides of the line, the one toward that edge of the table.
  const side = across * bow.toward < 0 ? -1 : 1;
  const sharpness = Math.log(0.5) / Math.log(bow.peak ?? 0.5);
  const arch = Math.sin(Math.PI * Math.max(0, through) ** sharpness);
  const off = side * bow.share * arch;
  const back = (bow.back ?? 0) * arch;
  return {
    ...pose,
    x: pose.x - down * off - across * back,
    y: pose.y + across * off,
    // It leans into the bow, its top toward the side it swings out to.
    rotation: pose.rotation - side * (bow.lean ?? 0) * arch,
  };
}

/** Whether two poses lie the opposite way up the table, nearer half a turn apart than none:
 * an opponent's hand, which faces them, and the stack. */
const opposed = (a: Pose, b: Pose) =>
  Math.abs(((((a.rotation - b.rotation) % 360) + 540) % 360) - 180) > 90;

/** `pose` with its card half a turn round in the table's plane about `middle`, the card's
 * own middle: over the same place, the other way up. Of the two rotations that is, the one
 * nearer `near`. */
export function halfTurned(pose: Pose, middle: Point, near: number): Pose {
  return {
    ...pose,
    x: 2 * middle.x - pose.x,
    y: 2 * middle.y - pose.y,
    rotation: pose.rotation + (near > pose.rotation ? 180 : -180),
  };
}

/** What is said of a glide besides its ends. With `over`, its card leaves face down and
 * lands face up, and turns over on the way (`flatAt`), as `turning` says: `over` gives a
 * pose half a turn round (`halfTurned`). With `bow`, it leaves the straight line. With
 * `round`, which gives the same, it lands face down. */
export interface Flown {
  over?: (pose: Pose, near: number) => Pose;
  turning?: Turning;
  bow?: Bow;
  round?: (pose: Pose, near: number) => Pose;
  /** It is thrown up off the stack and drops onto its place (`lifted`). */
  lift?: boolean;
}

/** The glide to its resting place of an entry that was at `start`: `aloft` for what a step
 * is about, and at its resting depth for what makes room. One that turns over shows its back
 * until it is edge-on, `TURN` of the way there. Where it lands the other way up the table
 * than it left, the turn-over is all that brings it round: a card tossed top over bottom
 * lands facing the other way, so it never spins in the table's plane besides. Until it is
 * edge-on it is on its way to where it rests half a turn round, and from then it comes from
 * where it was half a turn round; the two ways part by a hair, where nothing of it shows.
 * One that lands face down the other way up the table has nothing to turn it round, and a
 * card back spinning half a turn across the table is all the eye would follow: it leaves
 * the way up it will lie, turned in the tick it begins to move (the opponent's draw, from a
 * library that lies upright to a hand that faces them). */
export const glide =
  (
    start: Pose,
    ease: Timing["ease"],
    aloft: boolean,
    { over, turning = CAST, bow, round, lift }: Flown = {},
  ): Path =>
  (progress, rest) => {
    const eased = ease(progress);
    const through = lift ? 1 - (1 - eased) ** LIFT.launch : eased;
    const back = over !== undefined && progress < turning.at;
    const turned = over !== undefined && opposed(start, rest);
    const hidden = over === undefined && round !== undefined;
    const from =
      turned && !back
        ? over(start, rest.rotation)
        : hidden && opposed(start, rest)
          ? round(start, rest.rotation)
          : start;
    const to = turned && back ? over(rest, start.rotation) : rest;
    const straight = between(from, to, through, aloft);
    const curved = bow ? bowed(straight, from, to, through, bow) : straight;
    const pose = lift ? lifted(curved, from, to, progress) : curved;
    if (!over) return pose;
    return {
      ...pose,
      face: back ? "back" : pose.face,
      flat: flatAt(progress, turning),
    };
  };

/** What a card that showed its face at `before` is drawn as on its way to `after`, where it
 * lies face down or out of sight: as it was, there. None for any other move, which is
 * dressed as it will rest from the first. */
export function shownUntil(
  before: Resting,
  after: Resting,
): Resting | undefined {
  const hid = ({ dress }: Resting) =>
    dress.back || dress.rim || dress.look === "hidden";
  return hid(after) && !hid(before)
    ? { ...before, pose: after.pose }
    : undefined;
}

/** An entry that is at its resting place from the first, and comes to be seen there. */
export const fade =
  (ease: Timing["ease"]): Path =>
  (progress, rest) => ({ ...rest, opacity: rest.opacity * ease(progress) });

/** How a face of the Revealed Cards pile comes to be seen: how large it is at first, against
 * its size at rest, and how far through its step it is whole to the eye. Arena's grows in
 * place from a speck and is opaque from the first (`I3Te0m1bBFw 22:08.40` to `22:08.53`);
 * Beryl's starts at over half its size, so that a face is a card from its first frame. */
export const SHOWN = { size: 0.6, seen: 0.5 };

/** An entry that comes to be seen where it rests, growing to its size there: `about` is a
 * pose so many times as large about its card's middle. */
export const shownUp =
  (about: (pose: Pose, size: number) => Pose, ease: Timing["ease"]): Path =>
  (progress, rest) => {
    const through = ease(progress);
    return {
      ...about(rest, SHOWN.size + (1 - SHOWN.size) * through),
      opacity: rest.opacity * Math.min(1, through / SHOWN.seen),
    };
  };

/** How long what a card that leaves made room for takes to close up once it has gone, as a
 * share of the step that took it. The step is the card's going and no more: as it ends the
 * card is at its new place, and only what then has room to close up moves after it, so that
 * nothing waits where nothing does (the motion review of vanish, by-w8gv.21.48). Arena's row
 * closes up after the permanent that left it is gone (3sEoP_s4DQE 24:49.6 to 24:49.8, a
 * creature returned to its hand). */
export const CLOSES = 0.4;

/** When what only makes room in a step of `verb`, from `from` to `to`, goes to where it
 * rests: over the step, or once a card that leaves has gone, for `CLOSES` of its step, in
 * whole milliseconds. */
export const roomIn = (
  verb: Act["verb"],
  from: number,
  to: number,
): [from: number, to: number] =>
  verb === "vanish" ? [to, to + Math.round(CLOSES * (to - from))] : [from, to];

/** How long what a finished ability left room for takes to close up (the motion review of
 * vanish, by-w8gv.21.47: a phone's rows, hands and piles closed its band of the table "in
 * one frame"; "one move of about 200 to 250 ms, ease-out, starting on the frame after"). The
 * ability itself is cut (UI Conventions, "A finished ability"). */
export const CLOSED = 225;

/** An entry that goes from sight where it was at `start`, and never travels: slowly at first,
 * while the effect that says how it leaves is seen on it (effects.ts), all at once at the
 * last, and gone as its step ends. */
export const dissolve =
  (start: Pose, ease: Timing["ease"]): Path =>
  (progress) => ({
    ...start,
    opacity: start.opacity * (1 - ease(progress) ** 2),
  });

/** The glide of an entry that was at `start` and stays there until `from` of its step, then
 * glides to where it rests by `to`, at its resting depth. */
export const later =
  (start: Pose, ease: Timing["ease"], from: number, to = 1): Path =>
  (progress, rest) =>
    progress <= from
      ? start
      : between(
          start,
          rest,
          ease(Math.min(1, (progress - from) / (to - from))),
          false,
        );

/** The share of a step moving cards one after another, each for `each` of its stage
 * milliseconds, in which the card `name` moves: the cards a look handles, in the order they
 * come off their pile. */
export function shareOf(
  step: { from: number; to: number; each?: number },
  table: Table,
  name: Name,
): [number, number] {
  const handled = table.handled ?? [];
  const length = step.to - step.from;
  const each = step.each ?? length;
  const gap = handled.length > 1 ? (length - each) / (handled.length - 1) : 0;
  const from = Math.max(0, handled.indexOf(name as number)) * gap;
  return [from / length, (from + each) / length];
}

/** How far a permanent turns as it taps, in degrees. */
export const QUARTER = 90;

/** How far over where it rests a permanent picked to pay a cost is drawn (spans.ts): over
 * every permanent, whose rows it reaches across as it lies turned, and under the stack. */
export const PAYING = 200;

/** The `pose` of a permanent whose mark says it is picked to pay a cost, `turned` of the way
 * to that mark's look (spans.ts): untapped, it turns as far as it would to tap; once the
 * script has tapped it under the mark it lies turned as it rests, `rest`, from the tap's
 * first tick, and so is not seen to stand up and turn again. Over its row from the first:
 * turned, it reaches across the card beside it. */
export function paying(
  pose: Pose,
  rest: Pose,
  { tapped }: { tapped: boolean },
  turned: number,
): Pose {
  const turn = tapped ? rest.turn : pose.turn + QUARTER * turned;
  return { ...pose, turn, depth: pose.depth + PAYING };
}

/** How much of its step a permanent that taps or untaps turns in. It turns where it lies, and
 * only then do it and its row go to where they now rest, the tapped apart from the untapped
 * (game/battlefield.ts), so that nothing crosses a card while it turns. */
export const TURNED = 0.6;

/** A permanent turning as it taps or untaps (UI Conventions, exception 2). It is dressed as
 * its record now says, lying or upright, and `unturned` is where that dress is drawn with its
 * card a quarter back, so that the card lies exactly as it did as the step began: from there
 * the card turns about its own middle to `home`, as it now rests but with that middle where
 * it was, and then goes to where it rests, at the depth it rests at. Where it rests within
 * `NEAR` of its width of `home` it turns straight onto its place, in one motion. */
export const turned =
  (unturned: Pose, home: Pose, ease: Timing["ease"]): Path =>
  (progress, rest) => {
    const far = Math.hypot(rest.x - home.x, rest.y - home.y);
    const onto = far < NEAR * rest.width ? rest : home;
    return progress < TURNED
      ? between(unturned, onto, ease(progress / TURNED), false)
      : between(onto, rest, ease((progress - TURNED) / (1 - TURNED)), false);
  };
/** How near where it lay, in its own widths, a place is that a card turns straight onto. */
export const NEAR = 0.5;

/** How far along its way what only makes room in a step of these verbs begins to move, and
 * is until then as it was, at the depth it had: once a card that taps has turned. What makes
 * room for a card that leaves has its way after that card's step (`roomIn`), and waits there
 * for it to begin: a spell over one that leaves the stack stays over it while it goes. */
const WAITS: Partial<Record<Act["verb"], number>> = {
  vanish: 0,
  tap: TURNED,
};

/** How far out of its row a card swings as it steps past the cards that tapped, in its own
 * widths, and never more than a third of how far it goes: a card that only closes up a gap
 * hardly leaves the row. */
const SWING = 0.45;

/** The way of an entry that was at `start` and only makes room in a step of `verb`, if such
 * a step keeps it waiting (`WAITS`): it stays there and then glides to where it rests. After
 * a tap the untapped and the tapped of a row change places, and it swings out of the row on
 * its way, `toward` its controller's edge of the table (1 down the stage, -1 up it), so that
 * it is seen to step past and does not pass under them. */
export function makingRoom(
  verb: Act["verb"],
  start: Pose,
  ease: Timing["ease"],
  toward: number,
): Path | undefined {
  const from = WAITS[verb];
  if (from === undefined) return undefined;
  const waited = later(start, ease, from);
  if (verb !== "tap") return waited;
  return (progress, rest) => {
    const pose = waited(progress, rest);
    const far = Math.hypot(rest.x - start.x, rest.y - start.y);
    const out = Math.min(far / 3, SWING * rest.width);
    const along = Math.max(0, (progress - from) / (1 - from));
    return { ...pose, y: pose.y + toward * out * Math.sin(Math.PI * along) };
  };
}

const into = (progress: number) => progress * progress;
const out = (progress: number) => 1 - (1 - progress) * (1 - progress);
const inOut = (progress: number) =>
  progress < 0.5 ? 2 * into(progress) : 1 - 2 * into(1 - progress);

/** How a strike is shared out and how far it goes. Arena's attacker pulls back a little,
 * dashes at the avatar or its blocker and drifts back more slowly (data/arena-motion.json,
 * strike: "small pull back, then a straight dash ..., then a drift back"); a strike that
 * begins at 22:25.4 lands at 25.80 of 650 to 800 ms. `wind` and `hit` are the shares of the
 * step by which it has pulled back and has landed, and `back` how far it pulls back, in its
 * own widths. Beryl's cards are whole and lie close, so the blow is where the two first
 * overlap by `bite` of the striker's width and no further: what is struck stays in sight
 * under the blow, and two creatures that strike each other meet half way (enter.ts). Before
 * a blow that is hardly any way off it still goes `least` of its width. What is much smaller
 * than the striker (a phone's small permanents and its life totals) it dives onto instead,
 * shrinking as it goes, or it would lie over the neighbours more than over what it strikes:
 * not at all for something `small.from` of its length, wholly for something `small.to`, when
 * it lands `small.least` of its own size. Its blow has been seen by `seen` of its step,
 * when it gives way to the strikers still on their way (`depths`), and by `home` it is as
 * good as back where it rests, all but the last of its slowing: a strike that would pass
 * over it before then is held back (enter.ts `awaited`). */
export const STRIKE = {
  wind: 0.15,
  hit: 0.45,
  seen: 0.57,
  home: 0.9,
  back: 0.3,
  bite: 0.38,
  least: 0.3,
  small: { from: 0.7, to: 0.55, least: 0.6 },
};

/** How far along its way to the blow a creature is `progress` through its strike, and how
 * far back of where it rests, as a share of how far it pulls back: it pulls back, slowing,
 * dashes from there, quickening all the way to the blow, stays on it a moment and draws back,
 * slowing onto its place. */
export function struck(progress: number): [toward: number, back: number] {
  const { wind, hit } = STRIKE;
  if (progress < wind) return [0, out(progress / wind)];
  if (progress < hit) {
    const dash = into((progress - wind) / (hit - wind));
    return [dash, 1 - dash];
  }
  return [1 - inOut(Math.min(1, (progress - hit) / (1 - hit))), 0];
}

/** A creature's lunge at what it deals combat damage to and its return to where it rests:
 * the blow is `toward` away from it as the step begins, and it first pulls `back` the other
 * way. It is `size` times as large at the blow, by `about`, which gives a pose that large
 * about its card's middle. Never turned, and over everything at rest throughout: at the
 * depth `over` until its blow has been seen, and at `after` from then (`depths`). */
export const lunge =
  (
    toward: Point,
    back: Point,
    size: number,
    about: (pose: Pose, size: number) => Pose,
    [over, after]: readonly [number, number],
  ): Path =>
  (progress, rest) => {
    const [to, away] = struck(progress);
    const pose = about(rest, 1 + (size - 1) * to);
    return {
      ...pose,
      x: pose.x + toward.x * to - back.x * away,
      y: pose.y + toward.y * to - back.y * away,
      depth: progress < STRIKE.seen ? over : after,
    };
  };

/** How deep the strike is drawn that is the `nth` step of its beat, or with `answers` the
 * blow dealt back to that step's: until its blow has been seen, over every card and over
 * the strikes after it; from then, still over what rests, under every strike on its way and
 * over the earlier ones drawing back, so that a strike whose way crosses another fight goes
 * over it both ways. A creature is over the one that strikes it back throughout. */
export function depths(nth: number, answers: boolean): [number, number] {
  const rank = 2 * Math.min(nth, 40) + (answers ? 1 : 0);
  return [2 * ALOFT - rank, ALOFT + rank + (answers ? -1 : 1)];
}

/** Whether the strikes `a` and `b` are the two blows of one fight: each creature strikes the
 * other. They are played as one, at one time (the script puts them together:
 * beryl_session's script/infer.rs `order`). */
export const meets = (a: Act, b: Act | undefined) =>
  a.verb === "strike" &&
  b?.verb === "strike" &&
  a.to.kind === "object" &&
  b.to.kind === "object" &&
  a.to.slot === b.source &&
  b.to.slot === a.source;

/** How a card that turns over where it lies is shared out and how large it grows (UI
 * Conventions, "A card turning face up": it "enlarges in place and flips"): it has grown by
 * `grown` of the step, shows its new face from the middle, and begins to settle at `settle`.
 * The veil that hides the change is the `cover` effect's (stage.css). */
export const FLIP = { size: 1.2, grown: 0.3, turn: 0.5, settle: 0.7 };

/** A card turning over in place: it grows about `middle` of itself, quickly and then slowing
 * into its largest, shows the side it showed as the step began until `FLIP.turn`, and settles
 * back to where it rests, slowing onto the table. `about` gives a pose `size` times as large
 * about that middle. */
export const turnedOver =
  (start: Pose, about: (pose: Pose, size: number) => Pose): Path =>
  (progress, rest) => {
    const { size, grown, turn, settle } = FLIP;
    const share =
      progress < grown
        ? out(progress / grown)
        : progress < settle
          ? 1
          : 1 - inOut((progress - settle) / (1 - settle));
    const pose = about(rest, 1 + (size - 1) * share);
    return {
      ...pose,
      depth: ALOFT + rest.depth,
      face: progress < turn ? start.face : rest.face,
    };
  };

/** Whether the step `run` flies its card to a pile: a library, a graveyard or exile. Until
 * it lands, the pile shows the top it had (stage.ts `Entry.landing`). */
export const piled = ({ step: { act } }: Pick<Run, "step">) =>
  act.verb === "fly" &&
  ["library", "graveyard", "exile"].includes(act.object.zone);

/** What a step's act moves, if that is something on the table: the creature that strikes,
 * or else what the act is about. */
export const moverOf = (act: Act): Name | undefined =>
  act.verb === "strike" ? act.source : subjectOf(act);

/** What the effects of a step's act are on (effects.ts): what the act is about, or what its
 * number is on, a permanent or a player's life total. */
export function litBy(act: Act): On | undefined {
  if (act.verb !== "count") return subjectOf(act);
  const { tally } = act;
  if (tally.kind === "life") return tally.player;
  return tally.kind === "pool" ? undefined : tally.object.slot;
}

/** A step as the track of what its change moves is made from (beat.ts `Run`). */
type Moving = Pick<
  Run,
  "verb" | "from" | "to" | "ease" | "each" | "opens" | "closes" | "step"
>;

/** Whether what the change of the step `run`, applied at stage time `at`, moves travels to
 * where it now rests: the step takes time, or what it left room for closes up after it. */
export const carries = (run: Moving, at: number) =>
  run.to > at || run.closes !== undefined;

/** The track of the entry `name`, whose resting place the step `run`, applied at stage time
 * `at`, moved from `before` to `resting`, it being at `start`; and what it is drawn as until
 * that track turns it over. Only the card a `fly` is about, or a `count` (a number that
 * lays its card elsewhere), travels over what rests, bowed if its flight is (`bowOf`) and
 * turning over on the way if it lands face up from face down, `halfTurn` giving a pose half
 * a turn round (`halfTurned`). It flies for its step's own time, where it is until then if
 * its act went ahead of it (`Timing.ahead`). What another verb's change moves goes there as what makes
 * room does, when its verb has it go: over the step; once a card has turned (`WAITS`);
 * after a card that left has gone, for `CLOSES` of its step or, after a cut, for the time
 * the step says (`Timing.closes`); or before the step begins, where its act went ahead of it
 * (`Timing.opens`), slowing into its place. A card a `lift` lets go to lie face down or out
 * of sight shows the face it had until it is there. */
export function carried(
  run: Moving,
  at: number,
  [name, start, before, resting]: [Name, Pose, Resting, Resting],
  table: Table,
  halfTurn: NonNullable<Flown["over"]>,
): [Track, Resting | undefined] {
  const { verb, to: end } = run;
  const kept = verb === "lift" ? shownUntil(before, resting) : undefined;
  const toward = resting.object?.controller === "opponent" ? -1 : 1;
  const about = verb === "fly" || verb === "count";
  const flies = about && litBy(run.step.act) === name;
  // The land an opponent plays is face up as it leaves their hand, the way up it will lie
  // (Arena's, `UrGalw_f35w 16:46.37`): it does not turn over on the way.
  const faceUp = flies && bowOf(run.step.act, toward)?.back !== undefined;
  const turns =
    flies && !faceUp && start.face === "back" && resting.pose.face === "front";
  const opens = run.opens !== undefined && !flies;
  const [from, to] = opens
    ? [at, run.opens!.by]
    : run.closes !== undefined
      ? [end, end + run.closes]
      : flies
        ? [run.from, end]
        : roomIn(verb, at, end);
  const ease = opens || run.closes !== undefined ? out : run.ease;
  const pose = run.each
    ? later(start, ease, ...shareOf(run, table, name))
    : ((opens ? undefined : makingRoom(verb, start, ease, toward)) ??
      glide(start, ease, flies, {
        over: turns ? halfTurn : undefined,
        turning: turningOf(run.step.act),
        bow: flies ? bowOf(run.step.act, toward) : undefined,
        lift: flies && lifts(run.step.act),
        round:
          flies && (faceUp || resting.pose.face === "back")
            ? halfTurn
            : undefined,
      }));
  const turn = turns ? turningOf(run.step.act).at : kept && 1;
  return [{ from, to, closes: to > end, pose, turn, own: flies }, kept];
}

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
  draws(act) &&
  act.verb === "fly" &&
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
  /** It closes up after the step that moved it has ended (`roomIn`): that step's beat is
   * not over until it is where it rests. */
  closes?: boolean;
  /** A card let go where its drop was taken, kept there with no end of its own until what the
   * drop did has played: a step that moves it starts from there, and otherwise it glides home
   * when the script is over. */
  stays?: boolean;
}

/** The way home from `from`, begun at stage time `at`, of a card let go, played as `release`
 * says: none if that takes no time. */
export function home(
  release: Timing,
  from: Pose,
  at: number,
): Track | undefined {
  const { duration, ease } = release;
  if (duration <= 0) return undefined;
  return { from: at, to: at + duration, pose: glide(from, ease, true) };
}

/** How far along `track` stage time `at` is. */
export const along = (track: Track, at: number) =>
  Math.max(0, (at - track.from) / (track.to - track.from));

/** Whether `track` has yet to turn its card face up at stage time `at`. */
export const turning = (track: Track | undefined, at: number) =>
  track?.turn !== undefined && at < track.to && along(track, at) < track.turn;

/** Where what is on `track`, or on none, and rests at `resting` is at stage time `at`: on its
 * track while one runs, else at rest. */
export function poseOn(
  { track, resting }: { track?: Track; resting: { pose: Pose } },
  at: number,
): Pose {
  if (!track || at >= track.to) return resting.pose;
  return track.pose(along(track, at), resting.pose);
}

/** When a beat that would end at `end` is over, with `entries` on their tracks: no sooner
 * than the last of them closing up after one of its steps is where it rests. */
export function closedBy(entries: Iterable<{ track?: Track }>, end: number) {
  let last = end;
  for (const { track } of entries) {
    if (track?.closes) last = Math.max(last, track.to);
  }
  return last;
}
