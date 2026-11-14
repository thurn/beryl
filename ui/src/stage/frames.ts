// How smoothly the player's own screen plays the stage's scripts. Headless browsers (`just
// ui-perf`) do not drop the frames the app's webview drops on a loaded Mac with a Retina
// window, so the app times its own frames: from when the stage starts playing until it is idle
// again, it measures the time between frames and counts, for each verb playing, the frames that
// came late. A run that dropped any is written to the UI log, which a bug report carries
// (diagnostics.ts), so a report about lag names the animations that lagged.
import type { Frames } from "./clock.ts";
import type { ScriptStage } from "./stage.ts";
import type { Step } from "./table.ts";

/** A frame that took longer than this many milliseconds dropped at least one: one and a half
 * frames at 60 Hz, as `just ui-perf` counts them. */
const DROPPED = (1000 / 60) * 1.5;

/** A gap between frames longer than this is a page that was hidden or frozen (the bug
 * button freezes it), not a frame the player watched. */
const UNWATCHED = 2000;

/** The frames of one run of the stage, from busy to idle. */
export interface FrameRun {
  frames: number;
  dropped: number;
  /** The longest frame, in milliseconds. */
  worst: number;
  /** For each verb playing (an announcement by its kind, `announce:turn`; `(none)` between
   * steps), its frames, how many dropped, and the milliseconds they were late by in all. */
  verbs: Record<string, { frames: number; dropped: number; late: number }>;
}

/** Times the frames of every run of `stage` and hands `report` each run that dropped any.
 * Returns what stops it. */
export function timeFrames(
  stage: Pick<ScriptStage, "follow" | "idle">,
  frames: Frames,
  report: (run: FrameRun) => void,
): () => void {
  const playing = new Map<Step, string>();
  let run: FrameRun | undefined;
  let last: number | undefined;
  let pending: number | undefined;
  const frame = (time: number) => {
    pending = undefined;
    if (!run) return;
    const length = last === undefined ? 0 : time - last;
    last = time;
    pending = frames.request(frame);
    if (length === 0 || length > UNWATCHED) return;
    const dropped = length > DROPPED;
    run.frames += 1;
    run.worst = Math.max(run.worst, Math.round(length));
    if (dropped) run.dropped += 1;
    const names = playing.size > 0 ? new Set(playing.values()) : ["(none)"];
    for (const name of names) {
      const verb = (run.verbs[name] ??= { frames: 0, dropped: 0, late: 0 });
      verb.frames += 1;
      if (dropped) {
        verb.dropped += 1;
        verb.late += Math.round(length - 1000 / 60);
      }
    }
  };
  return stage.follow(({ edge, step }) => {
    const { act } = step;
    const name =
      act.verb === "announce" ? `announce:${act.notice.kind}` : act.verb;
    if (edge === "start") playing.set(step, name);
    else playing.delete(step);
    if (run) return;
    run = { frames: 0, dropped: 0, worst: 0, verbs: {} };
    last = undefined;
    // The frame the last run asked for, if it has not come yet, is this run's first.
    pending ??= frames.request(frame);
    void stage.idle().then(() => {
      const done = run;
      run = undefined;
      // A step passed over by a skip tells no end.
      playing.clear();
      if (done && done.dropped > 0) report(done);
    });
  });
}
