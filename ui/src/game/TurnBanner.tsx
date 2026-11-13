// The turn's announcement, as Arena makes it: "Your Turn" or "Opponent's Turn" across the middle
// of the table as each turn begins, in a display face over two rosettes of interlaced circles
// turning against each other on a smoky band, in the player's color (cyan) or the opponent's
// (ember). It takes no pointer, and one shows at a time.
import type { ReactNode } from "react";
import type { Side } from "../generated/protocol";

/** Where an announcement is drawn: an element that is no box of its own (`display:
 * contents`), always there, on which the stage writes how far through each announcement is
 * (stage/dom.ts `notice`). Written on the table instead, that number was inherited by every
 * element on it, and the whole table was restyled every frame of every banner. */
export function NoticeSlot({ children }: { children?: ReactNode }) {
  return <div className="stage-notices">{children}</div>;
}

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

/** The announcement of `side`'s turn: shown for as long as the step that announces the turn
 * plays, and drawn from how far through that step is (stage/stage.css `.stage-notice`), with
 * no keyframes of its own. */
export function TurnAnnouncement({ side }: { side: Side }) {
  return (
    <div
      className={`turn-banner turn-banner-${side} stage-notice`}
      role="status"
      data-turn={side}
    >
      <div className="turn-banner-band" />
      {/* Each rosette is a drawing of its own, turned whole, so the compositor turns it
          and its glow is drawn once (screens/styles/turn-banner.css). */}
      <svg
        className="turn-banner-rings turn-banner-ring-outer"
        viewBox="-110 -110 220 220"
        aria-hidden
      >
        {rosette(12, 50)}
      </svg>
      <svg
        className="turn-banner-rings turn-banner-ring-inner"
        viewBox="-110 -110 220 220"
        aria-hidden
      >
        {rosette(8, 32, 0.5)}
      </svg>
      <svg
        className="turn-banner-rings"
        viewBox="-110 -110 220 220"
        aria-hidden
      >
        <circle className="turn-banner-rim" r={101} />
        <circle className="turn-banner-rim" r={104} />
      </svg>
      <span className="turn-banner-title">
        {side === "you" ? "Your Turn" : "Opponent's Turn"}
      </span>
    </div>
  );
}
