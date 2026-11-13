// The stage's clock. Stage time moves only inside
// `advance`, so everything the stage shows is a function of how many milliseconds it was given
// and never of when it was asked. Live, one requestAnimationFrame loop feeds it the time
// between frames; because it only sums those, a test that pauses the page's clock (Playwright's
// `page.clock`) or holds this one steps the stage exactly. Nothing here, and nothing on the
// stage, reads `performance.now()` or `Date.now()`, and nothing waits on a timer.

/** The clock a test can step. */
export interface StageClock {
  /** Stage milliseconds. Moves only inside `advance`. */
  readonly now: number;
  /** Runs everything due in (now, now + ms], then writes styles once. Synchronous. */
  advance(ms: number): void;
  /** Stops following frames: from here only `advance` moves time. */
  hold(): void;
  /** Follows frames again. */
  release(): void;
}

/** Where live frames come from: `requestAnimationFrame`, in a browser. */
export interface Frames {
  request(frame: (timestamp: number) => void): number;
  cancel(handle: number): void;
}

/** The browser's frames; none under Node, where a clock is only ever advanced by hand. */
export const animationFrames: Frames | undefined =
  typeof requestAnimationFrame === "function"
    ? {
        request: (frame) => requestAnimationFrame(frame),
        cancel: (handle) => cancelAnimationFrame(handle),
      }
    : undefined;

/** A clock that hands each span of time it is advanced over to `tick`, once `now` is the
 * span's end. It starts held. */
export class Clock implements StageClock {
  #now = 0;
  #handle: number | undefined;
  #last: number | undefined;
  readonly #tick: (from: number, to: number) => void;
  readonly #frames: Frames | undefined;

  constructor(tick: (from: number, to: number) => void, frames?: Frames) {
    this.#tick = tick;
    this.#frames = frames;
  }

  get now() {
    return this.#now;
  }

  advance(ms: number) {
    if (!(ms >= 0) || ms === Infinity) {
      throw new RangeError(`the stage clock cannot advance by ${ms} ms`);
    }
    const from = this.#now;
    this.#now = from + ms;
    this.#tick(from, this.#now);
  }

  hold() {
    if (this.#handle !== undefined) this.#frames?.cancel(this.#handle);
    this.#handle = undefined;
    this.#last = undefined;
  }

  release() {
    if (this.#handle !== undefined || !this.#frames) return;
    this.#handle = this.#frames.request(this.#frame);
  }

  /** One live frame: the time since the frame before it, none for the first after a release.
   * The next frame is asked for first, so a tick that throws is reported and the stage goes
   * on. */
  readonly #frame = (timestamp: number) => {
    const last = this.#last;
    this.#last = timestamp;
    this.#handle = this.#frames?.request(this.#frame);
    this.advance(last === undefined ? 0 : Math.max(0, timestamp - last));
  };
}
