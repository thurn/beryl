// What the board says about the game playing on: who is acting (the opponent's badge)
// and the game log.
import { memo, useEffect, useLayoutEffect, useRef } from "react";
import type { PlayerView } from "../generated/protocol";
import type { LogEntry } from "./log";

/** Whether the opponent is acting while the player waits: thinking, or their turn playing on.
 * The spinner under their orb (Player.tsx' Thinking) shows it, wordless. */
export function opponentActing(view: PlayerView, waiting: boolean): boolean {
  return (
    view.status.kind === "thinking" ||
    (waiting && view.turn.active === "opponent")
  );
}

/** The game log at the table's left: a heading for each turn, then what happened, newest at
 * the foot, scrolled to it as entries arrive and whenever it shows again. While permanents
 * reach into its column it is `folded` to a button, which toggles it `open`, the rows making
 * room for it (layout.ts), so it never lies over the cards or half hidden under them. */
export const GameLog = memo(function GameLog({
  log,
  folded,
  open,
  onToggle,
}: {
  log: LogEntry[];
  folded: boolean;
  open: boolean;
  onToggle(): void;
}) {
  const list = useRef<HTMLOListElement>(null);
  const shown = !folded || open;
  const toNewest = () => {
    const el = list.current;
    if (el) el.scrollTop = el.scrollHeight;
  };
  useLayoutEffect(toNewest, [log, shown]);
  // A box laid out empty loses its scroll: hidden by an ancestor, or not yet sized. It
  // scrolls to the newest entry again once it has its size.
  useEffect(() => {
    const el = list.current;
    if (!el) return;
    const observer = new ResizeObserver(toNewest);
    observer.observe(el);
    return () => observer.disconnect();
  }, []);
  return (
    <section
      className={`game-log${folded ? " game-log-folded" : ""}`}
      aria-label="Game log"
    >
      {folded && (
        <button
          type="button"
          className="icon-button game-log-toggle"
          aria-label={open ? "Hide log" : "Show log"}
          aria-expanded={open}
          onClick={onToggle}
        >
          <svg viewBox="0 0 20 20" aria-hidden>
            <path d="M4 5h12M4 10h12M4 15h8" />
          </svg>
        </button>
      )}
      <ol ref={list} className="game-log-list" hidden={!shown}>
        {log.map((e) => (
          <li
            key={e.key}
            className={`game-log-entry game-log-${e.kind} game-log-${e.side}`}
          >
            {e.text}
          </li>
        ))}
      </ol>
    </section>
  );
});
