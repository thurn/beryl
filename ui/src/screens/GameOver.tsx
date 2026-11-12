// The result over the table once the game has ended.
import { BLANK, paragraphOf, useAbilityFace } from "../game/faces";
import { Button, Dock, DockButtons } from "../components/Controls";
import { Card } from "../components/Card";
import {
  type Culprit,
  type LossReason,
  type OutcomeView,
  type Stall,
} from "../ipc";
import { useState } from "react";

const STALLS: Record<Stall, string> = {
  decisionsPerTurn: "too many decisions in one turn",
  turns: "too many turns",
  stateBasedIterations: "state-based actions did not settle",
  capacity: "a zone or queue was full",
};

/** The result over the table once the game has ended: shown until the player puts it aside
 * to see the battlefield, and back on asking. The view of the end comes only once its script
 * has played out, the table held still at its end (UI Conventions, Verbs, dwell, "game's
 * end"), so the result waits no longer. */
export function useResult(ended: boolean) {
  const [aside, setAside] = useState(false);
  return {
    state: !ended ? "waiting" : aside ? "aside" : "shown",
    putAside: () => setAside(true),
    show: () => setAside(false),
  };
}

export function resultTitle(outcome?: OutcomeView): string {
  if (!outcome) return "Game stopped";
  switch (outcome.kind) {
    case "win":
      return outcome.winner === "you" ? "Victory" : "Defeat";
    case "draw":
      return "Draw";
    case "stalled":
      return "Stalled";
  }
}

type ShownLossReason = Exclude<LossReason, "life">;

/** Why the game ended, in a word beside its icon. */
const LOSSES: Record<ShownLossReason, string> = {
  library: "Library",
  effect: "Card effect",
  concede: "Conceded",
};

/** The end of the game: who won, and why. */
export function GameOver({
  outcome,
  onClose,
}: {
  outcome?: OutcomeView;
  onClose(): void;
}) {
  const title = resultTitle(outcome);
  const win = outcome?.kind === "win" ? outcome : undefined;
  const reason =
    win && win.reason !== "life"
      ? { icon: win.reason, text: LOSSES[win.reason] }
      : outcome?.kind === "stalled"
        ? { icon: "stall" as const, text: STALLS[outcome.reason] }
        : undefined;
  return (
    <div className="game-over" role="dialog" aria-label={title}>
      <div className="overlay-backdrop" />
      <h2 className="game-over-title">{title}</h2>
      {reason && (
        <p className="game-over-reason" data-reason={reason.icon}>
          <ReasonIcon reason={reason.icon} />
          {reason.text}
        </p>
      )}
      {reason && win?.by && <CulpritCard culprit={win.by} />}
      <Dock>
        <DockButtons>
          <Button onClick={() => (window.location.hash = "#/")}>
            New Game
          </Button>
          <Button variant="secondary" onClick={onClose}>
            View Battlefield
          </Button>
        </DockButtons>
      </Dock>
    </div>
  );
}

/** The card that ended the game, under the reason: with the ability that did it lit, as a
 * plate over its scan, or whole for a spell or a source of damage. */
function CulpritCard({ culprit }: { culprit: Culprit }) {
  const face = useAbilityFace(culprit.name, undefined, BLANK);
  const lit =
    culprit.ability && face.text
      ? paragraphOf(face.text, culprit.ability)
      : undefined;
  return (
    <div className="game-over-card" data-card={culprit.name}>
      <Card card={{ ...face, lit }} width={230} />
    </div>
  );
}

/** A small line icon for why the game ended. */
function ReasonIcon({ reason }: { reason: ShownLossReason | "stall" }) {
  const paths: Record<ShownLossReason | "stall", string> = {
    // A stack of cards: the library.
    library: "M7 4h10v16H7z M5 6v14h10 M3 8v14h10",
    // A spark: a card's effect.
    effect: "M12 3l2 7 7 2-7 2-2 7-2-7-7-2 7-2z",
    // A flag: a concession.
    concede: "M6 21V4 M6 4h11l-2 4 2 4H6",
    // An hourglass: the engine gave up.
    stall:
      "M7 3h10 M7 21h10 M8 3c0 5 8 5 8 9s-8 4-8 9 M16 3c0 5-8 5-8 9s8 4 8 9",
  };
  return (
    <svg className="game-over-icon" viewBox="0 0 24 24" aria-hidden>
      <path d={paths[reason]} />
    </svg>
  );
}
