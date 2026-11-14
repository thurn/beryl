// The announcements a script makes (`announce` steps), read for the banners and pills around
// the table: which is passing over it now is the stage's to say (`Shown.notices`), and what
// each reads is here, for the desktop's chrome and the phone's alike.
import type {
  Notice,
  PlayerView,
  ScriptStep,
  Side,
} from "../generated/protocol";
import type { Counting } from "./stage.ts";

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

/** What the pill beside the opponent's library says of the end of it they chose for a card.
 * Arena shows no words for it (UI Conventions, "Not seen in Arena"), so it is the end's name
 * alone, as a scry's chooser names its piles. */
export const endWords = ({ top }: Of<"topOrBottom">) =>
  top ? "Top" : "Bottom";

/** The banner of a coin's flip: the card that flipped it, which the notice names, and who won
 * or lost. */
export function coinBanner(
  { notice }: { notice: Of<"coin"> },
  table: PlayerView,
): { title: string; detail: string } {
  const source = table.objects.find((o) => o.slot === notice.source)?.name;
  const who = notice.player === "you" ? "You" : "Opponent";
  return {
    title: source ?? "Coin flip",
    detail: `${who} ${notice.won ? "won" : "lost"} the flip`,
  };
}

/** By how much `side`'s life total is changing now: over the `count` steps of it under way,
 * from the total the first found to the last one's. None while no such step plays. */
export function lifeChange(
  counts: readonly Counting[],
  side: Side,
): number | undefined {
  let from: number | undefined;
  let to: number | undefined;
  for (const { step, found } of counts) {
    const { act } = step;
    if (act.verb !== "count" || act.tally.kind !== "life") continue;
    if (act.tally.player !== side) continue;
    from ??= found?.view.players[side].life;
    to = act.tally.life;
  }
  return from === undefined || to === undefined || to === from
    ? undefined
    : to - from;
}
