// A fault to plant in the stage, for the check of the audits themselves
// (ui/scripts/audits.mjs): with `?skip=<verb>` under a driven browser, the stage is never
// given the first step of that verb in any script the session sends, as a stage that dropped
// a step would play it. The view still carries the script whole, which is what an audit
// judges by (ui/scripts/stage-audit.mjs), so the audit must report the step.
import type { PlayerView, Script } from "../generated/protocol";
import type { ScriptStage } from "./stage.ts";

/** The stage as what feeds it uses it (feed.ts). */
type Player = Pick<ScriptStage, "play" | "skip" | "shown" | "subscribe">;

/** The verb `?skip=` names, under a driven browser; none otherwise. */
export const skipped = (): string | null =>
  navigator.webdriver
    ? new URLSearchParams(window.location.search).get("skip")
    : null;

/** `script` without its first step of `verb`; itself if it has none. */
export function without(script: Script, verb: string): Script {
  let dropped = false;
  const beats = script.beats
    .map((beat) =>
      beat.filter((step) => {
        if (dropped || step.act.verb !== verb) return true;
        dropped = true;
        return false;
      }),
    )
    .filter((beat) => beat.length > 0);
  return dropped ? { ...script, beats } : script;
}

/** `player`, given each script without its first step of `verb`. */
export function skipping(player: Player, verb: string): Player {
  return {
    play: (view: PlayerView, script?: Script) =>
      player.play(view, script && without(script, verb)),
    skip: () => player.skip(),
    subscribe: player.subscribe,
    get shown() {
      return player.shown;
    },
  };
}
