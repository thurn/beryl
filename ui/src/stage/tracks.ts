// The tracks a step gives what it moves: where an entry is `progress` of the way through the
// step, given where it now rests, which a later step may have moved. Each is a function of
// the poses it is made from and of progress alone, so a pose is the same however its stage
// time is reached.
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
  /** It passes over the table: its beat does not wait for it, and it plays on to its own
   * end over the beats, and the scripts, that follow. The turn's banner. */
  passes?: boolean;
  /** It has the table to itself: it begins no sooner than everything passing over the table
   * has gone (beat.ts). A coin, which is not thrown under the turn's banner. */
  alone?: boolean;
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
