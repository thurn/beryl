// A stage's idle waiters and live frame loop. The table remains mounted behind a result,
// but its settled view needs no more frames; another view starts the loop again.
import type { Status } from "../generated/protocol.ts";
import type { StageClock } from "./clock.ts";

export class StageIdle {
  readonly #clock: StageClock;
  #waiting: (() => void)[] = [];
  #heldForResult = false;

  constructor(clock: StageClock) {
    this.#clock = clock;
  }

  wait(busy: boolean): Promise<void> {
    if (!busy) return Promise.resolve();
    return new Promise((resolve) => this.#waiting.push(resolve));
  }

  playing() {
    if (!this.#heldForResult) return;
    this.#heldForResult = false;
    this.#clock.release();
  }

  settled(status?: Status["kind"]) {
    const waiting = this.#waiting;
    this.#waiting = [];
    for (const resolve of waiting) resolve();
    if (status !== "over" && status !== "stopped") return;
    this.#clock.hold();
    this.#heldForResult = true;
  }
}
