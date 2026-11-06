// A game against an agent: the start screen, then the battlefield (game/Board.tsx) with the
// mulligan, choice (game/ChoiceOverlay.tsx) and game-over screens over it. Views arrive as
// `game-update` events and play back at a readable pace (game/playback.ts); the screen answers
// the open decision through `choose`.
import { useEffect, useRef, useState } from "react";
import { Button } from "../components/Controls";
import { Card } from "../components/Card";
import { RulesText } from "../components/Mana";
import {
  cancel,
  choose,
  CommandError,
  listAgents,
  setStops,
  type AgentView,
  type CandidateIndex,
  type DecisionView,
  type ObjectView,
  type OutcomeView,
  type PlayerView,
  type Stall,
} from "../ipc";
import { Board } from "../game/Board";
import { ChoiceOverlay } from "../game/ChoiceOverlay";
import { choiceOf, type Choice } from "../game/choices";
import { follow, isOpening, type Intent } from "../game/decisions";
import { useFace } from "../game/faces";
import type { Playback } from "../game/playback";
import { Brand, Stage } from "../game/Stage";

/** The agent that plays on the true state, hidden cards included. */
const CHEATING = "mcts-omniscient";

/** Picks the opponent and an optional seed, and starts the game; or returns to the game in
 * progress. */
export function StartScreen({
  inProgress,
  onStart,
}: {
  inProgress: boolean;
  onStart(agent: string, seed?: number): Promise<void>;
}) {
  const [agents, setAgents] = useState<AgentView[]>([]);
  const [agent, setAgent] = useState<string>();
  const [seed, setSeed] = useState("");
  const [error, setError] = useState<string>();
  const [pending, setPending] = useState(false);
  useEffect(() => {
    listAgents().then((list) => {
      setAgents(list);
      setAgent((a) => a ?? list[0]?.name);
    });
  }, []);
  const seedValue = seed.trim() === "" ? undefined : Number(seed);
  const seedOk =
    seedValue === undefined ||
    (Number.isInteger(seedValue) && seedValue >= 0 && seedValue < 2 ** 32);
  const start = () => {
    if (!agent) return;
    setPending(true);
    setError(undefined);
    onStart(agent, seedValue).then(
      () => (window.location.hash = "#/game"),
      (e: unknown) => {
        setPending(false);
        setError(String(e));
      },
    );
  };
  return (
    <Stage>
      <div className="table start-screen">
        <Brand />
        <section className="start-panel">
          <h1 className="start-title">New game</h1>
          <p className="start-subtitle">WU Control mirror</p>
          <fieldset className="start-agents">
            <legend>Opponent</legend>
            {agents.map((a) => (
              <label key={a.name} className="start-agent">
                <input
                  type="radio"
                  name="agent"
                  value={a.name}
                  checked={agent === a.name}
                  onChange={() => setAgent(a.name)}
                />
                <span className="start-agent-name">
                  {a.name}
                  {a.name === CHEATING && (
                    <em className="start-cheating">cheating: sees your hand</em>
                  )}
                </span>
                <span className="start-agent-about">{a.description}</span>
              </label>
            ))}
          </fieldset>
          <label className="start-seed">
            <span>Seed</span>
            <input
              inputMode="numeric"
              placeholder="random"
              value={seed}
              onChange={(e) => setSeed(e.target.value)}
              aria-invalid={!seedOk}
            />
          </label>
          {/* With a game in progress, going back to it comes first. */}
          <div className="start-buttons">
            {inProgress && (
              <Button onClick={() => (window.location.hash = "#/game")}>
                Return to Game
              </Button>
            )}
            <Button
              variant={inProgress ? "secondary" : "primary"}
              disabled={!agent || !seedOk || pending}
              onClick={start}
            >
              {pending ? "Starting…" : inProgress ? "New Game" : "Start"}
            </Button>
          </div>
          {error && <p className="game-error">{error}</p>}
        </section>
      </div>
    </Stage>
  );
}

const STALLS: Record<Stall, string> = {
  decisionsPerTurn: "too many decisions in one turn",
  turns: "too many turns",
  stateBasedIterations: "state-based actions did not settle",
  capacity: "a zone or queue was full",
};

/** A command's failure as the player should hear of it; none for an answer to a decision
 * that was answered already (a double click) or replaced, or a cast completed before it could
 * be taken back. */
function failure(e: unknown): string | undefined {
  if (!(e instanceof CommandError)) {
    return String(e);
  }
  const kind = e.error.kind;
  return kind === "notDeciding" ||
    kind === "staleDecision" ||
    kind === "notCancellable"
    ? undefined
    : e.message;
}

export function GameScreen({
  playback,
  starting,
}: {
  playback?: Playback;
  starting: boolean;
}) {
  if (starting || !playback) {
    return (
      <Stage>
        <div className="table start-screen">
          <Brand />
          <section className="start-panel" role="status">
            {starting ? (
              <>
                <h1 className="start-title">Starting game</h1>
                <p className="start-subtitle start-working">
                  Shuffling up and drawing opening hands
                  <i className="working-dots" aria-hidden>
                    <i />
                    <i />
                    <i />
                  </i>
                </p>
              </>
            ) : (
              <>
                <p className="start-subtitle">No game in progress.</p>
                <Button onClick={() => (window.location.hash = "#/")}>
                  New Game
                </Button>
              </>
            )}
          </section>
        </div>
      </Stage>
    );
  }
  return <Playing playback={playback} />;
}

/** A game in progress: the board, and over it the opening, a choice or the game's end. */
function Playing({ playback }: { playback: Playback }) {
  const [error, setError] = useState<string>();
  // The view an answer was sent from: until another arrives, the opponent is acting.
  const [answered, setAnswered] = useState<PlayerView>();
  const [showOver, setShowOver] = useState(true);
  // The picks still to send for a choice already made (a drag's follow-up, an overlay's
  // whole scry), one per decision as each arrives; and the overlay that made them, shown
  // until they are sent.
  const planned = useRef<Intent[]>([]);
  const [held, setHeld] = useState<Choice>();
  // The choice whose overlay is put aside to see the battlefield.
  const [peek, setPeek] = useState<number>();
  const { update, live, behind, log } = playback;
  const { session, view } = update;
  const status = view.status;
  // An answer is on its way, or the game plays on without the player: the opponent deciding,
  // or a step passing, shown or still to show.
  const waiting =
    answered === view ||
    behind ||
    status.kind === "thinking" ||
    status.kind === "playing";
  const fail = (e: unknown) => {
    setAnswered(undefined);
    planned.current = [];
    setHeld(undefined);
    const message = failure(e);
    if (message) setError(message);
  };
  const decision = status.kind === "deciding" ? status.decision : undefined;
  const answer = (index: CandidateIndex, then: Intent[] = []) => {
    if (!decision || waiting) return;
    setError(undefined);
    setAnswered(view);
    // Picks still waiting for their decision come after the new ones.
    planned.current = [...then, ...planned.current];
    choose(decision.id, index).catch(fail);
  };
  /** Takes back the cast in progress, and any picks planned for it. */
  const cancelCast = () => {
    if (!decision?.cancellable || waiting) return;
    setError(undefined);
    setAnswered(view);
    planned.current = [];
    cancel(decision.id).catch(fail);
  };

  // Each new decision takes the next planned pick, if it asks for it; otherwise the plan is
  // over (but for a pick that waits) and the player decides. The views of the game playing on
  // in between keep a plan with picks to come, and the overlay that made it; with none left,
  // the overlay gives way to the table so the player sees the game play on.
  useEffect(() => {
    if (status.kind === "thinking" || status.kind === "playing") {
      if (planned.current.length === 0) setHeld(undefined);
      return;
    }
    const { intent, index, rest } = decision
      ? follow(decision, planned.current)
      : { rest: [] };
    planned.current = [];
    if (index === undefined) {
      // A pick that waits stays planned while the player answers the decisions before it.
      if (intent?.waits && decision && decision.kind !== "priority") {
        planned.current = [intent, ...rest];
      }
      setHeld(undefined);
      return;
    }
    answer(index, rest);
    // Only a new view answers (`answer` changes with every render).
  }, [view]);

  const choice = held ?? (waiting ? undefined : choiceOf(view));
  const plan = (picks: Intent[]) => {
    if (!decision || !choice) return;
    const { index, rest } = follow(decision, picks);
    if (index === undefined) return;
    setHeld(choice);
    answer(index, rest);
  };
  const peeking = choice !== undefined && peek === choice.key;
  const opening = decision && isOpening(decision.kind) && !waiting;
  const over =
    (status.kind === "over" || status.kind === "stopped") && showOver;
  return (
    <Stage>
      <Board
        view={view}
        live={live}
        log={log}
        waiting={waiting}
        choose={answer}
        cancel={cancelCast}
        setFullControl={(on) =>
          setStops(session, on ? "exact" : "smart").catch(fail)
        }
        onLeave={() => (window.location.hash = "#/")}
        dimmed={opening || over || (choice !== undefined && !peeking)}
        inert={choice !== undefined}
      />
      {opening && <Opening view={view} decision={decision} onPick={answer} />}
      {choice && !peeking && (
        <ChoiceOverlay
          key={choice.key}
          view={view}
          choice={choice}
          busy={waiting || held !== undefined}
          onPlan={plan}
          onViewBattlefield={() => setPeek(choice.key)}
        />
      )}
      {choice && peeking && (
        <Button
          variant="secondary"
          className="overlay-return"
          onClick={() => setPeek(undefined)}
        >
          Back to <RulesText text={choice.title} />
        </Button>
      )}
      {status.kind === "over" && showOver && (
        <GameOver outcome={status.outcome} onClose={() => setShowOver(false)} />
      )}
      {status.kind === "stopped" && showOver && (
        <GameOver onClose={() => setShowOver(false)} />
      )}
      {error && (
        <p className="game-error game-toast" role="alert">
          {error}
        </p>
      )}
    </Stage>
  );
}

/** The opening decisions over the dimmed table: play or draw, keep or mulligan, and the
 * cards to put on the bottom, with the hand laid out to read. */
function Opening({
  view,
  decision,
  onPick,
}: {
  view: PlayerView;
  decision: DecisionView;
  onPick(index: CandidateIndex): void;
}) {
  const hand = view.objects.filter(
    (o) => o.zone === "hand" && o.owner === "you",
  );
  const picks = new Map(
    decision.candidates.flatMap((c, i) =>
      c.option.kind === "object" ? [[c.option.slot, i] as const] : [],
    ),
  );
  const buttons = decision.candidates.flatMap((c, i) =>
    c.option.kind === "object" ? [] : [{ label: c.label, index: i }],
  );
  const mulligans = view.players.you.mulligans;
  const [title, instruction] =
    decision.kind === "playOrDraw"
      ? ["Play or draw?", "Choose whether to take the first turn."]
      : decision.kind === "keepOrMulligan"
        ? [
            "Opening hand",
            mulligans === 0
              ? "Keep these seven, or shuffle them away and draw seven more."
              : `Mulligan ${mulligans}: keep these and put ${mulligans} on the bottom, or mulligan again.`,
          ]
        : [
            "Put a card on the bottom",
            "Choose a card to put on the bottom of your library.",
          ];
  return (
    <div className="opening" role="dialog" aria-label={title}>
      <div className="overlay-backdrop" />
      <h2 className="opening-title">{title}</h2>
      <p className="opening-instruction">{instruction}</p>
      <div className="opening-hand">
        {hand.map((o) => (
          <OpeningCard
            key={o.slot}
            object={o}
            pick={picks.get(o.slot)}
            onPick={onPick}
          />
        ))}
      </div>
      <div className="opening-buttons">
        {buttons.map((b, i) => (
          <Button
            key={b.index}
            variant={i === 0 ? "primary" : "secondary"}
            onClick={() => onPick(b.index)}
          >
            {b.label}
          </Button>
        ))}
      </div>
    </div>
  );
}

function OpeningCard({
  object,
  pick,
  onPick,
}: {
  object: ObjectView;
  pick?: CandidateIndex;
  onPick(index: CandidateIndex): void;
}) {
  const face = useFace(object);
  const name = object.name ?? "";
  return (
    <button
      type="button"
      className="opening-card"
      disabled={pick === undefined}
      data-card={name}
      onClick={() => pick !== undefined && onPick(pick)}
    >
      <Card
        card={face}
        width={176}
        highlight={pick === undefined ? undefined : "target"}
      />
    </button>
  );
}

/** The end of the game: who won, or why there is no winner. */
function GameOver({
  outcome,
  onClose,
}: {
  outcome?: OutcomeView;
  onClose(): void;
}) {
  const [title, detail] = !outcome
    ? ["Game stopped", "This game was replaced or closed."]
    : outcome.kind === "win"
      ? outcome.winner === "you"
        ? ["Victory", "You won the game."]
        : ["Defeat", "Your opponent won the game."]
      : outcome.kind === "draw"
        ? ["Draw", "The game is a draw."]
        : ["Stalled", `The engine gave up: ${STALLS[outcome.reason]}.`];
  return (
    <div className="game-over" role="dialog" aria-label={title}>
      <div className="overlay-backdrop" />
      <h2 className="game-over-title">{title}</h2>
      <p className="opening-instruction">{detail}</p>
      <div className="opening-buttons">
        <Button onClick={() => (window.location.hash = "#/")}>New game</Button>
        <Button variant="secondary" onClick={onClose}>
          View battlefield
        </Button>
      </div>
    </div>
  );
}
