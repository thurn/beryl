// The motion a step gives what it moves where that is no glide from where it was (stage.ts,
// `#begin`): an ability lifting out of its source, a token fading in, a draw held up to be
// read, a permanent turning as it taps, a creature's lunge, a card turning over in place. Each
// is made from the model as the step begins (where things are and how they are dressed, and
// how they were before the step's act), whatever the scripts before it were and however they
// were cut: nothing here reads a view. The geometry is the layout's.
import type { Act, PlayerView } from "../generated/protocol";
import {
  abilityName,
  type Name,
  type Point,
  type Resting,
  type StageLayout,
} from "./layout.ts";
import type { Pose } from "./pose.ts";
import {
  drawsToRead,
  fade,
  FLIP,
  glide,
  HELD_TURN,
  heldUp,
  lunge,
  meets,
  STRIKE,
  turned,
  turnedOver,
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
      return { name, track: own(glide(start, ease, true)) };
    }
    case "appear":
      // No flight, wherever it was before.
      return placed(act.object.slot)
        ? { name: act.object.slot, track: own(fade(ease)) }
        : undefined;
    case "fly": {
      // Your draw is held up to be read on its way, unless it is one of several: Arena's
      // three cards of one spell pass at their own size (`WQiYBv23buw 29:14.2`).
      const card = placed(act.object.slot);
      const several = scene.beat.some((o) => o !== act && drawsToRead(o));
      if (!card || !scene.moving || several || !drawsToRead(act)) {
        return undefined;
      }
      return {
        name: act.object.slot,
        track: {
          ...own(heldUp(card.pose, ease, layout.drawn)),
          turn: card.pose.face === "back" ? HELD_TURN : undefined,
        },
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
      const way = away(middleOf(layout, striker), at);
      const far = Math.hypot(way.x, way.y) || 1;
      const unit = { x: way.x / far, y: way.y / far };
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
              : (mine[side] * size + theirs[side] - into) /
                Math.abs(unit[side]),
          ),
        );
      // On a player it stops at the rim of the orb's face, which the total is read on, or
      // with its edge on the orb's middle where it dives.
      const bitten = apart(life ? 0 : STRIKE.bite * width * size);
      const landed = life ? apart(Math.min(theirs.x, theirs.y)) : 0;
      const near = bitten + (landed - bitten) * dive;
      const met = scene.beat.some((other) => meets(other, act));
      const go = Math.max(STRIKE.least * width, (far - near) / (met ? 2 : 1));
      const by = (length: number) => ({
        x: unit.x * length,
        y: unit.y * length,
      });
      const about = sized(layout, striker.resting);
      // What a blow does to a card shows as it lands: until then the card it strikes is
      // drawn from the record the step found, and so is one that strikes back, which the
      // other's step has covered already.
      const veiled = was !== undefined && was.resting !== striker.resting;
      const lunging: Entering = {
        name: act.source,
        track: {
          ...own(lunge(by(go), by(STRIKE.back * width), size, about)),
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
