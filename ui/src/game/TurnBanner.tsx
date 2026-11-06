// The turn's announcement, as Arena makes it: "Your Turn" or "Opponent's Turn" across the middle
// of the table as each turn begins, in a display face over two rosettes of interlaced circles
// turning against each other on a smoky band, in the player's color (cyan) or the opponent's
// (ember). It takes no pointer and fades on its own.
import { useEffect, useState } from "react";
import { AnimatePresence, motion, useReducedMotion } from "motion/react";
import type { PlayerView, Side } from "../generated/protocol";

/** How long the announcement shows, in ms. */
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
  const reduce = useReducedMotion() ?? false;
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
  const spin = (turns: number) =>
    reduce
      ? undefined
      : {
          rotate: [0, turns * 90],
          transition: {
            duration: TURN_BANNER_MS / 1000 + 0.6,
            ease: "linear" as const,
          },
        };
  return (
    <AnimatePresence>
      {shown && (
        <motion.div
          key={shown.turn}
          className={`turn-banner turn-banner-${shown.side}`}
          role="status"
          data-turn={shown.side}
          initial={reduce ? false : { opacity: 0, scale: 0.92 }}
          animate={{ opacity: 1, scale: 1 }}
          exit={reduce ? undefined : { opacity: 0, scale: 1.04 }}
          transition={{ duration: 0.35, ease: "easeOut" }}
        >
          <div className="turn-banner-band" />
          <svg
            className="turn-banner-rings"
            viewBox="-110 -110 220 220"
            aria-hidden
          >
            <motion.g className="turn-banner-ring-outer" animate={spin(1)}>
              {rosette(12, 50)}
            </motion.g>
            <motion.g className="turn-banner-ring-inner" animate={spin(-1)}>
              {rosette(8, 32, 0.5)}
            </motion.g>
            <circle className="turn-banner-rim" r={101} />
            <circle className="turn-banner-rim" r={104} />
          </svg>
          <span className="turn-banner-title">
            {shown.side === "you" ? "Your Turn" : "Opponent's Turn"}
          </span>
        </motion.div>
      )}
    </AnimatePresence>
  );
}
