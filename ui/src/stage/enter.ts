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
  /** The table once the step's act is applied. */
  table: PlayerView;
  /** What the stage holds under `name`, at rest as the act leaves it and where it is now. */
  placed(name: Name): Placed | undefined;
  /** What the step moves (tracks.ts `moverOf`), as it was before the act. */
  was: Placed | undefined;
  /** The layout moved where the step's mover rests, and it is gliding there. */
  moving: boolean;
  /** Another step of the beat is a draw to be read. */
  several: boolean;
}

/** A track for the node `name`, which shows as `cover` until the track turns it over. */
export interface Entering {
  name: Name;
  track: Omit<Track, "from" | "to">;
  cover?: Resting;
}

const own = (pose: Track["pose"]) => ({ pose, own: true });

const away = (from: Point, to: Point): Point => ({
  x: to.x - from.x,
  y: to.y - from.y,
});

/** The own motion of the step `act`, played with `ease`, if it has one. */
export function entering(
  act: Act,
  ease: (progress: number) => number,
  scene: Scene,
): Entering | undefined {
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
      if (!card || !scene.moving || scene.several || !drawsToRead(act)) {
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
      // Dressed as it now lies and turned a quarter back, its middle where it was.
      const before = layout.middle(was.resting, was.pose);
      const now = layout.middle(card.resting, was.pose);
      const unturned: Pose = {
        ...was.pose,
        x: was.pose.x + before.x - now.x,
        y: was.pose.y + before.y - now.y,
        turn: act.object.tapped ? -90 : 90,
      };
      return { name: act.object.slot, track: own(turned(unturned, ease)) };
    }
    case "strike": {
      const striker = placed(act.source);
      if (!striker) return undefined;
      const target = act.to.kind === "object" ? placed(act.to.slot) : undefined;
      const at =
        act.to.kind === "player"
          ? layout.anchors(scene.table).life(act.to.player).middle
          : target && layout.middle(target.resting, target.pose);
      if (!at) return undefined;
      const from = layout.middle(striker.resting, striker.pose);
      return { name: act.source, track: own(lunge(away(from, at))) };
    }
    case "flip": {
      const card = placed(act.object.slot);
      if (!card || !was) return undefined;
      const about = (pose: Pose, size: number): Pose => {
        const middle = layout.middle(card.resting, pose);
        return {
          ...pose,
          x: middle.x + (pose.x - middle.x) * size,
          y: middle.y + (pose.y - middle.y) * size,
          width: pose.width * size,
        };
      };
      return {
        name: act.object.slot,
        track: { ...own(turnedOver(was.pose, ease, about)), turn: FLIP.turn },
        cover: { ...was.resting, pose: card.resting.pose },
      };
    }
    default:
      return undefined;
  }
}
