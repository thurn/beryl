// A game against an agent: the start screen, then the battlefield (game/Board.tsx) with the
// mulligan, choice (game/ChoiceOverlay.tsx) and game-over screens over it. Every view arrives
// as a `game-update` event; the screen answers the open decision through `choose`.
import { useEffect, useRef, useState } from "react";
import { Button } from "../components/Controls";
import { Card } from "../components/Card";
import { RulesText } from "../components/Mana";
import {
  choose,
  CommandError,
  listAgents,
  onGameUpdate,
  setStops,
  startGame,
  type AgentView,
  type CandidateIndex,
  type DecisionView,
  type GameUpdate,
  type ObjectView,
  type OutcomeView,
  type PlayerView,
  type Stall,
} from "../ipc";
import { Board } from "../game/Board";
import { ChoiceOverlay } from "../game/ChoiceOverlay";
import { choiceOf, type Choice } from "../game/choices";
import { intended, isOpening, type Intent } from "../game/decisions";
import { useFace } from "../game/faces";
import { Brand, Stage } from "./Table";

/** The latest view of the game in progress, from `game-update` events. Session numbers only
 * grow, so a late view from a replaced game is dropped. */
export function useGameUpdates(): GameUpdate | null {
  const [update, setUpdate] = useState<GameUpdate | null>(null);
  useEffect(() => {
    const unlisten = onGameUpdate((next) =>
      setUpdate((last) => (last && next.session < last.session ? last : next)),
    );
    return () => void unlisten.then((stop) => stop());
  }, []);
  return update;
}

/** The agent that plays on the true state, hidden cards included. */
const CHEATING = "mcts-omniscient";

/** Picks the opponent and an optional seed, and starts the game. */
export function StartScreen() {
  const [agents, setAgents] = useState<AgentView[]>([]);
  const [agent, setAgent] = useState<string>();
  const [seed, setSeed] = useState("");
  const [error, setError] = useState<string>();
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
  const start = () =>
    agent &&
    startGame(agent, seedValue).then(
      () => (window.location.hash = "#/game"),
      (e: unknown) => setError(String(e)),
    );
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
          <Button disabled={!agent || !seedOk} onClick={() => void start()}>
            Start
          </Button>
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
 * that was answered already (a double click) or replaced. */
function failure(e: unknown): string | undefined {
  if (!(e instanceof CommandError)) {
    return String(e);
  }
  const kind = e.error.kind;
  return kind === "notDeciding" || kind === "staleDecision"
    ? undefined
    : e.message;
}

export function GameScreen({ update }: { update: GameUpdate | null }) {
  if (!update) {
    return (
      <Stage>
        <div className="table start-screen">
          <section className="start-panel">
            <p className="start-subtitle">No game in progress.</p>
            <a className="start-link" href="#/">
              New game
            </a>
          </section>
        </div>
      </Stage>
    );
  }
  return <Playing update={update} />;
}

/** A game in progress: the board, and over it the opening, a choice or the game's end. */
function Playing({ update }: { update: GameUpdate }) {
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
  const { session, view } = update;
  const waiting = answered === view;
  const fail = (e: unknown) => {
    setAnswered(undefined);
    planned.current = [];
    setHeld(undefined);
    const message = failure(e);
    if (message) setError(message);
  };
  const status = view.status;
  const decision = status.kind === "deciding" ? status.decision : undefined;
  const answer = (index: CandidateIndex, then: Intent[] = []) => {
    if (!decision || waiting) return;
    setError(undefined);
    setAnswered(view);
    planned.current = then;
    choose(decision.id, index).catch(fail);
  };

  // Each new view takes the next planned pick, if it asks for it; otherwise the plan is over
  // and the player decides.
  useEffect(() => {
    const [next, ...rest] = planned.current;
    planned.current = [];
    const index = next && decision ? intended(decision, next) : undefined;
    if (index === undefined) {
      setHeld(undefined);
      return;
    }
    answer(index, rest);
    // Only a new view answers (`answer` changes with every render).
  }, [view]);

  const choice = held ?? (waiting ? undefined : choiceOf(view));
  const plan = (picks: Intent[]) => {
    const [first, ...rest] = picks;
    const index = first && decision ? intended(decision, first) : undefined;
    if (index === undefined || !choice) return;
    setHeld(choice);
    answer(index, rest);
  };
  const peeking = choice !== undefined && peek === choice.key;
  const opening = decision && isOpening(decision.kind) && !waiting;
  const over = status.kind !== "deciding" && showOver;
  return (
    <Stage>
      <Board
        view={view}
        waiting={waiting}
        choose={answer}
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
