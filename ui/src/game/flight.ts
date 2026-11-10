/** Direct travel shared by cards and ability indicators moving across the table. */
const FLIGHT_MS = 320;
export const FLIGHT_HOLD = FLIGHT_MS + 40; // Allow the first and last frames to paint.

export const DIRECT_FLIGHT = {
  type: "tween",
  duration: FLIGHT_MS / 1000,
  ease: [0.33, 0.44, 0.4, 1],
} as const;

/** An object created by another appears as it starts moving away. */
export const EMERGENCE_FADE = { duration: 0.045, ease: "linear" } as const;
