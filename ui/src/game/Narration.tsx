// What the board says about the game playing on: who is acting (the turn controls' pill and the
// opponent's badge), the game log, and the spells that came and went in the view on screen,
// shown large for a moment so a counter or an instant is seen rather than only found later in
// a graveyard.
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { AnimatePresence, motion } from "motion/react";
import { Card } from "../components/Card";
import type { ObjectView, PlayerView, Side } from "../generated/protocol";
import { useFace } from "./faces";
import { STEP_NAMES } from "./layout";
import type { LogEntry } from "./log";
import { HOLD } from "./playback";

/** What the game is doing while the player waits: the opponent deciding, or a step of whose
 * turn passing. */
export function actingLabel(view: PlayerView): string {
  switch (view.status.kind) {
    case "thinking":
      return "Opponent is thinking";
    case "playing": {
      const whose = view.turn.active === "you" ? "Your" : "Opponent's";
      return `${whose} ${STEP_NAMES[view.turn.step]}`;
    }
    default:
      return "Opponent has priority";
  }
}

/** The opponent's badge's line under their name while they act, if they are. */
export function opponentActivity(
  view: PlayerView,
  waiting: boolean,
): string | undefined {
  if (view.status.kind === "thinking") return "Thinking";
  if (waiting && view.turn.active === "opponent") return "Taking their turn";
  return undefined;
}

/** The game log at the table's left: a heading for each turn, then what happened, newest at
 * the foot, scrolled to it as entries arrive. While permanents reach into its column it is
 * `folded` to a button, which toggles it `open`, the rows making room for it (layout.ts), so it
 * never lies over the cards or half hidden under them. */
export function GameLog({
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
  useLayoutEffect(() => {
    const el = list.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [log, shown]);
  return (
    <section
      className={`game-log${folded ? " game-log-folded" : ""}`}
      aria-label="Game log"
    >
      {folded && (
        <button
          type="button"
          className="game-log-toggle"
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
}

/** A spell that left the stack in the view on screen, and how. */
interface Announced {
  object: ObjectView;
  label: string;
  countered: boolean;
}

/** The spells `view`'s events took off the stack into a graveyard or exile, as they resolved
 * or were countered, oldest first: those would otherwise flash by unseen. Permanents that
 * resolve are seen arriving on the battlefield. */
export function announced(view: PlayerView): Announced[] {
  const objects = new Map(view.objects.map((o) => [o.slot, o]));
  const cast = new Map(
    view.events.flatMap((e) =>
      e.kind === "cast" ? [[e.slot, e.controller] as const] : [],
    ),
  );
  const out: Announced[] = [];
  for (const e of view.events) {
    if (
      e.kind !== "zoneChange" ||
      e.from !== "stack" ||
      (e.reason !== "resolve" && e.reason !== "counter") ||
      (e.to !== "graveyard" && e.to !== "exile")
    ) {
      continue;
    }
    const object = objects.get(e.slot);
    if (!object?.name) continue;
    const countered = e.reason === "counter";
    const caster: Side | undefined = cast.get(e.slot);
    out.push({
      object,
      countered,
      label: countered
        ? "Countered"
        : caster
          ? `${caster === "you" ? "You" : "Opponent"} cast`
          : "Resolved",
    });
  }
  return out.slice(-3);
}

/** How long announced spells stay up, in milliseconds. */
const ANNOUNCED_FOR = HOLD.counter + 400;

/** The spells `announced` names, large beside the stack for a moment. */
export function Announcements({ view }: { view: PlayerView }) {
  const items = announced(view);
  // The view whose spells are up: each view's for a while from when it shows.
  const [upFor, setUpFor] = useState<PlayerView | undefined>(view);
  useEffect(() => {
    setUpFor(view);
    const timer = setTimeout(() => setUpFor(undefined), ANNOUNCED_FOR);
    return () => clearTimeout(timer);
  }, [view]);
  const up = upFor === view;
  return (
    <AnimatePresence>
      {up && items.length > 0 && (
        <motion.div
          className="announcements"
          role="status"
          initial={{ opacity: 0, y: 12 }}
          animate={{ opacity: 1, y: 0 }}
          exit={{ opacity: 0 }}
          transition={{ duration: 0.25 }}
        >
          {items.map((item) => (
            <AnnouncedCard key={item.object.slot} item={item} />
          ))}
        </motion.div>
      )}
    </AnimatePresence>
  );
}

function AnnouncedCard({ item }: { item: Announced }) {
  const face = useFace(item.object);
  return (
    <figure
      className={item.countered ? "announced announced-countered" : "announced"}
      data-card={item.object.name}
    >
      <Card card={face} width={196} />
      {item.countered && <span className="announced-stamp">Countered</span>}
      <figcaption className="announced-label">{item.label}</figcaption>
    </figure>
  );
}
