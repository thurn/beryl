// The desktop's layout for the stage: game/layout.ts's table, read as resting poses and
// anchors. It places every card; nothing here does.
import type { PlayerView } from "../generated/protocol";
import {
  emblemObjects,
  fromMiddle,
  knownLibraries,
  layout,
  MANA_POOLS,
  stackPlace,
  zonePiles,
  type Placement,
} from "../game/layout";
import type { Anchors, Resting, StageLayout } from "./layout.ts";

/** Each life orb's corner on the base stage, held to the stage's middle, and its size
 * (screens.css `.table .life-orb`: Player.tsx's 272 by 116 art, a little smaller). */
const LIFE = { x: 670, y: { opponent: 48, you: 618 } };
const ORB = { width: 272 / 1.045, height: 116 / 1.045 };

/** No order of arrival is kept yet: a hand lies in its cards' slot order. */
const NO_ORDER: ReadonlyMap<number, number> = new Map();
const NONE: ReadonlySet<number> = new Set();

const resting = (object: Resting["object"], place: Placement): Resting => ({
  object,
  pose: {
    x: place.x,
    y: place.y,
    width: place.width * place.scale,
    rotation: place.rotate,
    skew: place.skew,
    opacity: place.opacity,
    depth: place.z,
    face:
      place.back || object.name === null || object.facing === "down"
        ? "back"
        : "front",
  },
  dress: {
    zone: place.zone,
    look: place.look,
    frame: place.width,
    back: place.back ?? false,
    rim: place.rim ?? false,
    count: place.count,
  },
});

function rest(table: PlayerView): Map<number, Resting> {
  const placements = layout(
    table,
    "held",
    false,
    {},
    NO_ORDER,
    NONE,
    zonePiles(table, NO_ORDER),
    [],
  );
  const out = new Map<number, Resting>();
  for (const object of [...table.objects, ...emblemObjects(table)]) {
    const place = placements.get(object.slot);
    if (place) out.set(object.slot, resting(object, place));
  }
  return out;
}

function anchors(table: PlayerView): Anchors {
  const piles = [...zonePiles(table, NO_ORDER), ...knownLibraries(table)];
  return {
    pile(side, zone) {
      const { x, y, width } = piles.find(
        (pile) => pile.side === side && pile.zone === zone,
      )!.look;
      return { x, y, width };
    },
    stack: stackPlace,
    life(side) {
      const corner = { x: fromMiddle(LIFE.x), y: LIFE.y[side] };
      return {
        corner,
        middle: { x: corner.x + ORB.width / 2, y: corner.y + ORB.height / 2 },
      };
    },
    pool: (side) => ({ x: MANA_POOLS[side].left, y: MANA_POOLS[side].top }),
  };
}

/** The table as the desktop lays it out, on the stage as wide as it now is (game/Stage.tsx). */
export const desktop: StageLayout = { rest, anchors };
