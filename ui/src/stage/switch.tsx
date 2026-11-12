// Which board draws the game's table and what paces its views: the stage's (plan K25) with
// `?stage` in the URL, read as mobile/phone.ts reads `?phone`, and the old ones without, until
// the stage takes their place. The desktop's and the phone's boards are two layouts of the
// one stage.
import type { ComponentProps } from "react";
import { Board as CardBoard } from "../game/Board";
import { usePlayback as useBeats } from "../game/playback";
import { PhoneBoard as CardPhoneBoard } from "../mobile/PhoneBoard";
import { PhoneStageBoard, type PhoneStageProps } from "./PhoneStageBoard.tsx";
import { pageStage, useStagePlayback } from "./playback.ts";
import type { Asked } from "./prompt.ts";
import { usePrompt } from "./PromptBar.tsx";
import { StageBoard } from "./StageBoard.tsx";

const staged = new URLSearchParams(window.location.search).has("stage");

/** The game's table. The stage's has the old board's chrome and none of its prompts yet. */
export function Board(props: ComponentProps<typeof CardBoard>) {
  return staged ? <StageBoard {...props} /> : <CardBoard {...props} />;
}

/** The game's screen on a phone. The stage's has the phone's chrome and none of its prompts
 * yet. */
export function PhoneBoard(props: PhoneStageProps) {
  return staged ? (
    <PhoneStageBoard {...props} />
  ) : (
    <CardPhoneBoard {...props} />
  );
}

/** What the stage asks of the player, where it draws the prompt itself (prompt.ts): none
 * without it, where the old boards answer every decision through the screen. */
export const useAsked: () => Asked | undefined = staged
  ? () => usePrompt(pageStage().prompts)
  : () => undefined;

/** The game in progress as the player watches it: on the stage, the views' scripts played on
 * its clock; without it, the views cut into beats and held by timers. */
export const usePlayback = staged ? useStagePlayback : useBeats;
