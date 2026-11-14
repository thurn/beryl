// The motion a step gives what it moves where that is no glide from where it was (stage.ts,
// `#begin`): an ability lifting out of its source, a token fading in, a draw held up to be
// read, a permanent turning as it taps, a creature's lunge, a card turning over in place. Each
// is made from the model as the step begins (where things are and how they are dressed, and
// how they were before the step's act), whatever the scripts before it were and however they
// were cut: nothing here reads a view. The geometry is the layout's.
import type { PlayerView } from "../generated/protocol";
import type { Act } from "./table.ts";
import {
  abilityName,
  revealedName,
  type Name,
  type Point,
  type Resting,
  type StageLayout,
} from "./layout.ts";
import { HELD_TURN, heldUp, swept } from "./drawn.ts";
import type { Pose } from "./pose.ts";
import { bowOf } from "./bows.ts";
import {
  depths,
  DRAW_TURN,
  drawsToRead,
  fade,
  FLIP,
  glide,
  lunge,
  meets,
  shownUp,
  STRIKE,
  struck,
  turned,
  turnedOver,
  turning,
  type Track,
} from "./tracks.ts";

/** One thing on the stage as a step begins: how it rests, and where it is. */
export interface Placed {
  resting: Resting;
  pose: Pose;
}

/** What a step's own motion is made from. */
export interface Scene {
  layout: StageLayout;
  /** The table once the step's act is applied, and as the step found it. */
  table: PlayerView;
  found: PlayerView;
  /** What the stage holds under `name`, at rest as the act leaves it and where it is now. */
  placed(name: Name): Placed | undefined;
  /** What the step moves (tracks.ts `moverOf`), as it was before the act. */
  was: Placed | undefined;
  /** The layout moved where the step's mover rests, and it is gliding there. */
  moving: boolean;
  /** The acts of every step of the step's beat, its own among them. */
  beat: readonly Act[];
}

/** A track for the node `name`, which shows as `cover` until the track turns it over. One
 * that `rides` goes on along the track of its own it is on, if it is on one, and takes only
 * the turn: what a strike lands on while it strikes back. */
export interface Entering {
  name: Name;
  track: Omit<Track, "from" | "to">;
  cover?: Resting;
  rides?: boolean;
}

/** Where it rests, for as long as its step: it only waits to show what the step did to it. */
const stays: Track["pose"] = (_, rest) => rest;

const own = (pose: Track["pose"]) => ({ pose, own: true });

const away = (from: Point, to: Point): Point => ({
  x: to.x - from.x,
  y: to.y - from.y,
});

const middleOf = (layout: StageLayout, { resting, pose }: Placed) =>
  layout.middle(resting, pose);

/** How much of half its art's height the face of a life orb is (table.css `.life-hit`). */
const FACE = 0.8;

/** How far into the face of a life orb, from its rim to its middle, a striker that dives
 * onto it goes: the total stays in sight. */
const INTO = 0.55;

/** A pose `size` times as large about the middle of the card `resting` is dressed as. */
const sized =
  (layout: StageLayout, resting: Resting) =>
  (pose: Pose, size: number): Pose => {
    const middle = layout.middle(resting, pose);
    return {
      ...pose,
      x: middle.x + (pose.x - middle.x) * size,
      y: middle.y + (pose.y - middle.y) * size,
      width: pose.width * size,
    };
  };

/** A strike's way: the middle of its creature's card and how far that reaches (its `half`),
 * the line from there to what it strikes, how far along that line its blow is, and how large
 * it lands. */
interface Way {
  middle: Point;
  half: Point;
  unit: Point;
  go: number;
  size: number;
}

type Strike = Extract<Act, { verb: "strike" }>;

/** The way of the strike `act` from where its creature is, if both it and what it strikes
 * are on the table. */
function wayOf(
  act: Strike,
  scene: Pick<Scene, "layout" | "table" | "placed" | "beat">,
): Way | undefined {
  const { layout, placed } = scene;
  const striker = placed(act.source);
  if (!striker) return undefined;
  const target = act.to.kind === "object" ? placed(act.to.slot) : undefined;
  const life =
    act.to.kind === "player"
      ? layout.anchors(scene.table).life(act.to.player)
      : undefined;
  const at = life?.middle ?? (target && middleOf(layout, target));
  if (!at) return undefined;
  // The blow is along the line between their middles, where the two cards overlap by a
  // little on whichever side they meet. Two that strike each other meet half way.
  const middle = middleOf(layout, striker);
  const line = away(middle, at);
  const far = Math.hypot(line.x, line.y) || 1;
  const unit = { x: line.x / far, y: line.y / far };
  const { width } = striker.pose;
  const mine = layout.half(striker.resting, striker.pose);
  const art = life ? life.middle.y - life.corner.y : 0;
  const theirs = life
    ? { x: art * FACE, y: art * FACE }
    : layout.half(target!.resting, target!.pose);
  // What is much smaller than it, it shrinks toward the size of and lands on.
  const long = (half: Point) => Math.max(half.x, half.y);
  const ratio = (life ? art : long(theirs)) / long(mine);
  const { from, to, least } = STRIKE.small;
  const dive = Math.min(1, Math.max(0, (from - ratio) / (from - to)));
  const size = 1 - dive * (1 - least);
  // How far apart their middles are with each side's edges `into` the other's.
  const apart = (into: number) =>
    Math.min(
      ...(["x", "y"] as const).map((side) =>
        unit[side] === 0
          ? Infinity
          : (mine[side] * size + theirs[side] - into) / Math.abs(unit[side]),
      ),
    );
  // On a player it stops at the rim of the orb's face, which the total is read on, or
  // where it dives, with its edge short of the number in the orb's middle.
  const bitten = apart(life ? 0 : STRIKE.bite * width * size);
  const landed = life ? apart(Math.min(theirs.x, theirs.y) * INTO) : 0;
  const near = bitten + (landed - bitten) * dive;
  const met = scene.beat.some((other) => meets(other, act));
  const go = Math.max(STRIKE.least * width, (far - near) / (met ? 2 : 1));
  return { middle, half: mine, unit, go, size };
}

/** A step of a beat laid out in time (stage.ts `#beat`). */
interface Span {
  turn: number;
  from: number;
  to: number;
  step: { act: Act };
}

/** Where the middle of the card that goes `way` is at stage time `at`, its strike being
 * played over `span`: where it rests before and after, and its pull back left out. */
function middleAt(way: Way, span: Span, at: number): Point {
  const progress = (at - span.from) / (span.to - span.from);
  const [toward] = struck(Math.min(1, Math.max(0, progress)));
  return {
    x: way.middle.x + way.unit.x * way.go * toward,
    y: way.middle.y + way.unit.y * way.go * toward,
  };
}

/** How often a strike is looked at for what is in its way, and by how much it is held back
 * at a time, in stage ms. */
const LOOK = { every: 1000 / 60, wait: 25 };

/** Whether a point that goes in a straight line from `from` to `to` passes inside the box
 * that reaches `half` about the origin (the slab test). */
function crosses(from: Point, to: Point, half: Point): boolean {
  let [enter, leave] = [0, 1];
  for (const side of ["x", "y"] as const) {
    const move = to[side] - from[side];
    if (move === 0) {
      if (Math.abs(from[side]) >= half[side]) return false;
      continue;
    }
    const a = (-half[side] - from[side]) / move;
    const b = (half[side] - from[side]) / move;
    enter = Math.max(enter, Math.min(a, b));
    leave = Math.min(leave, Math.max(a, b));
    if (enter >= leave) return false;
  }
  return true;
}

/** Whether the strike that goes `way` over `span`, held back by `wait`, meets the earlier
 * one that goes `other` over `theirs` before that one is home (`STRIKE.home` of it): at some
 * time one of the two cards lies over the middle of the other, which is more than brushing
 * past it. Between two looks each card is taken to go straight, so a dash that crosses a
 * card in less than one look still meets it (Arena's takes two or three frames). */
function meetsOnTheWay(
  [way, span]: [Way, Span],
  [other, theirs]: [Way, Span],
  wait: number,
): boolean {
  const home = theirs.from + STRIKE.home * (theirs.to - theirs.from);
  const apart = (at: number): Point => {
    const mine = middleAt(way, span, at - wait);
    const its = middleAt(other, theirs, at);
    return { x: mine.x - its.x, y: mine.y - its.y };
  };
  let was = apart(span.from + wait);
  for (let at = span.from + wait; at < home; at += LOOK.every) {
    const now = apart(Math.min(at + LOOK.every, home));
    if (crosses(was, now, way.half) || crosses(was, now, other.half)) {
      return true;
    }
    was = now;
  }
  return false;
}

/** Holds back each strike of the beat `runs` that would pass over a creature of an earlier
 * strike still on its way out or back, and every step after it as long, until it no longer
 * does: no blow is dealt across a fight still drawing apart, to be taken for a blow at it.
 * `resting` is how the table holds a name as the beat begins: no strike changes where a
 * card rests. */
export function awaited(
  runs: Span[],
  layout: StageLayout,
  table: PlayerView,
  resting: (name: Name) => Resting | undefined,
) {
  const placed = (name: Name) => {
    const lies = resting(name);
    return lies && { resting: lies, pose: lies.pose };
  };
  const beat = runs.map((run) => run.step.act);
  const ways = beat.map((act) =>
    act.verb === "strike"
      ? wayOf(act, { layout, table, placed, beat })
      : undefined,
  );
  runs.forEach((run, nth) => {
    const way = ways[nth];
    // The blow dealt back goes with the strike it answers.
    if (!way || meets(run.step.act, beat[nth - 1])) return;
    const earlier = runs.slice(0, nth).flatMap((span, i): [Way, Span][] => {
      const theirs = ways[i];
      return theirs ? [[theirs, span]] : [];
    });
    let wait = 0;
    while (earlier.some((other) => meetsOnTheWay([way, run], other, wait))) {
      wait += LOOK.wait;
    }
    for (const later of wait > 0 ? runs.slice(nth) : []) {
      later.turn += wait;
      later.from += wait;
      later.to += wait;
    }
  });
}

/** The own motions of the step `act`, played with `ease`: none for most verbs. */
export function entering(
  act: Act,
  ease: (progress: number) => number,
  scene: Scene,
): Entering[] {
  const made = enters(act, ease, scene);
  return made === undefined ? [] : [made].flat();
}

function enters(
  act: Act,
  ease: (progress: number) => number,
  scene: Scene,
): Entering | Entering[] | undefined {
  const { layout, placed, was } = scene;
  switch (act.verb) {
    case "emerge": {
      // Out of its source, wherever that is now; with none on the table, it is on the
      // stack at once.
      const name = abilityName(act.id);
      const ability = placed(name);
      const slot = ability?.resting.source?.slot;
      const source = slot === undefined ? undefined : placed(slot);
      if (!ability || !source) return undefined;
      const start = layout.emerging(source.pose, ability.resting.pose);
      const mine = ability.resting.ability?.controller !== "opponent";
      const toward = mine ? 1 : -1;
      const bow = bowOf(act, toward);
      return { name, track: own(glide(start, ease, true, { bow })) };
    }
    case "showcase": {
      // Its face comes to be seen in the Revealed Cards pile, growing where the layout
      // rests it, as Arena's does (`I3Te0m1bBFw 22:08.4`). The card itself lies where it
      // was.
      const name = revealedName(act.object.slot);
      const face = placed(name);
      if (!face) return undefined;
      return { name, track: own(shownUp(sized(layout, face.resting), ease)) };
    }
    case "appear":
      // No flight, wherever it was before.
      return placed(act.object.slot)
        ? { name: act.object.slot, track: own(fade(ease)) }
        : undefined;
    case "fly": {
      // Your draw flies across the table to the place it is held up at, to be read, and
      // drops from there into your hand; one of several goes the same way and passes that
      // place without stopping (Arena's: `vPdoSA5H5xU 17:46.2`, `WQiYBv23buw 29:14.2`).
      const card = placed(act.object.slot);
      if (!card || !scene.moving || !drawsToRead(act)) return undefined;
      const several = scene.beat.some((o) => o !== act && drawsToRead(o));
      const back = card.pose.face === "back";
      const way = several
        ? swept(card.pose, ease, layout.drawn)
        : heldUp(card.pose, ease, layout.drawn);
      const turn = several ? DRAW_TURN.at : HELD_TURN;
      return {
        name: act.object.slot,
        track: { ...own(way), turn: back ? turn : undefined },
      };
    }
    case "tap": {
      // Only a permanent that lay the other way turns: one that arrives tapped does not.
      const card = placed(act.object.slot);
      const lay = was?.resting.object;
      if (!card || !was || lay?.zone !== "battlefield") return undefined;
      if (lay.tapped === act.object.tapped) return undefined;
      // Dressed as it now lies and turned a quarter back, its middle where it was; and as it
      // now rests, with that middle still there, which it turns to.
      const before = layout.middle(was.resting, was.pose);
      const held = (pose: Pose): Pose => {
        const now = layout.middle(card.resting, pose);
        return {
          ...pose,
          x: pose.x + before.x - now.x,
          y: pose.y + before.y - now.y,
        };
      };
      const unturned = {
        ...held(was.pose),
        turn: act.object.tapped ? -90 : 90,
      };
      const home = held(card.resting.pose);
      return {
        name: act.object.slot,
        track: own(turned(unturned, home, ease)),
      };
    }
    case "strike": {
      const striker = placed(act.source);
      const way = wayOf(act, scene);
      if (!striker || !way) return undefined;
      const target = act.to.kind === "object" ? placed(act.to.slot) : undefined;
      const { width } = striker.pose;
      const by = (length: number) => ({
        x: way.unit.x * length,
        y: way.unit.y * length,
      });
      const about = sized(layout, striker.resting);
      const nth = scene.beat.indexOf(act);
      const answers = meets(act, scene.beat[nth - 1]);
      const deep = depths(answers ? nth - 1 : nth, answers);
      // What a blow does to a card shows as it lands: until then the card it strikes is
      // drawn from the record the step found, and so is one that strikes back, which the
      // other's step has covered already.
      const veiled = was !== undefined && was.resting !== striker.resting;
      const lunging: Entering = {
        name: act.source,
        track: {
          ...own(
            lunge(
              by(way.go),
              by(STRIKE.back * width),
              way.size,
              // Arena's leans toward the side what it strikes is on, about 15 degrees
              // (IEXcjBn3BXk 22:24.97 to its left, 35:35.05 to its right).
              way.unit.x < 0 ? -STRIKE.lean : STRIKE.lean,
              about,
              deep,
            ),
          ),
          turn: veiled ? STRIKE.hit : undefined,
        },
        cover: veiled ? was.resting : undefined,
      };
      const lay = target?.resting;
      const before =
        lay?.object &&
        scene.found.objects.find((o) => o.slot === lay.object.slot);
      if (!lay?.object || !before) return lunging;
      const struck: Entering = {
        name: lay.object.slot,
        track: { pose: stays, turn: STRIKE.hit },
        cover: { ...lay, object: before },
        rides: true,
      };
      return [lunging, struck];
    }
    case "flip": {
      // Only a permanent turns over where it lies (UI Conventions, "A card turning face up").
      const card = placed(act.object.slot);
      const lay = was?.resting.object;
      if (!card || !was || lay?.zone !== "battlefield") return undefined;
      if (act.object.zone !== "battlefield") return undefined;
      const about = sized(layout, card.resting);
      return {
        name: act.object.slot,
        track: { ...own(turnedOver(was.pose, about)), turn: FLIP.turn },
        cover: { ...was.resting, pose: card.resting.pose },
      };
    }
    default:
      return undefined;
  }
}

/** `resting` drawn as a card back. */
export const backed = (resting: Resting): Resting => ({
  ...resting,
  dress: { ...resting.dress, back: true },
});

/** The face a card still shows at stage time `at` as `track` takes it where it goes, if
 * `track` is not its own flight: the face a `lift` let it go with (tracks.ts `carried`).
 * The stage keeps it when a later step of the beat moves that place again
 * (`showingOn`). */
export const stillShown = (
  covered: Resting | undefined,
  track: Track | undefined,
  at: number,
): Resting | undefined =>
  covered && !covered.dress.back && !track?.own && turning(track, at)
    ? covered
    : undefined;

/** The track a card goes on, and what it is drawn as until it is there: the face `kept`
 * that `carried` gave it, or else the face it was `showing` (`stillShown`) until the track
 * ends. Without this, the rest of a Stock Up, let go to the bottom of the library, took
 * the pile's edge-only look as soon as the first card kept flew to the hand, and crossed
 * the table as black slabs. */
export const showingOn = (
  track: Track,
  kept: Resting | undefined,
  showing: Resting | undefined,
): [Track, Resting | undefined] =>
  kept || !showing ? [track, kept] : [{ ...track, turn: 1 }, showing];
