// The phone's layout for the stage: mobile/layout.ts's bands, read as resting poses and anchors
// on a board the size of `viewport`. The recordings are of the desktop alone, so everything a
// phone plays is the desktop's verbs between the phone's own places (UI Conventions, "The
// phone"). Besides the cards mobile/layout.ts places (the battlefield, your hand, the stack),
// everything a step can move rests somewhere: the opponent's hand as backs at the top edge,
// and the top card of each library and graveyard on its pile in its player's strip, the rest
// of a pile unseen beneath it. The phone draws no exile pile and no emblem yet: an exiled card
// rests unseen under its owner's graveyard, unless a permanent holds it, and an emblem's
// ability has no source to lift out of. A card exiled until a permanent leaves, or with its
// ability, is tucked under that permanent as the desktop's is (UI Conventions, exception 6).
import type { ObjectView, PlayerView, Side } from "../generated/protocol";
import { CARD_ASPECT } from "../components/Card";
import { BENEATH_STEP } from "../game/placement";
import { held as holder } from "../game/zoneViews";
import {
  BACKS,
  backSpot,
  phoneLayout,
  PILE,
  pileSpot,
  RATIO,
  SIDE,
  STACK_CARD,
  type Spot,
  type Viewport,
} from "../mobile/layout";
import { drawn, emerging, half, middle } from "./desktop.ts";
import {
  abilityName,
  type Anchors,
  type Dress,
  type Name,
  type Place,
  type Resting,
  type StageLayout,
} from "./layout.ts";
import type { Pose } from "./pose.ts";
import { fan, type PileSpace } from "./revealed.ts";
import type { Table } from "./table.ts";
import { laidOut, type TraySpace } from "./tray.ts";

/** What each kind of thing at rest is drawn over, all under what flies (pose.ts ALOFT): the
 * piles, the opponent's hand, the permanents in the order their rows overlap, your hand, and
 * the stack. */
const DEPTH = {
  pile: 1,
  backs: 2,
  field: 10,
  hand: 600,
  stack: 700,
  tray: 850,
};

/** Where the cards a prompt holds up lie on a phone the size of `viewport`: over the
 * opponent's rows, and the second of two piles over the player's own, so that the middle
 * band and what stands in it (the stack, the way back) stay clear; at the size of a card of
 * the hand, smaller where there are many. */
export function phoneTray(view: PlayerView, viewport: Viewport): TraySpace {
  const { bands } = phoneLayout(view, viewport);
  const [x, width] = [SIDE, viewport.width - 2 * SIDE];
  const top = bands.opponentLands.top + 4;
  const foot = bands.middle.top + bands.middle.height + 4;
  return {
    x,
    y: top,
    width,
    height: bands.middle.top - 4 - top,
    card: { most: 104, least: 54, aspect: CARD_ASPECT },
    gap: 6,
    heading: 18,
    // Clear of what the prompt asks, which is written over the player's strip.
    second: { x, y: foot, width, height: bands.yourStrip.top - 40 - foot },
  };
}

/** Where the Revealed Cards lie on a phone: in the opponent's strip beside their graveyard,
 * at a pile's size, short of their life total. */
export const phoneRevealed = (
  view: PlayerView,
  viewport: Viewport,
): PileSpace => ({
  ...pileSpot(phoneLayout(view, viewport).bands, "opponent", "graveyard"),
  x: PILE.graveyard + PILE.width + 12,
  step: 8,
  reach: 24,
  depth: DEPTH.backs,
  dress: { look: "pile", frame: PILE.width },
});

/** The life orb's size (phone.css `.phone-orb`: Player.tsx's 272 by 116 art at 0.37px). */
const ORB = { width: 272 * 0.37, height: 116 * 0.37 };

const upright = (place: Place, depth: number, face: Pose["face"]): Pose => ({
  x: place.x,
  y: place.y,
  width: place.width,
  rotation: 0,
  skew: 0,
  turn: 0,
  opacity: 1,
  depth,
  face,
});

/** Cards lifted off a library: how far from its pile they lie, and the step between them. */
const LIFTED = { gap: 4, step: 12 };
const dressed = (
  zone: Dress["zone"],
  look: Dress["look"],
  frame: number,
  back = false,
): Dress => ({ zone, look, frame, back, rim: false });

const faceOf = (object: ObjectView): Pose["face"] =>
  object.name === null || object.facing === "down" ? "back" : "front";

/** A hand card's pose in the fan. The fan turns a card about its bottom center; a pose turns
 * about its top-left corner, which that turn carries here. */
function fanned(spot: Spot, depth: number, face: Pose["face"]): Pose {
  const turn = (spot.rotate * Math.PI) / 180;
  const pivot = { x: spot.width / 2, y: spot.width / CARD_ASPECT };
  return {
    ...upright(spot, depth, face),
    x: spot.x + pivot.x - pivot.x * Math.cos(turn) + pivot.y * Math.sin(turn),
    y: spot.y + pivot.y - pivot.x * Math.sin(turn) - pivot.y * Math.cos(turn),
    rotation: spot.rotate,
  };
}

/** The pile a card outside the battlefield, the hands and the stack rests on. */
const pileOf = (zone: ObjectView["zone"]) =>
  zone === "library" ? "library" : "graveyard";

function rest(
  { view, abilities, handled, tray, revealed }: Table,
  viewport: Viewport,
  left?: ReadonlySet<Name>,
) {
  // A card a prompt holds up is off its pile as one still leaving for it is.
  const leaving = tray
    ? new Set([...(left ?? []), ...tray.piles.flatMap((pile) => pile.cards)])
    : left;
  const { bands, cards, stack } = phoneLayout(view, viewport);
  const objects = new Map(view.objects.map((o) => [o.slot, o]));
  const out = new Map<Name, Resting>();
  // Permanents lie over one another in the order of their rows; nothing else overlaps them.
  const rows = cards
    .filter((spot) => spot.zone === "battlefield")
    .sort((a, b) => a.z - b.z);
  // Each a depth apart from the next, for what it holds in exile to lie between.
  rows.forEach((spot, rank) => {
    const object = objects.get(spot.slot)!;
    out.set(spot.slot, {
      object,
      pose: upright(spot, DEPTH.field + 2 * rank + 1, faceOf(object)),
      dress: dressed("battlefield", "field", spot.width),
    });
  });
  // What a permanent holds in exile peeks out from under it, each a step further to the left
  // and toward its controller.
  const onField = new Set(view.battlefield);
  const holding = new Map<number, number>();
  for (const object of view.objects) {
    const host = holder(object, onField);
    const under = host === undefined ? undefined : out.get(host);
    if (!under?.object) continue;
    const n = (holding.get(host!) ?? 0) + 1;
    holding.set(host!, n);
    const { pose } = under;
    const toward = under.object.controller === "you" ? 1 : -1;
    const height = pose.width / CARD_ASPECT;
    out.set(object.slot, {
      object,
      pose: {
        ...upright(pose, pose.depth - 1, faceOf(object)),
        x: pose.x - n * pose.width * BENEATH_STEP.x,
        y: pose.y + toward * n * height * BENEATH_STEP.y,
      },
      dress: dressed("exile", "beneath", pose.width),
    });
  }
  let held = 0;
  for (const spot of cards) {
    if (spot.zone !== "hand") continue;
    const object = objects.get(spot.slot)!;
    out.set(spot.slot, {
      object,
      pose: fanned(spot, DEPTH.hand + held++, faceOf(object)),
      dress: dressed("hand", "hand", spot.width),
    });
  }
  const theirs = view.objects.filter(
    (o) => o.zone === "hand" && o.owner === "opponent",
  );
  theirs.forEach((object, i) => {
    const place = backSpot(bands, viewport.width, i, theirs.length);
    out.set(object.slot, {
      object,
      pose: upright(place, DEPTH.backs + i, "back"),
      dress: dressed("hand", "held", BACKS.width, true),
    });
  });
  // The cards the opponent is looking at lie beside their library, toward the table, face
  // down at its size (UI Conventions, "Cards the opponent looks at").
  const handling = view.handling;
  if (handling?.zone === "library") {
    const spot = pileSpot(bands, handling.side, "library");
    const toward = handling.side === "you" ? -1 : 1;
    (handled ?? []).forEach((slot, i) => {
      const object = objects.get(slot);
      if (object?.zone !== "library") return;
      const place = {
        ...spot,
        x: spot.x + i * LIFTED.step,
        y: spot.y + toward * (PILE.height + LIFTED.gap),
      };
      out.set(slot, {
        object,
        pose: upright(place, DEPTH.backs + i, "back"),
        dress: dressed("library", "pile", PILE.width, true),
      });
    });
  }
  for (const object of view.objects) {
    if (out.has(object.slot) || object.zone === "stack") continue;
    const pile = pileOf(object.zone);
    // A pile's top is the last card that has arrived: one still leaving for it is seen
    // where it was, and the pile shows the top it had until that one lands.
    const arrived = view.players[object.owner][pile].filter(
      (slot) => !leaving?.has(slot),
    );
    const top = object.zone === pile && arrived.at(-1) === object.slot;
    const back = pile === "library";
    out.set(object.slot, {
      object,
      pose: upright(
        pileSpot(bands, object.owner, pile),
        top ? DEPTH.pile : 0,
        back ? "back" : faceOf(object),
      ),
      dress: dressed(object.zone, top ? "pile" : "hidden", PILE.width, back),
    });
  }
  // As the desktop's: an ability the table has no number for cannot be named by a later
  // step, so it is not drawn.
  let numbered = 0;
  for (const spot of stack) {
    const pose = upright(spot, DEPTH.stack + spot.index, "front");
    const dress = dressed("stack", "stack", spot.width);
    if (spot.item.kind === "spell") {
      const object = objects.get(spot.item.slot);
      if (object) out.set(object.slot, { object, pose, dress });
      continue;
    }
    const id = abilities[numbered++];
    if (id === undefined) continue;
    const source =
      spot.item.source === null ? undefined : out.get(spot.item.source)?.object;
    out.set(abilityName(id), { ability: spot.item, source, pose, dress });
  }
  fan(out, revealed, phoneRevealed(view, viewport));
  // The cards a prompt holds up, face up where it lays them out: each the node of its card.
  let depth = DEPTH.tray;
  const laid = tray ? laidOut(tray, phoneTray(view, viewport)).cards : [];
  for (const [slot, place] of laid) {
    const lies = out.get(slot);
    if (!lies?.object) continue;
    const face = faceOf(lies.object);
    out.set(slot, {
      object: lies.object,
      pose: upright(place, depth++, face),
      dress: dressed(lies.dress.zone, "hand", place.width, face === "back"),
    });
  }
  return out;
}

function anchors(view: PlayerView, viewport: Viewport): Anchors {
  const { bands } = phoneLayout(view, viewport);
  const middle = bands.middle.top + bands.middle.height / 2;
  const life = (side: Side) => {
    const strip = side === "you" ? bands.yourStrip : bands.opponentStrip;
    const middle = {
      x: viewport.width / 2,
      y: strip.top + strip.height / 2,
    };
    return {
      corner: { x: middle.x - ORB.width / 2, y: middle.y - ORB.height / 2 },
      middle,
    };
  };
  return {
    pile: (side, zone) => pileSpot(bands, side, pileOf(zone)),
    // The stack's cards stand about the middle of its band, wherever the band's edges are.
    stack: (index, count) => ({
      x:
        SIDE +
        (viewport.width - 2 * SIDE - STACK_CARD.width) / 2 +
        (index - (count - 1) / 2) * STACK_CARD.step,
      y: middle - STACK_CARD.width / RATIO / 2,
      width: STACK_CARD.width,
    }),
    life,
    // A phone shows no unspent mana: it is its player's.
    pool: (side) => life(side).middle,
  };
}

/** The table as a phone lays it out, on a board the size of `viewport`. An ability lifts out
 * of its source as the desktop's does. */
export const phone = (viewport: Viewport): StageLayout => ({
  rest: (table, leaving) => rest(table, viewport, leaving),
  anchors: (view) => anchors(view, viewport),
  emerging,
  drawn,
  middle,
  half,
  // The phone's scans wear no combat marks (mobile/faces.tsx).
  chevron: 0,
});
