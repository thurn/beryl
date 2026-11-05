// A game against an agent: the start screen, then the battlefield (game/Board.tsx) with the
// mulligan and game-over screens over it. Every view arrives as a `game-update` event; the
// screen answers the open decision through `choose`.
import { useEffect, useState } from "react";
import { Button } from "../components/Controls";
import { Card } from "../components/Card";
import { useCardArt } from "../cards";
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
import { isOpening } from "../game/decisions";
import { frameFace, unknownFace } from "../game/faces";
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
  const [error, setError] = useState<string>();
  // The view an answer was sent from: until another arrives, the opponent is acting.
  const [answered, setAnswered] = useState<PlayerView>();
  const [showOver, setShowOver] = useState(true);
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
  const { session, view } = update;
  const waiting = answered === view;
  const fail = (e: unknown) => {
    setAnswered(undefined);
    const message = failure(e);
    if (message) setError(message);
  };
  const status = view.status;
  const answer = (index: CandidateIndex) => {
    if (status.kind !== "deciding" || waiting) return;
    setError(undefined);
    setAnswered(view);
    choose(status.decision.id, index).catch(fail);
  };
  const decision = status.kind === "deciding" ? status.decision : undefined;
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
        dimmed={opening || over}
      />
      {opening && <Opening view={view} decision={decision} onPick={answer} />}
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
  const art = useCardArt(object.name ?? "");
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
        card={art ? frameFace(object, art) : unknownFace(name)}
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
