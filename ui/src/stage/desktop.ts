// The desktop's layout for the stage: game/layout.ts's table, read as resting poses and
// anchors. It places every card; nothing here does.
import type { ObjectView, PlayerView } from "../generated/protocol";
import { CARD_ASPECT } from "../components/Card";
import type { HandHold } from "../game/handLayout";
import { layout, STACK_Z } from "../game/layout";
import { fromMiddle } from "../game/perspective";
import { FULL_WIDTH, type Placement } from "../game/placement";
import { stackPlace } from "../game/stackLayout";
import { MANA_POOLS } from "../game/tableRows";
import {
  emblemObjects,
  emblemSlot,
  knownLibraries,
  zonePiles,
} from "../game/zoneViews";
import {
  abilityName,
  type Anchors,
  type Name,
  type Resting,
  type StackAbility,
  type StageLayout,
} from "./layout.ts";
import type { Pose } from "./pose.ts";
import type { Table } from "./table.ts";

/** Each life orb's corner on the base stage, held to the stage's middle, and its size
 * (screens.css `.table .life-orb`: Player.tsx's 272 by 116 art, a little smaller). */
const LIFE = { x: 670, y: { opponent: 48, you: 618 } };
const ORB = { width: 272 / 1.045, height: 116 / 1.045 };

/** No order of arrival is kept yet: a hand lies in its cards' slot order. */
const NO_ORDER: ReadonlyMap<number, number> = new Map();
const NONE: ReadonlySet<number> = new Set();

/** How much of its source's width an ability is as it lifts out of it (Board.tsx's
 * triggerOrigin). */
const EMERGING = 0.6;

const resting = (object: ObjectView, place: Placement): Resting => ({
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

/** The ability `item` at its place on the stack, `index` from the bottom of `count`, drawn as
 * a full frame of its source (components/Stack.tsx). */
const ability = (
  item: StackAbility,
  source: ObjectView | undefined,
  index: number,
  count: number,
): Resting => {
  const place = stackPlace(index, count);
  return {
    ability: item,
    source,
    pose: {
      x: place.x,
      y: place.y,
      width: place.width,
      rotation: 0,
      skew: 0,
      opacity: 1,
      depth: STACK_Z + index,
      face: "front",
    },
    dress: {
      zone: "stack",
      look: "stack",
      frame: FULL_WIDTH,
      back: false,
      rim: false,
    },
  };
};

/** The slot an ability on the stack lifts out of: its source, or for an emblem's ability,
 * which the view gives no source ([CR 114.4](https://yawgatog.com/resources/magic-rules/#R1144):
 * "Abilities of emblems function in the command zone"), the card the table draws for the
 * emblem of that name its controller has. */
function origin(view: PlayerView, item: StackAbility): number | undefined {
  if (item.source !== null) return item.source;
  const index = view.players[item.controller].emblems.indexOf(item.name);
  return index < 0 ? undefined : emblemSlot(item.controller, index);
}

/** What of the screen around the table its layout goes by. */
export interface Around {
  /** How the hand is held: tucked or away under a screen over the table. */
  hold: HandHold;
  /** The game log stands open in its column, and the permanents' rows keep clear of it. */
  logOpen: boolean;
}

function rest(
  { view, abilities }: Table,
  { hold, logOpen }: Around,
): Map<Name, Resting> {
  const placements = layout(
    view,
    hold,
    logOpen,
    {},
    NO_ORDER,
    NONE,
    zonePiles(view, NO_ORDER),
    [],
  );
  const out = new Map<Name, Resting>();
  for (const object of [...view.objects, ...emblemObjects(view)]) {
    const place = placements.get(object.slot);
    if (place) out.set(object.slot, resting(object, place));
  }
  // An ability the table has no number for (a stage that started over from a view in the
  // middle of a game) cannot be named by a later step, so it is not drawn.
  let numbered = 0;
  view.stack.forEach((item, index) => {
    if (item.kind !== "ability") return;
    const id = abilities[numbered++];
    if (id === undefined) return;
    const from = origin(view, item);
    const source = from === undefined ? undefined : out.get(from)?.object;
    out.set(abilityName(id), ability(item, source, index, view.stack.length));
  });
  return out;
}

/** A small copy in the middle of the source, upright and as wide as most of it. */
export function emerging(source: Pose, rest: Pose): Pose {
  const width = source.width * EMERGING;
  return {
    ...rest,
    x: source.x + (source.width - width) / 2,
    y: source.y + (source.width - width) / CARD_ASPECT / 2,
    width,
  };
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

/** The table as the desktop lays it out, on the stage as wide as it now is (game/Stage.tsx),
 * with what stands `around` it. */
export const desktopWith = (around: Around): StageLayout => ({
  rest: (table) => rest(table, around),
  anchors,
  emerging,
});

/** The desktop's table with nothing over it. */
export const desktop = desktopWith({ hold: "held", logOpen: false });
