// Where the phone writes what the open prompt asks (stage/PromptBar.tsx `PhonePromptOver`).
import type { PhoneLayout } from "./layout.ts";
import { RATIO } from "./sizing.ts";

/** How far the pill with what a prompt asks keeps from the cards above and below it. */
const ASKED_GAP = 4;

/** Where the top of the pill with what the open prompt asks stands, `height` high, in
 * `layout`: centered on the middle band's gold line, as Arena writes what a prompt asks on a
 * line across the middle of its table; or, where a card lies there (an item on the stack),
 * at the nearest height clear of every permanent and stack item, so that a pick made on the
 * battlefield shows every card it picks among. With no such height it stays on the line.
 * Heights are compared across the whole width, since the pill is centered and only as wide
 * as its words. */
export function askedTop(
  { bands, cards, stack }: PhoneLayout,
  height: number,
): number {
  const line = bands.middle.top + (bands.middle.height - height) / 2;
  const taken = [
    ...cards
      .filter((card) => card.zone === "battlefield")
      .map((card) => ({
        top: card.y,
        // A tapped card's footprint is turned a quarter, `width` high.
        bottom: card.y + (card.tapped ? card.width : card.width / RATIO),
      })),
    ...stack.map((item) => ({
      top: item.y,
      bottom: item.y + item.width / RATIO,
    })),
  ];
  const clear = (top: number) =>
    top >= bands.opponentLands.top &&
    top + height <= bands.yourStrip.top &&
    taken.every(
      (card) =>
        top + height + ASKED_GAP <= card.top || top >= card.bottom + ASKED_GAP,
    );
  const places = [
    line,
    ...taken.flatMap((card) => [
      card.bottom + ASKED_GAP,
      card.top - ASKED_GAP - height,
    ]),
  ].filter(clear);
  places.sort((a, b) => Math.abs(a - line) - Math.abs(b - line));
  return places[0] ?? line;
}
