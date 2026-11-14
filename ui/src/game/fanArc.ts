// The circle a hand of cards fans along, on the desktop and the phone.

/** The radius of the circle a hand fans along, in card widths. Arena's hand lies on one
 * circle whatever its size: a card's turn grows with its distance from the middle, about
 * 0.0275° a pixel at 1920 wide with cards 180 wide, at 5, 6, 7, 8, 10 and 18 cards
 * (sybcO8S7x6A 24:27 and 51:04, FGsR_FGChOU 2:25, kvVLJ_cS_KE 6:24, uzg_P976qhc 17:20,
 * _IBqPx0gcag 12:00, nBj2ikeYCtk 1:07). A crowded hand so turns and sinks less per card than a
 * spread one, rather than curling at its ends. */
export const FAN_RADIUS = 14;

/** Where a fanned card `dx` from the fan's middle lies on the circle of `FAN_RADIUS` card widths
 * of `width`: how far its top center sinks below the middle card's, and how many degrees it
 * turns (clockwise right of the middle) to lie along the circle. */
export function onArc(dx: number, width: number) {
  const radius = FAN_RADIUS * width;
  const x = Math.max(-radius, Math.min(radius, dx));
  return {
    sink: radius - Math.sqrt(radius ** 2 - x ** 2),
    turn: (Math.asin(x / radius) * 180) / Math.PI,
  };
}
