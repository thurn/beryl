// Which board draws the game's table and what paces its views: the stage's (plan K25) with
// `?stage` in the URL, read as mobile/phone.ts reads `?phone`, and the old ones without, until
// the stage takes their place. The phone has no stage yet.
import type { ComponentProps } from "react";
import { Board as CardBoard } from "../game/Board";
import { usePlayback as useBeats } from "../game/playback";
import { useStagePlayback } from "./playback.ts";
import { StageBoard } from "./StageBoard.tsx";

const params = new URLSearchParams(window.location.search);
const staged = params.has("stage") && !params.has("phone");

/** The game's table. The stage's takes none of the old board's controls yet, but the click
 * that skips what is playing. */
export function Board(props: ComponentProps<typeof CardBoard>) {
  return staged ? (
    <StageBoard onSkip={props.onSkip} />
  ) : (
    <CardBoard {...props} />
  );
}

/** The game in progress as the player watches it: on the stage, the views' scripts played on
 * its clock; without it, the views cut into beats and held by timers. */
export const usePlayback = staged ? useStagePlayback : useBeats;
