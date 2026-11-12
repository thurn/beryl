// The turn's announcement, as Arena makes it: "Your Turn" or "Opponent's Turn" across the middle
// of the table as each turn begins, in a display face over two rosettes of interlaced circles
// turning against each other on a smoky band, in the player's color (cyan) or the opponent's
// (ember). It takes no pointer and fades on its own. One shows at a time: CSS keyframes fade
// it in and out within its time and its timer removes it, so nothing waits on an exit
// animation (which a loaded machine may never finish) and a new turn replaces it outright.
import { useEffect, useState, type CSSProperties } from "react";
import type { PlayerView, Side } from "../generated/protocol";

/** How long the announcement shows, in ms, its fades included. */
export const TURN_BANNER_MS = 1700;

/** `count` circles of radius `r` whose centers lie `r` from the middle: each passes through the
 * middle, and together they interlace into a rosette. */
function rosette(count: number, r: number, offset = 0) {
  return Array.from({ length: count }, (_, i) => {
    const a = ((i + offset) / count) * Math.PI * 2;
    return (
      <circle
        key={i}
        cx={(Math.cos(a) * r).toFixed(2)}
        cy={(Math.sin(a) * r).toFixed(2)}
        r={r}
      />
    );
  });
}

/** Announces each turn as a live view begins it; views from the past announce nothing. */
export function TurnBanner({
  view,
  live,
}: {
  view: PlayerView;
  live: boolean;
}) {
  const [shown, setShown] = useState<{ turn: number; side: Side }>();
  useEffect(() => {
    if (!live) return;
    for (const e of view.events) {
      if (e.kind === "stepBegins" && e.step === "untap") {
        setShown({ turn: view.turn.number, side: e.active });
      }
    }
  }, [view, live]);
  useEffect(() => {
    if (!shown) return;
    const timer = setTimeout(() => setShown(undefined), TURN_BANNER_MS);
    return () => clearTimeout(timer);
  }, [shown]);
  if (!shown) return null;
  return (
    <TurnAnnouncement key={shown.turn} side={shown.side} ms={TURN_BANNER_MS} />
  );
}

/** The announcement itself, of `side`'s turn, fading in and out within `ms`: whoever shows it
 * says for how long (the old board a timer). With `staged` it is the stage's: shown for as
 * long as the step that announces the turn plays, and drawn from how far through that step is
 * (stage/stage.css `.stage-notice`), with no keyframes of its own. */
export function TurnAnnouncement({
  side,
  ms,
  staged,
}: {
  side: Side;
  ms?: number;
  staged?: boolean;
}) {
  return (
    <div
      className={`turn-banner turn-banner-${side}${staged ? " stage-notice" : ""}`}
      role="status"
      data-turn={side}
      style={
        ms === undefined
          ? undefined
          : ({ "--turn-ms": `${ms}ms` } as CSSProperties)
      }
    >
      <div className="turn-banner-band" />
      <svg
        className="turn-banner-rings"
        viewBox="-110 -110 220 220"
        aria-hidden
      >
        <g className="turn-banner-ring-outer">{rosette(12, 50)}</g>
        <g className="turn-banner-ring-inner">{rosette(8, 32, 0.5)}</g>
        <circle className="turn-banner-rim" r={101} />
        <circle className="turn-banner-rim" r={104} />
      </svg>
      <span className="turn-banner-title">
        {side === "you" ? "Your Turn" : "Opponent's Turn"}
      </span>
    </div>
  );
}
