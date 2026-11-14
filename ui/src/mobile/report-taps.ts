// A double tap opens the bug report on a touch screen: with one finger on bare felt (the game
// board between cards, where a tap does nothing), or with two or more fingers anywhere, cards
// and buttons included. The stage acts only on one finger, so a tap with several fingers is
// free for this: the stage drops a press once a second finger joins it (stage/dom.ts asks
// `inMultiTouch`), and a button hears no click from it (`listenForReportTaps`).

/** Fingers that land within this many milliseconds of the first make one multi-finger tap. A
 * finger that lands later is another thumb playing, and the first finger's press goes on. */
export const TOGETHER_MS = 80;
/** The longest a tap can be held down. A longer touch is a long press, which inspects. */
export const TAP_MS = 300;
/** The longest gap between the first tap's lift and the second tap's touch. */
export const DOUBLE_MS = 300;
/** How far, in CSS pixels, a finger can drift during a tap, and how far apart the two taps
 * can land. */
export const SLOP_PX = 30;

type Touch = {
  /** When the first finger landed. */
  start: number;
  /** Where the first finger landed. */
  x: number;
  y: number;
  /** Whether the first finger landed on bare felt. */
  felt: boolean;
  /** Whether a second finger landed with the first. */
  multi: boolean;
  /** A finger moved, or landed too late to belong to the tap: this touch is not a tap. */
  spoiled: boolean;
};

/** Counts taps from pointer events, with times in milliseconds. It needs no DOM, so
 * `npm test` drives it directly. */
export class ReportTaps {
  /** Each finger down, by pointer id, and where it landed. */
  #fingers = new Map<number, { x: number; y: number }>();
  /** The touch under way, from its first finger landing to its last finger lifting. */
  #touch: Touch | undefined;
  /** The tap before, waiting for a second one. */
  #last: { end: number; x: number; y: number } | undefined;

  /** Whether the touch under way has two or more fingers that landed together. */
  get multi() {
    return this.#touch?.multi === true && this.#fingers.size > 0;
  }

  down(id: number, x: number, y: number, at: number, felt: boolean) {
    const touch = this.#touch;
    if (this.#fingers.size === 0 || !touch) {
      this.#touch = { start: at, x, y, felt, multi: false, spoiled: false };
    } else if (at - touch.start <= TOGETHER_MS) {
      touch.multi = true;
    } else {
      touch.spoiled = true;
    }
    this.#fingers.set(id, { x, y });
  }

  move(id: number, x: number, y: number) {
    const finger = this.#fingers.get(id);
    if (finger && this.#touch && far(finger, { x, y }))
      this.#touch.spoiled = true;
  }

  /** A finger lifted. Returns whether that finished a double tap. */
  up(id: number, at: number): boolean {
    if (!this.#fingers.delete(id) || this.#fingers.size > 0) return false;
    const touch = this.#touch;
    this.#touch = undefined;
    if (
      !touch ||
      touch.spoiled ||
      at - touch.start > TAP_MS ||
      !(touch.multi || touch.felt)
    ) {
      this.#last = undefined;
      return false;
    }
    const last = this.#last;
    if (last && touch.start - last.end <= DOUBLE_MS && !far(last, touch)) {
      this.#last = undefined;
      return true;
    }
    this.#last = { end: at, x: touch.x, y: touch.y };
    return false;
  }

  /** The browser took a finger (a scroll, a system gesture): no tap. */
  cancel(id: number) {
    if (!this.#fingers.delete(id)) return;
    if (this.#touch) this.#touch.spoiled = true;
    if (this.#fingers.size === 0) this.#touch = undefined;
    this.#last = undefined;
  }
}

const far = (a: { x: number; y: number }, b: { x: number; y: number }) =>
  Math.hypot(a.x - b.x, a.y - b.y) > SLOP_PX;

let listening: ReportTaps | undefined;

/** Whether two or more fingers that landed together are down now. The stage drops its press
 * then, so a multi-finger tap never plays or inspects a card. */
export const inMultiTouch = () => listening?.multi === true;

/** Whether `target` is bare felt: the game board or its card layer, with no card under it
 * (stage/dom.ts marks them `data-felt`). */
const onFelt = (target: EventTarget | null) =>
  target instanceof HTMLElement && target.dataset.felt !== undefined;

/** Listens for a double tap on the whole window and calls `report` on each one. It hears the
 * events first (the capture phase), so `inMultiTouch` is current when the stage hears them.
 * It times them by `performance.now()`, which tests can step, not by `timeStamp`.
 * Returns the function that stops it. */
export function listenForReportTaps(report: () => void): () => void {
  const taps = new ReportTaps();
  listening = taps;
  /** A click from a multi-finger tap reaches no button until this time. */
  let quietUntil = 0;
  const touch = (e: PointerEvent) => e.pointerType === "touch";
  const down = (e: PointerEvent) => {
    if (!touch(e)) return;
    taps.down(
      e.pointerId,
      e.clientX,
      e.clientY,
      performance.now(),
      onFelt(e.target),
    );
  };
  const move = (e: PointerEvent) => {
    if (touch(e)) taps.move(e.pointerId, e.clientX, e.clientY);
  };
  const up = (e: PointerEvent) => {
    if (!touch(e)) return;
    const multi = taps.multi;
    if (taps.up(e.pointerId, performance.now())) report();
    if (multi) quietUntil = performance.now() + DOUBLE_MS;
  };
  const cancel = (e: PointerEvent) => {
    if (touch(e)) taps.cancel(e.pointerId);
  };
  const click = (e: MouseEvent) => {
    if (taps.multi || performance.now() <= quietUntil) {
      e.preventDefault();
      e.stopPropagation();
    }
  };
  const listeners = [
    ["pointerdown", down],
    ["pointermove", move],
    ["pointerup", up],
    ["pointercancel", cancel],
    ["click", click],
  ] as const;
  for (const [kind, heard] of listeners)
    window.addEventListener(kind, heard as EventListener, true);
  return () => {
    for (const [kind, heard] of listeners)
      window.removeEventListener(kind, heard as EventListener, true);
    if (listening === taps) listening = undefined;
  };
}
