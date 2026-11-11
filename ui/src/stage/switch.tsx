// Which board draws the game's table: the stage's (plan K25) with `?stage` in the URL, read as
// mobile/phone.ts reads `?phone`, and the old one without, until the stage takes its place.
import type { ComponentProps } from "react";
import { Board as CardBoard } from "../game/Board";
import { StageBoard } from "./StageBoard.tsx";

const staged = new URLSearchParams(window.location.search).has("stage");

/** The game's table. The stage's takes none of the old board's controls yet. */
export function Board(props: ComponentProps<typeof CardBoard>) {
  return staged ? <StageBoard view={props.view} /> : <CardBoard {...props} />;
}
