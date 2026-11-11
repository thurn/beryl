// The stage the table is drawn on and the camera that looks at it: its width, and how
// depth shrinks and leans what lies on the table.

/** The stage the table is drawn on, scaled to fit the window (Stage.tsx): always 900 high, and
 * as wide as the window's shape asks within `STAGE_WIDTHS`, so a squarer window draws the table
 * larger rather than leave bands above and below it, and a wider one takes the room at its sides.
 * Stage sets `width` (`setStageWidth`) before the table is drawn at it, and every placement reads
 * it when made: positions are pure functions of the view and the window's shape. */
export const STAGE = { width: 1600, height: 900 };

/** How wide the stage may be: `base`, the width every position here was drawn for (16:9);
 * `least`, 16:10, the app's first window and a MacBook's screen, as narrow as a choice
 * screen's tray and side column (components.css .overlay) still fit side by side; `most`, 21:9,
 * beyond which the table's controls would stand too far apart to take in at once. A window of
 * another shape shows the table's cloth past the stage's edges. */
export const STAGE_WIDTHS = { base: 1600, least: 1440, most: 2134 };

/** Sets the stage's width (Stage.tsx), within `STAGE_WIDTHS`. */
export function setStageWidth(width: number) {
  STAGE.width = Math.min(
    STAGE_WIDTHS.most,
    Math.max(STAGE_WIDTHS.least, width),
  );
}

/** Where `x`, drawn for the base stage, lies on the stage as wide as it is now: what stands at
 * the right keeps its distance from the right edge (`fromRight`), what stands at the middle from
 * the middle (`fromMiddle`); what stands at the left keeps its `x`. */
export const fromRight = (x: number) => x + STAGE.width - STAGE_WIDTHS.base;
export const fromMiddle = (x: number) =>
  x + (STAGE.width - STAGE_WIDTHS.base) / 2;

/** The camera's distance from the near edge of the table, in perspective's terms. */
const CAMERA_DISTANCE = 1400;
/** Depth grows linearly up the table, from zero at the near edge. */
const NEAR_Y = 760;
const DEPTH_PER_PX = 0.6;

/** How much a thing lying at screen height `y` is shrunk by distance: d / (d + z), what
 * `perspective(d)` does to something `z` behind the screen plane. Things lie at a depth that
 * grows toward the opponent, so they stay upright rectangles that shrink with distance. */
export const depthScale = (y: number) =>
  CAMERA_DISTANCE / (CAMERA_DISTANCE + Math.max(0, NEAR_Y - y) * DEPTH_PER_PX);

/** How far the camera is from a thing lying on the table at screen height `y`, in stage
 * pixels: what `perspective` would need to grow it as it rises toward the lens. */
export const cameraDistance = (y: number) => CAMERA_DISTANCE / depthScale(y);

/** Where the table's receding edges meet, far above the stage: a card left of center leans
 * left at its near end, and one right of center leans right. */
const VANISHING_Y = -4400;

/** How far a thing lying on the table with its top-center at (x, y) leans, in degrees: its
 * sides run toward the vanishing point, over the stage's middle. */
export const lean = (x: number, y: number) =>
  (Math.atan((x - STAGE.width / 2) / (y - VANISHING_Y)) * 180) / Math.PI;
