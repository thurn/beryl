// The announcements a script makes (`announce` steps), read for the banners and pills around
// the table: which is passing over it now is the stage's to say (`Shown.notices`), and what
// each reads is here, for the desktop's chrome and the phone's alike.
import type { Notice, PlayerView, ScriptStep } from "../generated/protocol";

type Of<K extends Notice["kind"]> = Extract<Notice, { kind: K }>;

/** The announcement of `kind` passing over the table, the last begun if several are, with the
 * step that makes it. */
export function announced<K extends Notice["kind"]>(
  notices: readonly ScriptStep[],
  kind: K,
): { notice: Of<K>; step: ScriptStep } | undefined {
  for (const step of [...notices].reverse()) {
    const { act } = step;
    if (act.verb === "announce" && act.notice.kind === kind) {
      return { notice: act.notice as Of<K>, step };
    }
  }
  return undefined;
}

/** What the pill over a player says of the scry they made, as Arena words it. */
export const scryWords = ({ top, bottom }: Of<"scry">) =>
  `Scry: ${top} top, ${bottom} bottom.`;

/** The banner of a coin's flip: the card that flipped it, which is the step's cause, and who
 * won or lost. */
export function coinBanner(
  { notice, step }: { notice: Of<"coin">; step: ScriptStep },
  table: PlayerView,
): { title: string; detail: string } {
  const cause = step.cause;
  const source =
    cause?.kind === "object"
      ? table.objects.find((o) => o.slot === cause.slot)?.name
      : undefined;
  const who = notice.player === "you" ? "You" : "Opponent";
  return {
    title: source ?? "Coin flip",
    detail: `${who} ${notice.won ? "won" : "lost"} the flip`,
  };
}
