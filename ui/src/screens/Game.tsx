// A game against an agent: the start screen, then the battlefield (game/Board.tsx) with the
// mulligan, choice (game/ChoiceOverlay.tsx) and game-over screens over it. Views arrive as
// `game-update` events and play back at a readable pace (game/playback.ts); the screen answers
// the open decision through `choose`. A reloaded page takes up the game in progress.
import { useEffect, useRef, useState } from "react";
import { Button } from "../components/Controls";
import { Card, CardBack } from "../components/Card";
import { Coin } from "../components/Player";
import { RulesText } from "../components/Mana";
import {
  cancel,
  choose,
  CommandError,
  concede,
  listAgents,
  setStops,
  type AgentView,
  type CandidateIndex,
  type DecisionView,
  type LossReason,
  type ObjectView,
  type OutcomeView,
  type PlayerView,
  type SessionId,
  type Stall,
  type StopsSetting,
} from "../ipc";
import { Board } from "../game/Board";
import { ChoiceOverlay } from "../game/ChoiceOverlay";
import { choiceOf, type Choice } from "../game/choices";
import { follow, isOpening, twins, type Intent } from "../game/decisions";
import { useFace } from "../game/faces";
import { useHoverInspect, type HoverHandlers } from "../game/Piles";
import { eventsHold, type Playback } from "../game/playback";
import { Brand, Stage } from "../game/Stage";
import { sameStops, savedStops, saveStops } from "../game/stops";

/** The agent that plays on the true state, hidden cards included. */
const CHEATING = "mcts-omniscient";
/** The opponent offered first: strong, and quick to answer. */
const DEFAULT_AGENT = "expert";
/** Where the start screen remembers the last opponent and seed. */
const LAST_GAME = "beryl.lastGame";

/** The opponent and seed of the last game started here, if the browser kept them. */
function lastGame(): { agent?: string; seed?: string } {
  try {
    const saved: unknown = JSON.parse(localStorage.getItem(LAST_GAME) ?? "{}");
    if (typeof saved !== "object" || saved === null) return {};
    const { agent, seed } = saved as Record<string, unknown>;
    return {
      agent: typeof agent === "string" ? agent : undefined,
      seed: typeof seed === "string" ? seed : undefined,
    };
  } catch {
    return {};
  }
}

function rememberGame(agent: string, seed: string) {
  try {
    localStorage.setItem(LAST_GAME, JSON.stringify({ agent, seed }));
  } catch {
    // Storage may be blocked; the start screen then offers the defaults.
  }
}

/** Picks the opponent and an optional seed, and starts the game; or returns to the game in
 * progress, `inProgress`, which a new game concedes once the player confirms it. Enter does
 * what the primary button does. */
export function StartScreen({
  inProgress,
  onStart,
}: {
  inProgress?: SessionId;
  onStart(agent: string, seed?: number): Promise<void>;
}) {
  const [agents, setAgents] = useState<AgentView[]>([]);
  const [agent, setAgent] = useState<string>();
  const [seed, setSeed] = useState(() => lastGame().seed ?? "");
  const [error, setError] = useState<string>();
  const [pending, setPending] = useState(false);
  // A new game was asked for with one in progress: the next click concedes it.
  const [confirming, setConfirming] = useState(false);
  useEffect(() => {
    listAgents().then((list) => {
      setAgents(list);
      const offered = (name?: string) =>
        list.find((a) => a.name === name)?.name;
      setAgent(
        (a) =>
          a ??
          offered(lastGame().agent) ??
          offered(DEFAULT_AGENT) ??
          list[0]?.name,
      );
    });
  }, []);
  const seedValue = seed.trim() === "" ? undefined : Number(seed);
  const seedOk =
    seedValue === undefined ||
    (Number.isInteger(seedValue) && seedValue >= 0 && seedValue < 2 ** 32);
  const startable = agent !== undefined && seedOk && !pending;
  const starting = useRef(false);
  const start = () => {
    if (!agent || !startable || starting.current) return;
    if (inProgress !== undefined && !confirming) {
      setConfirming(true);
      return;
    }
    starting.current = true;
    rememberGame(agent, seed.trim());
    setPending(true);
    setError(undefined);
    // The new game replaces the old one whether or not the concession lands first.
    const conceded =
      inProgress === undefined
        ? Promise.resolve()
        : concede(inProgress).catch(() => undefined);
    void conceded
      .then(() => onStart(agent, seedValue))
      .then(
        () => (window.location.hash = "#/game"),
        (e: unknown) => {
          starting.current = false;
          setPending(false);
          setError(String(e));
        },
      );
  };
  // Enter takes the primary button from anywhere on the screen but a button, which Enter
  // presses: back to the game in progress, else a new one.
  const onEnter = useRef<(e: KeyboardEvent) => void>(undefined);
  onEnter.current = (e) => {
    if (e.key !== "Enter" || e.repeat) return;
    if (e.target instanceof Element && e.target.closest("button")) return;
    e.preventDefault();
    if (inProgress !== undefined) window.location.hash = "#/game";
    else start();
  };
  useEffect(() => {
    const listener = (e: KeyboardEvent) => onEnter.current?.(e);
    window.addEventListener("keydown", listener);
    return () => window.removeEventListener("keydown", listener);
  }, []);
  return (
    <Stage>
      <div className="table start-screen">
        <Brand />
        <section className="start-panel">
          <h1 className="start-title">New Game</h1>
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
                  {a.title}
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
            {inProgress !== undefined && (
              <Button onClick={() => (window.location.hash = "#/game")}>
                Return to Game
              </Button>
            )}
            <Button
              variant={inProgress !== undefined ? "secondary" : "primary"}
              disabled={!startable}
              onClick={start}
            >
              {pending
                ? "Starting…"
                : inProgress === undefined
                  ? "Start"
                  : confirming
                    ? "Concede & Start"
                    : "New Game"}
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
  resuming,
}: {
  playback?: Playback;
  starting: boolean;
  /** The game in progress is still being asked for. */
  resuming: boolean;
}) {
  if (resuming && !playback) {
    return (
      <Stage>
        <div className="table start-screen">
          <Brand />
        </div>
      </Stage>
    );
  }
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
  // Each game starts with a clean screen.
  return <Playing key={playback.update.session} playback={playback} />;
}

/** A game in progress: the board, and over it the opening, a choice or the game's end. */
function Playing({ playback }: { playback: Playback }) {
  const [error, setError] = useState<string>();
  // The view an answer was sent from: until another arrives, the opponent is acting.
  const [answered, setAnswered] = useState<PlayerView>();
  // The picks still to send for a choice already made (a drag's follow-up, an overlay's
  // whole scry), one per decision as each arrives; and the overlay that made them, shown
  // until they are sent.
  const planned = useRef<Intent[]>([]);
  const [held, setHeld] = useState<Choice>();
  // The choice whose overlay is put aside to see the battlefield.
  const [peek, setPeek] = useState<number>();
  const { update, live, behind, skip, log } = playback;
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
  /** Takes back the cast in progress, and any picks planned for it. The choice it was asking,
   * if put aside, is done with: asked again, it is shown. */
  const cancelCast = () => {
    if (!decision?.cancellable || waiting) return;
    setError(undefined);
    setAnswered(view);
    planned.current = [];
    setPeek(undefined);
    cancel(decision.id).catch(fail);
  };

  // The stops the player chose, here or in an earlier game, until the game holds them: the
  // session takes a change only at the player's decision, so one made while the opponent acts
  // waits for it, and a new game takes up the last game's.
  const [chosen, setChosen] = useState<StopsSetting | undefined>(savedStops);
  const stops = chosen ?? view.stops;
  const chooseStops = (next: StopsSetting) => {
    saveStops(next);
    setChosen(next);
  };
  useEffect(() => {
    if (!chosen || !decision || waiting || sameStops(chosen, view.stops))
      return;
    setStops(session, chosen).catch((e: unknown) => {
      // A decision answered meanwhile: the next one takes them.
      const message = failure(e);
      if (message) setError(message);
    });
  }, [view, chosen, waiting]);

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
  /** Sends the picks the opening planned, the first now and the rest as they are asked. */
  const planOpening = (picks: Intent[]) => {
    if (!decision) return;
    const { index, rest } = follow(decision, picks);
    if (index !== undefined) answer(index, rest);
  };
  const peeking = choice !== undefined && peek === choice.key;
  // Before the first turn the opening stays over the table, the opponent's decisions too.
  const opening =
    view.turn.number === 0 &&
    status.kind !== "over" &&
    status.kind !== "stopped";
  // A pick still planned answers the decision it waits for: the player has nothing to do.
  const openDecision =
    decision &&
    isOpening(decision.kind) &&
    !waiting &&
    planned.current.length === 0
      ? decision
      : undefined;
  const ended = status.kind === "over" || status.kind === "stopped";
  const result = useResult(ended, live, view);
  const over = result.state === "shown";
  return (
    <Stage>
      <Board
        view={view}
        live={live}
        log={log}
        waiting={waiting}
        onSkip={behind ? skip : undefined}
        choose={answer}
        cancel={cancelCast}
        stops={stops}
        setStops={chooseStops}
        onLeave={() => (window.location.hash = "#/")}
        onConcede={
          ended
            ? undefined
            : () =>
                void concede(session).catch((e: unknown) => {
                  // A second concession, or one the game's end overtook.
                  if (!(
                    e instanceof CommandError && e.error.kind === "ended"
                  )) {
                    fail(e);
                  }
                })
        }
        dimmed={opening || over || (choice !== undefined && !peeking)}
        inert={choice !== undefined}
      />
      {opening && (
        <Opening
          view={view}
          decision={openDecision}
          onPick={answer}
          onPlan={planOpening}
        />
      )}
      {choice && !peeking && (
        <ChoiceOverlay
          key={choice.key}
          view={view}
          choice={choice}
          busy={waiting || held !== undefined}
          onPlan={plan}
          onViewBattlefield={() => setPeek(choice.key)}
          onCancel={decision?.cancellable ? cancelCast : undefined}
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
      {over && (
        <GameOver
          outcome={status.kind === "over" ? status.outcome : undefined}
          onClose={result.putAside}
        />
      )}
      {result.state === "aside" && (
        <Button
          variant="secondary"
          className="overlay-return"
          onClick={result.show}
        >
          Back to{" "}
          {resultTitle(status.kind === "over" ? status.outcome : undefined)}
        </Button>
      )}
      {error && (
        <p className="game-error game-toast" role="alert">
          {error}
        </p>
      )}
    </Stage>
  );
}

/** The starting hand size (CR 103.5). */
const STARTING_HAND = 7;
/** How long the coin spins once the opponent wins the flip, however soon they choose. */
const FLIP_MS = 1500;

/** The opening over the dimmed table, from the coin flip until both hands are kept: who plays
 * first, keep or mulligan, and the cards to put on the bottom, which fill slots and go back on
 * a click until confirmed. While the opponent decides it shows where the player stands: a hand
 * they mulliganed lies face down. */
function Opening({
  view,
  decision: asked,
  onPick,
  onPlan,
}: {
  view: PlayerView;
  /** The decision the player may answer now; none while the game goes on without them. */
  decision?: DecisionView;
  onPick(index: CandidateIndex): void;
  onPlan(picks: Intent[]): void;
}) {
  const { you } = view.players;
  const { flipWinner } = view.turn;
  // When the opponent won the flip, the coin spins a while before it shows their choice, made
  // at once or not, as the opening first shows: before anyone has declared.
  const [spinning, setSpinning] = useState(
    () =>
      flipWinner === "opponent" &&
      you.mulligans === 0 &&
      you.mulligan === "undeclared",
  );
  useEffect(() => {
    if (!spinning) return;
    const spin = window.setTimeout(() => setSpinning(false), FLIP_MS);
    return () => window.clearTimeout(spin);
  }, [spinning]);
  const starting = spinning ? null : view.turn.starting;
  const decision = spinning ? undefined : asked;
  // What the opponent decides while the player waits: who goes first, the coin rocking
  // between the seats, and then its hand, which it looks through.
  const choosingFirst =
    !spinning && starting === null && flipWinner === "opponent";
  const considering =
    !decision && starting !== null && view.status.kind === "thinking";
  const opponentHand = view.objects.filter(
    (o) => o.zone === "hand" && o.owner === "opponent",
  ).length;
  const kind = decision?.kind;
  // The cards picked for the bottom, kept as they were: those sent are gone from the hand.
  const [bottom, setBottom] = useState<ObjectView[]>([]);
  const inHand = view.objects.filter(
    (o) => o.zone === "hand" && o.owner === "you",
  );
  const hand = inHand.filter((o) => !bottom.some((b) => b.slot === o.slot));
  const unsent = bottom.filter((b) => inHand.some((o) => o.slot === b.slot));
  // One card per mulligan goes on the bottom; any already there count.
  const toBottom =
    kind === "putOnBottom"
      ? Math.max(1, inHand.length - (STARTING_HAND - you.mulligans))
      : 0;
  const slots = Math.max(toBottom, bottom.length);
  // The cards are small to fit seven: hovering one shows it whole.
  const [inspector, inspect] = useHoverInspect();
  const picking = kind === "putOnBottom";
  // A card clicked moves out from under the pointer, which never leaves it: its inspector goes.
  const toggle = (o: ObjectView) => {
    inspect(o).onPointerLeave();
    setBottom((picked) =>
      picked.some((b) => b.slot === o.slot)
        ? picked.filter((b) => b.slot !== o.slot)
        : picked.length < toBottom
          ? [...picked, o]
          : picked,
    );
  };
  const confirm = () =>
    onPlan(
      unsent.map((o) => ({
        kind: "putOnBottom",
        wants: (c) => c.kind === "object" && c.slot === o.slot,
        // The engine offers one of interchangeable cards (architecture §7.2): its slot fills
        // with the card that went.
        alike: (c) => {
          if (c.kind !== "object") return false;
          const other = inHand.find((h) => h.slot === c.slot);
          return other !== undefined && twins(other, o);
        },
        picked: (c) => {
          const sent =
            c.kind === "object" && inHand.find((h) => h.slot === c.slot);
          if (!sent || sent.slot === o.slot) return;
          // A twin picked too trades places with it.
          setBottom((picked) =>
            picked.map((b) =>
              b.slot === o.slot ? sent : b.slot === sent.slot ? o : b,
            ),
          );
        },
      })),
    );
  const title =
    starting === null
      ? flipWinner === "you"
        ? "You won the flip"
        : "Opponent won the flip"
      : picking
        ? "Put on the bottom"
        : you.mulligan === "willMulligan"
          ? "Mulligan"
          : you.mulligans === 0
            ? "Opening hand"
            : `Mulligan to ${STARTING_HAND - you.mulligans}`;
  const buttons =
    decision && !picking
      ? decision.candidates.map((c, i) => ({ label: c.label, index: i }))
      : [];
  const facedown = you.mulligan === "willMulligan";
  return (
    <div className="opening" role="dialog" aria-label={title}>
      <div className="overlay-backdrop" />
      <div className="opening-coin" data-starting={starting ?? undefined}>
        <Coin
          key={starting ?? "spinning"}
          spinning={starting === null && !choosingFirst}
          choosing={choosingFirst}
          first={starting === "you" || choosingFirst}
        />
        {starting && (
          <span className="opening-coin-label">
            {starting === "you" ? "On the play" : "On the draw"}
          </span>
        )}
      </div>
      <h2 className="opening-title">{title}</h2>
      <div className="opening-hand">
        {starting !== null &&
          hand.map((o) =>
            facedown ? (
              <CardBack key={o.slot} width={176} />
            ) : (
              <OpeningCard
                key={o.slot}
                object={o}
                hover={inspect(o)}
                onPick={
                  picking && bottom.length < toBottom
                    ? () => toggle(o)
                    : undefined
                }
              />
            ),
          )}
      </div>
      {slots > 0 && (
        <div className="opening-slots" aria-label="Bottom of your library">
          {Array.from({ length: slots }, (_, i) => {
            const o = bottom[i];
            return o ? (
              <OpeningCard
                key={o.slot}
                object={o}
                hover={inspect(o)}
                width={120}
                onPick={picking ? () => toggle(o) : undefined}
              />
            ) : (
              <i key={`empty-${i}`} className="opening-slot" />
            );
          })}
        </div>
      )}
      <div className="opening-buttons">
        {picking && (
          <Button disabled={unsent.length !== toBottom} onClick={confirm}>
            Done
          </Button>
        )}
        {buttons.map((b, i) => (
          <Button
            key={b.index}
            variant={i === 0 ? "primary" : "secondary"}
            onClick={() => onPick(b.index)}
          >
            {b.label}
          </Button>
        ))}
        {considering ? (
          <div
            className="opening-considering"
            role="status"
            aria-label="Opponent's hand"
          >
            {Array.from({ length: opponentHand }, (_, i) => (
              <CardBack
                key={i}
                width={44}
                style={{
                  rotate: `${(i - (opponentHand - 1) / 2) * 7}deg`,
                  animationDelay: `${i * 0.18}s`,
                }}
              />
            ))}
          </div>
        ) : (
          !decision &&
          !choosingFirst && (
            <i className="working-dots opening-waiting" role="status">
              <i />
              <i />
              <i />
            </i>
          )
        )}
      </div>
      {inspector}
    </div>
  );
}

function OpeningCard({
  object,
  width = 176,
  onPick,
  hover,
}: {
  object: ObjectView;
  width?: number;
  onPick?(): void;
  hover: HoverHandlers;
}) {
  const face = useFace(object);
  return (
    <button
      type="button"
      className="opening-card"
      disabled={!onPick}
      data-card={object.name ?? ""}
      data-slot={object.slot}
      onClick={onPick}
      {...hover}
    >
      <Card
        card={face}
        width={width}
        highlight={onPick ? "target" : undefined}
      />
    </button>
  );
}

/** The result over the table once the game has ended and its final events have played: shown
 * until the player puts it aside to see the battlefield, and back on asking. */
function useResult(ended: boolean, live: boolean, view: PlayerView) {
  // A game that ended in the past (a reloaded page, a recording) shows its result at once.
  const [state, setState] = useState<"waiting" | "shown" | "aside">(() =>
    ended && !live ? "shown" : "waiting",
  );
  useEffect(() => {
    if (!ended) return;
    const timer = setTimeout(
      () => setState((s) => (s === "waiting" ? "shown" : s)),
      live ? eventsHold(view) : 0,
    );
    return () => clearTimeout(timer);
    // Only a new view ends the game.
  }, [view]);
  return {
    state,
    putAside: () => setState("aside"),
    show: () => setState("shown"),
  };
}

function resultTitle(outcome?: OutcomeView): string {
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

/** Why the game ended, in a word beside its icon. */
const LOSSES: Record<LossReason, string> = {
  life: "Life",
  library: "Library",
  effect: "Card effect",
  concede: "Conceded",
};

/** The end of the game: who won, and why. */
function GameOver({
  outcome,
  onClose,
}: {
  outcome?: OutcomeView;
  onClose(): void;
}) {
  const title = resultTitle(outcome);
  const reason =
    outcome?.kind === "win"
      ? { icon: outcome.reason, text: LOSSES[outcome.reason] }
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
      <div className="opening-buttons">
        <Button onClick={() => (window.location.hash = "#/")}>New Game</Button>
        <Button variant="secondary" onClick={onClose}>
          View Battlefield
        </Button>
      </div>
    </div>
  );
}

/** A small line icon for why the game ended. */
function ReasonIcon({ reason }: { reason: LossReason | "stall" }) {
  const paths: Record<LossReason | "stall", string> = {
    // A heart: life.
    life: "M12 20s-7-4.4-7-10a4 4 0 0 1 7-2.6A4 4 0 0 1 19 10c0 5.6-7 10-7 10z",
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
