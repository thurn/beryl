// Where the hand's cards are drawn: the fan and the opponent's held cards.
import type { ObjectView } from "../generated/protocol";
import { CARD_ASPECT } from "../components/Card";
import type { Box, Placement } from "./placement";
import { onArc } from "./fanArc";
import { BACK_WIDTH, FULL_WIDTH, HAND, RAISE, turned } from "./placement";
import { stackPlace } from "./stackLayout";
import { STAGE, STAGE_WIDTHS, fromMiddle } from "./perspective";

/** Where the action dock's leftmost button begins, passing on to the next stop left of the turn
 * button: the dock stands 39 from the right edge (screens.css' .dock), the turn button
 * --button-width wide and the round one --button-height, --button-gap apart (tokens.css). */
const dockLeft = () => STAGE.width - 39 - 250 - 10 - 48;

/** The held fan's middle, between its outer cards' centers: its outer card, raised (RAISE), and
 * the glow of a playable card's outline stay clear of the dock (dockLeft). */
const heldRoom = () =>
  2 * (dockLeft() - 12 - STAGE.width / 2 - (HAND.width * RAISE.scale) / 2);

/** How far the hand sinks under a screen over the table (scry.png). */
const HAND_TUCK = 67;

/** The hand raised for a choice of its cards (a discard): the whole fan lifted into view and
 * larger, on the same circle as the held hand (`onArc`), each card whole above your life orb
 * (its top at y 625) and clear of the action dock at the right (from x 1310): its middle `room`
 * wide between the outer cards' centers. */
export const RAISED_HAND = {
  top: 325,
  scale: 1.1,
  spacing: 214,
  // The dock is held to the stage's right edge and the fan to its middle, so each half of the
  // fan gains or loses half of what the stage does.
  get room() {
    return 940 + STAGE.width - STAGE_WIDTHS.base;
  },
};

/** How the hand is held: at the near edge; `tucked`, sunk out of the way under a screen over
 * the table; `away`, below the stage while a screen shows the hand itself (the opening hand),
 * so it is not seen twice; `raised`, lifted whole into view for a choice of its cards. */
export type HandHold = "held" | "tucked" | "away" | "raised";

/** How far below the near edge the hand waits while held `away`. */
const HAND_AWAY = 420;

/** The hand fan, held at the near edge: the more cards, the closer they sit, and each card's
 * top center lies on one circle (`onArc`), so a crowded fan turns and sinks less at its ends
 * than a spread one; tucked, the fan sinks out of the way; raised, it rises whole into view. */
export function handPlacements(
  cards: ObjectView[],
  hold: HandHold,
  stackCount: number,
): Placement[] {
  const n = cards.length;
  const mid = (n - 1) / 2;
  const raised = hold === "raised";
  const fan = raised ? RAISED_HAND : { ...HAND, scale: 1, room: heldRoom() };
  const width = FULL_WIDTH * fan.scale;
  const spacing = n > 1 ? Math.min(fan.spacing, fan.room / (n - 1)) : 0;
  const places = cards.map((o, i): Placement => {
    const dx = (i - mid) * spacing;
    const { sink, turn } = onArc(dx, width);
    const left = STAGE.width / 2 + dx - width / 2;
    const top =
      (raised ? RAISED_HAND.top : HAND.top) +
      sink +
      (hold === "tucked" ? HAND_TUCK : hold === "away" ? HAND_AWAY : 0);
    return {
      slot: o.slot,
      zone: o.zone,
      look: "hand",
      width: FULL_WIDTH,
      span: FULL_WIDTH,
      opacity: 1,
      z: 200 + i,
      ...turned(left, top, turn + (raised ? 0 : HAND.bias), [width / 2, 0]),
      scale: fan.scale,
    };
  });
  if (!raised || stackCount === 0) return places;

  // Keep the entire raised fan beside the stack, including the turned corners.
  const stackLeft = stackPlace(0, stackCount).x;
  const rightEdge = Math.max(
    ...places.map((p) => {
      const angle = (p.rotate * Math.PI) / 180;
      return (
        p.x +
        width * (Math.cos(angle) + Math.max(0, -Math.sin(angle)) / CARD_ASPECT)
      );
    }),
  );
  const shift = Math.max(0, rightEdge + 16 - stackLeft);
  return places.map((p) => ({ ...p, x: p.x - shift }));
}

/** How far the opponent's hand reaches down the stage: above their life orb's top (y 48),
 * which would otherwise cover the middle backs' feet, so each back can be counted. Its feet lie
 * on one arc whatever the hand's size, rising `HELD_RISE` by `heldReach` either side of the
 * middle, so that the outer backs of the largest hand stay in sight; a hand wider than that
 * closes up, its outer backs clear of the opponent's piles. */
const HELD_FOOT = 44;
const HELD_RISE = 18;
// On the base stage; between the middle and the piles at the right edge, it gains half of what
// the stage does.
const heldReach = () => fromMiddle(370);

/** How far each hand card in `hand` may rise (RAISE) without the raised card, or the reach of
 * its element that keeps the pointer on it, coming over any of the `lands` boxes of your back
 * row: a land, or a permanent beside them, is always yours to point at. */
export function handLifts(hand: Placement[], lands: Box[]): Placement[] {
  // Clear of a land's foot by this much.
  const margin = 4;
  return hand.map((p) => {
    const a = (p.rotate * Math.PI) / 180;
    // The card's top center: its element turns about its top-left corner.
    const cx = p.x + (Math.cos(a) * p.width) / 2;
    const cy = p.y + (Math.sin(a) * p.width) / 2;
    // How far the turned card's higher top corner stands above its top center, and how far
    // to either side the raised card and its element's reach can come.
    const corner = (Math.abs(Math.sin(a)) * p.width) / 2;
    const half =
      (p.width * RAISE.scale) / 2 + RAISE.lift * Math.abs(Math.sin(a));
    let lift = RAISE.lift;
    for (const land of lands) {
      if (land.right < cx - half || land.left > cx + half) continue;
      // The element reaches `lift` above its turned top edge; the raised card's top stands
      // `lift` above its top center.
      const room = cy - land.bottom - margin;
      lift = Math.min(lift, (room - corner) / Math.cos(a), room);
    }
    return { ...p, lift: Math.max(0, lift) };
  });
}

/** The opponent's hand fanned along the top edge, as yours is along the bottom turned about the
 * table's middle: backs, and the faces of the cards you know (a look at their hand, a card
 * returned to it), upside down to you as the opponent holds them. Each back's foot lies on the
 * arc (`HELD_RISE`) and the back turns to lie along it, so the fan's lower edge runs smooth at
 * any size, no back's corner standing out of it. */
export function heldPlacements(cards: ObjectView[]): Placement[] {
  const n = cards.length;
  const mid = (n - 1) / 2;
  const height = BACK_WIDTH / CARD_ASPECT;
  const spacing =
    n > 1 ? Math.min(BACK_WIDTH - 2, (2 * heldReach()) / (n - 1)) : 0;
  // The arc's foot is HELD_FOOT - bend · dx² at dx from the middle; its slope there, -2 · bend
  // · dx, is how far a back turns.
  const bend = HELD_RISE / heldReach() ** 2;
  return cards.map((o, i) => {
    const dx = (i - mid) * spacing;
    const turn = (Math.atan(-2 * bend * dx) * 180) / Math.PI;
    // The foot on the arc, and the card's middle half a card up from it along its turn: the
    // card lies upside down about that middle, as the opponent holds it, facing them.
    const foot = [STAGE.width / 2 + dx, HELD_FOOT - bend * dx ** 2] as const;
    const a = (turn * Math.PI) / 180;
    const middle = [
      foot[0] + (Math.sin(a) * height) / 2,
      foot[1] - (Math.cos(a) * height) / 2,
    ] as const;
    return {
      slot: o.slot,
      zone: o.zone,
      look: "held",
      width: BACK_WIDTH,
      span: BACK_WIDTH,
      opacity: 1,
      z: 100 + i,
      ...turned(
        middle[0] - BACK_WIDTH / 2,
        middle[1] - height / 2,
        turn + 180,
        [BACK_WIDTH / 2, height / 2],
      ),
    };
  });
}
