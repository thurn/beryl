/** Phone layouts use one column: 375px at least, 430px at most. */
export const PHONE_WIDTH = { min: 375, max: 430 } as const;

/** The layout column inside the top safe area. Narrow screens scale the 375px layout. */
export function phoneFrame(width: number, height: number) {
  const scale = Math.min(1, width / PHONE_WIDTH.min);
  return {
    width: Math.min(PHONE_WIDTH.max, Math.max(PHONE_WIDTH.min, width)),
    height: height / scale,
    scale,
  };
}

/** A crowded creature row overlaps cards instead of making their name bars unreadable. */
export function creatureHeight(byHeight: number, byWidth: number) {
  return Math.max(56, Math.min(byHeight, byWidth, 128));
}
