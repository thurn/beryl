// One entry of the stage (stage.ts): what it holds of each game object it draws.
import type { Run } from "./beat.ts";
import type { Resting } from "./layout.ts";
import type { Track } from "./tracks.ts";

export interface Entry {
  resting: Resting;
  track?: Track;
  /** Its pose is to be written at the tick's end. */
  moved: boolean;
  /** The `vanish` taking it from where `resting` still has it, until that step ends. */
  leaving?: Run;
  /** The `fly` taking it to a pile, until that step ends: the pile shows the top it had. */
  landing?: Run;
  /** What it is drawn as while it has yet to turn over on its way (`Track.turn`): as it
   * rests, but for its back. */
  covered?: Resting;
}
