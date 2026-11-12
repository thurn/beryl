// The desktop's layout for the stage: game/layout.ts's table, read as resting poses and
// anchors. It places every card; nothing here does.
import type { ObjectView, PlayerView } from "../generated/protocol";
import { CARD_ASPECT } from "../components/Card";
import type { HandHold } from "../game/handLayout";
import { layout, STACK_Z } from "../game/layout";
import { fromMiddle } from "../game/perspective";
import { FULL_WIDTH, type Placement } from "../game/placement";
import { STACK, stackPlace } from "../game/stackLayout";
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
  type Point,
  type Resting,
  type StackAbility,
  type StageLayout,
} from "./layout.ts";
import type { Pose } from "./pose.ts";
import { fan, type PileSpace } from "./revealed.ts";
import type { Table, Tray } from "./table.ts";
import { laidOut, type TraySpace } from "./tray.ts";

/** Each life orb's corner on the base stage, held to the stage's middle, and its size
 * (screens.css `.table .life-orb`: Player.tsx's 272 by 116 art, a little smaller). */
const LIFE = { x: 670, y: { opponent: 48, you: 618 } };
const ORB = { width: 272 / 1.045, height: 116 / 1.045 };

/** Where the cards a prompt holds up lie: over the middle of the table, clear of the hands,
 * the dock and the stack, where what asks stays in sight; at the size of a card read in a
 * browser of Arena's (UI Conventions, "The local player's scry"), smaller where there are
 * many. */
export function desktopTray(): TraySpace {
  const x = fromMiddle(150);
  return {
    x,
    y: 152,
    width: STACK.x - 16 - x,
    height: 420,
    card: { most: 200, least: 104, aspect: CARD_ASPECT },
    gap: 14,
    heading: 30,
  };
}

/** What a card held up in a tray is drawn over: everything at rest, the Revealed Cards
 * among it, and under what flies (pose.ts ALOFT). */
const TRAY_DEPTH = 850;

/** `rest` with the cards of `tray` where it lays them out, face up at full frame: each is
 * the node of its card, whatever zone the table has it in. */
function held(rest: Map<Name, Resting>, tray: Tray) {
  let depth = TRAY_DEPTH;
  for (const [slot, place] of laidOut(tray, desktopTray()).cards) {
    const lies = rest.get(slot);
    if (!lies?.object) continue;
    const known = lies.object.name !== null;
    rest.set(slot, {
      object: lies.object,
      pose: {
        ...place,
        rotation: 0,
        skew: 0,
        turn: 0,
        opacity: 1,
        depth: depth++,
        face: known ? "front" : "back",
      },
      dress: {
        zone: lies.dress.zone,
        look: "shown",
        frame: FULL_WIDTH,
        back: !known,
        rim: false,
      },
    });
  }
}

/** Where the Revealed Cards lie: at the table's left under the opponent's badge and over the
 * game log's column, where Arena's do, at about a permanent's size; over the permanents, whose
 * rows may reach under them, and under the cards a prompt holds up. */
export const DESKTOP_REVEALED: PileSpace = {
  x: 24,
  y: 118,
  width: 92,
  step: 16,
  reach: 64,
  depth: 760,
  dress: { look: "shown", frame: FULL_WIDTH },
};

/** No order of arrival is kept yet: a hand lies in its cards' slot order. */
const NO_ORDER: ReadonlyMap<number, number> = new Map();
const NONE: ReadonlySet<Name> = new Set();

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
    turn: 0,
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
      turn: 0,
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
  /** The Revealed Cards the player dismissed: they lie nowhere. */
  cleared?: ReadonlySet<ObjectView>;
}

function rest(
  { view, abilities, handled, tray, revealed }: Table,
  { hold, logOpen, cleared }: Around,
  leaving: ReadonlySet<Name> = NONE,
): Map<Name, Resting> {
  // A card still leaving for a pile is off it as a borrowed one is: the pile lies as it did,
  // its top showing, and the card has its place on it for when it lands. So is a card a
  // prompt holds up.
  const away = tray
    ? new Set([...leaving, ...tray.piles.flatMap((pile) => pile.cards)])
    : leaving;
  const placements = layout(
    view,
    hold,
    logOpen,
    {},
    NO_ORDER,
    // An ability's name is no card's slot.
    away as ReadonlySet<number>,
    zonePiles(view, NO_ORDER),
    handled ?? [],
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
  fan(out, revealed, DESKTOP_REVEALED, cleared);
  if (tray) held(out, tray);
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

/** How large a card you draw is held up, against its size in your hand: Arena's 1.55 and
 * 1.59 (data/arena-motion.json, fly, "draw"). */
const DRAWN = 1.57;

/** Upright over where it will lie, its foot a quarter of the way down the card it becomes:
 * Arena's drops straight down into the hand (`UrGalw_f35w 16:52.4`). */
export function drawn(rest: Pose): Pose {
  const width = rest.width * DRAWN;
  return {
    ...rest,
    x: rest.x - (width - rest.width) / 2,
    y: rest.y + (rest.width / 4 - width) / CARD_ASPECT,
    width,
    rotation: 0,
    skew: 0,
  };
}

const radians = (degrees: number) => (degrees * Math.PI) / 180;

/** Half the width and half the height of its card: of the frame, which a tapped permanent's
 * lies turned a quarter in (components/Card.tsx's slot). */
export function half({ object, dress }: Resting, pose: Pose): Point {
  const lies = dress.zone === "battlefield" && object?.tapped === true;
  const long = pose.width / CARD_ASPECT;
  return {
    x: (lies ? long : pose.width) / 2,
    y: (lies ? pose.width : long) / 2,
  };
}

/** The middle of its card, leaned and then turned about the pose's corner as the node is
 * (pose.ts `transformOf`). */
export function middle(resting: Resting, pose: Pose): Point {
  const { x, y: down } = half(resting, pose);
  const across = x + Math.tan(radians(pose.skew)) * down;
  const turn = radians(pose.rotation);
  return {
    x: pose.x + across * Math.cos(turn) - down * Math.sin(turn),
    y: pose.y + across * Math.sin(turn) + down * Math.cos(turn),
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

/** How far past its card's edge the tip of an attacker's chevron is, in the card's widths
 * (design/components/card-marks-and-piles.css `.attack-chevron`: 0.15em clear of the frame
 * and 1.9em tall, on a card 12em wide). */
const CHEVRON = 2 / 12;

/** The table as the desktop lays it out, on the stage as wide as it now is (game/Stage.tsx),
 * with what stands `around` it. */
export const desktopWith = (around: Around): StageLayout => ({
  rest: (table, leaving) => rest(table, around, leaving),
  anchors,
  emerging,
  drawn,
  middle,
  half,
  chevron: CHEVRON,
});

/** The desktop's table with nothing over it. */
export const desktop = desktopWith({ hold: "held", logOpen: false });
