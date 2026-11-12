// What the stage shows besides poses, for what is drawn from it by others: the faces of its
// nodes and the chrome around the table (stage.ts `ScriptStage.shown`).
import type { PlayerView, ScriptStep } from "../generated/protocol";
import type { Name, Resting } from "./layout.ts";
import type { Table } from "./table.ts";

/** What the stage shows now, for what is drawn from it besides poses (faces, the chrome): a
 * new one whenever a record, a dress or the table changes, a script has played out, or the
 * stage starts or stops playing, and never for motion alone. */
export interface Shown {
  /** The table as the steps so far have left it. */
  table: PlayerView | undefined;
  resting: ReadonlyMap<Name, Resting>;
  /** The view last shown whole, its script played out: the one the prompt and the chrome
   * around the table are drawn from. */
  view: PlayerView | undefined;
  /** Something given to `play` has not played out. */
  busy: boolean;
  /** The announcements passing over the table: the `announce` steps under way, in the order
   * they began. The banners around the table show for as long as their step is here, on the
   * stage's time. */
  notices: readonly ScriptStep[];
  /** The numbers changing now: the `count` steps under way, in the order they began, each
   * with the table it found. What shows that a number changed (the change beside a life
   * total) shows for as long as its step is here. */
  counts: readonly Counting[];
}

/** A `count` step under way, and the table as it found it. */
export interface Counting {
  step: ScriptStep;
  found?: Table;
}
