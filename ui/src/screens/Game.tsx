// A game against an agent: the start screen, then the battlefield (game/Board.tsx) with the
// mulligan, choice (game/ChoiceOverlay.tsx) and game-over screens over it. Views arrive as
// `game-update` events and play back at a readable pace (game/playback.ts); the screen answers
// the open decision through `choose`. A reloaded page takes up the game in progress.
import { useEffect, useRef, useState, type CSSProperties } from "react";
import { Button, Dock, DockButtons } from "../components/Controls";
import { Card, CardBack } from "../components/Card";
import { Coin } from "../components/Player";
import { RulesText } from "../components/Mana";
import { faceEntry } from "../cards";
import {
  cancel,
  undo,
  passOn,
  choose,
  CommandError,
  concede,
  exportBugs,
  listAgents,
  setStops,
  type AgentView,
  type CandidateIndex,
  type Culprit,
  type DecisionView,
  type LossReason,
  type ObjectView,
  type OutcomeView,
  type PlayerView,
  type SessionId,
  type Stall,
  type StopsSetting,
  type TargetRef,
} from "../ipc";
import { Board } from "../stage/switch";
import { FloatPrompt, PromptRow } from "../game/FloatPrompt";
import { ChoiceOverlay } from "../game/ChoiceOverlay";
import { choiceOf, choiceSlots, floats, type Choice } from "../game/choices";
import {
  follow,
  isOpening,
  twins,
  warnedTarget,
  type Intent,
} from "../game/decisions";
import { BLANK, paragraphOf, useAbilityFace, useFace } from "../game/faces";
import { useHoverInspect, type HoverHandlers } from "../game/Piles";
import { eventsHold, HOLD, type Playback } from "../game/playback";
import { Stage } from "../game/Stage";
import { PhoneBoard } from "../mobile/PhoneBoard";
import { usePhone } from "../mobile/phone";
import { sameStops, savedStops, saveStops } from "../game/stops";
import { play } from "../audio/sound";
import { SoundCorner } from "../audio/SoundControls";
import { SpecOpponentMove, SpecReplayPlay } from "./Testing";

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
  resumed,
  onStart,
  exportReports,
}: {
  inProgress?: SessionId;
  /** The game in progress was resumed at launch and not yet gone back to: a new game
   * concedes it without asking. */
  resumed: boolean;
  onStart(agent: string, seed?: number): Promise<void>;
  exportReports: boolean;
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
    if (inProgress !== undefined && !resumed && !confirming) {
      setConfirming(true);
      return;
    }
    starting.current = true;
    play("start");
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
        <SoundCorner />
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
          {exportReports && <ExportReports />}
        </section>
      </div>
    </Stage>
  );
}

function ExportReports() {
  const [pending, setPending] = useState(false);
  const [status, setStatus] = useState("");
  const save = async () => {
    setPending(true);
    setStatus("");
    try {
      const count = await exportBugs();
      if (count === 0) setStatus("No reports to export.");
      else if (count !== null) setStatus("ZIP saved.");
    } catch {
      setStatus("Couldn’t save reports.");
    } finally {
      setPending(false);
    }
  };
  return (
    <>
      <Button
        size="small"
        variant="secondary"
        disabled={pending}
        title="Includes screenshots, replays, and logs"
        onClick={() => void save()}
      >
        {pending ? "Exporting…" : "Export Bug Reports"}
      </Button>
      {status && (
        <p className="start-working" role="status">
          {status}
        </p>
      )}
    </>
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
    kind === "notCancellable" ||
    kind === "nothingToUndo" ||
    kind === "noPassOn"
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
        <div className="table start-screen"></div>
      </Stage>
    );
  }
  if (starting || !playback) {
    return (
      <Stage>
        <div className="table start-screen">
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
  const params = new URLSearchParams(window.location.search);
  const live = params.get("live");
  return (
    <>
      <Playing key={playback.update.session} playback={playback} />
      {live && params.has("specReplay") && <SpecReplayPlay url={live} />}
      {live && params.has("specOpponent") && (
        <SpecOpponentMove url={live} update={playback.update} />
      )}
    </>
  );
}

/** The longest a choice over the table waits for its view's events to be seen, in ms. */
const CHOICE_WAIT = 1500;

/** A game in progress: the board, and over it the opening, a choice or the game's end. */
function Playing({ playback }: { playback: Playback }) {
  const [error, setError] = useState<string>();
  // The view an answer was sent from: until another arrives, the opponent is acting.
  const [answered, setAnswered] = useState<PlayerView>();
  const [removal, setRemoval] = useState<{
    view: PlayerView;
    slot: number;
    index: CandidateIndex;
    then: Intent[];
  }>();
  // The picks still to send for a choice already made (a drag's follow-up, an overlay's
  // whole scry), one per decision as each arrives; and the overlay that made them, shown
  // until they are sent.
  const planned = useRef<Intent[]>([]);
  // Keep the tray's faces as shown while its planned answers play through newer views.
  const [held, setHeld] = useState<{ choice: Choice; view: PlayerView }>();
  // The choice whose overlay is put aside to see the battlefield.
  const [peek, setPeek] = useState<number>();
  const { update, live, spotlight, triggerFlights, behind, skip, log } =
    playback;
  const { gather, release, latest } = playback;
  const { session, view } = update;
  const status = view.status;
  const confirmingRemoval = removal?.view === view ? removal : undefined;
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
    release();
    setHeld(undefined);
    const message = failure(e);
    if (message) {
      play("no");
      setError(message);
    }
  };
  const decision = status.kind === "deciding" ? status.decision : undefined;
  // A spec may open directly at a target choice. Give the reviewer its paused state
  // before applying the normal shortcut for a lone target.
  const canAutoPick = useRef(
    !new URLSearchParams(window.location.search).has("spec"),
  );
  /** Stages a removal before sending even the count of an optional target. */
  const answer = (index: CandidateIndex, then: Intent[] = []) => {
    if (confirmingRemoval) return;
    if (!decision || waiting) {
      play("no");
      return;
    }
    const slot = warnedTarget(decision, index, then[0]);
    if (slot !== undefined) {
      setRemoval({ view, slot, index, then });
      return;
    }
    submit(index, then);
  };
  /** Answers the decision as the player picked: heard, or refused while the game is busy. */
  const submit = (index: CandidateIndex, then: Intent[]) => {
    if (!decision || waiting) {
      play("no");
      return;
    }
    canAutoPick.current = true;
    play("select");
    // Keep intermediate engine questions off the table for declarations and payments
    // that the player confirmed as one action.
    if (then.length > 0) {
      if (decision.kind === "attacker" || decision.kind === "blocker")
        gather("combat");
      else if (decision.prompt.purpose === "waterbend") gather();
    }
    send(index, then);
  };
  /** Sends an answer, the player's or one taken for them (a planned pick, a lone target). */
  const send = (
    index: CandidateIndex,
    then: Intent[] = [],
    continuation?: boolean,
  ) => {
    if (!decision || waiting) return;
    setError(undefined);
    setAnswered(view);
    // Picks still waiting for their decision come after the new ones.
    planned.current = [...then, ...planned.current];
    choose(decision.id, index, continuation).catch(fail);
  };
  /** Takes back the cast in progress, and any picks planned for it. The choice it was asking,
   * if put aside, is done with: asked again, it is shown. */
  const cancelCast = () => {
    if (!decision?.cancellable || waiting) return;
    play("back");
    setError(undefined);
    setAnswered(view);
    planned.current = [];
    setPeek(undefined);
    cancel(decision.id).catch(fail);
  };

  /** Takes back the player's latest answer, dropping any picks planned. */
  const undoAnswer = () => {
    if (!decision?.undo || waiting) return;
    play("back");
    setError(undefined);
    setAnswered(view);
    planned.current = [];
    setPeek(undefined);
    undo(decision.id).catch(fail);
  };
  /** Passes on to the player's next stop. */
  const passOnward = () => {
    if (!decision?.passOn || waiting) return;
    play("select");
    setError(undefined);
    setAnswered(view);
    passOn(decision.id).catch(fail);
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
      // Optional picks the engine did not ask for (a search's done, once it took as many as
      // it may) are over once the game plays on.
      if (planned.current.every((i) => i.optional)) {
        planned.current = [];
        setHeld(undefined);
      }
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
      // A target with no alternative is part of the preceding action. Undo must
      // reach before that action rather than silently answering the target again.
      // A payment waiting for its decision must survive the automatic answer.
      const lone =
        canAutoPick.current && !view.undone && decision
          ? loneTarget(decision, view)
          : undefined;
      if (lone !== undefined) {
        send(lone, [], true);
        return;
      }
      setHeld(undefined);
      return;
    }
    send(index, rest, intent?.continuation);
    // Only a new view answers (`send` changes with every render).
  }, [view]);

  // While gathering, each view held back that asks for the next planned pick gets it at once;
  // the first that doesn't, or the game playing on without the player, ends the gathering,
  // and the views held back show as one, for the effect above to take up.
  useEffect(() => {
    if (!latest) return;
    const status = latest.view.status;
    if (status.kind === "deciding" && planned.current.length > 0) {
      const { decision } = status;
      const { intent, index, rest } = follow(decision, planned.current);
      if (index !== undefined) {
        planned.current = rest;
        choose(decision.id, index, intent?.continuation).catch(fail);
        return;
      }
    }
    release();
  }, [latest]);

  // A choice over the table waits until what its view shows has been seen (the spell that asks
  // it cast and resolving, what it exiled), rather than covering it as it plays.
  const [seen, setSeen] = useState<PlayerView>();
  useEffect(() => {
    if (!live || status.kind !== "deciding" || view.events.length === 0) {
      setSeen(view);
      return;
    }
    const wait = setTimeout(
      () => setSeen(view),
      Math.min(eventsHold(view), CHOICE_WAIT),
    );
    return () => clearTimeout(wait);
  }, [view, live]);
  const asked = held?.choice ?? (waiting ? undefined : choiceOf(view));
  const choice = held?.choice ?? (seen === view ? asked : undefined);
  const plan = (picks: Intent[]) => {
    if (!decision || !choice) return;
    const { index, rest } = follow(decision, picks);
    if (index === undefined) return;
    setHeld({ choice, view });
    // The picks after this one go out unseen under the choice, and what they all did plays
    // once it gives way to the table.
    if (rest.length > 0) gather();
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
    decision && isOpening(decision) && !waiting && planned.current.length === 0
      ? decision
      : undefined;
  const ended = status.kind === "over" || status.kind === "stopped";
  const result = useResult(ended, live, view);
  const over = result.state === "shown";
  const phone = usePhone();
  const leave = () => (window.location.hash = "#/");
  const board = (
    <Board
      view={view}
      live={live}
      spotlight={spotlight}
      triggerFlights={triggerFlights}
      log={log}
      waiting={waiting}
      onSkip={behind ? skip : undefined}
      choose={answer}
      cancel={confirmingRemoval ? undefined : cancelCast}
      undo={confirmingRemoval ? undefined : undoAnswer}
      passOn={passOnward}
      stops={stops}
      setStops={chooseStops}
      onLeave={leave}
      onConcede={
        ended
          ? undefined
          : () =>
              void concede(session).catch((e: unknown) => {
                // A second concession, or one the game's end overtook.
                if (!(e instanceof CommandError && e.error.kind === "ended")) {
                  fail(e);
                }
              })
      }
      // A question in the floating prompt leaves the table lit.
      dimmed={
        opening || over || (choice !== undefined && !floats(choice) && !peeking)
      }
      // Nor does it take input while a choice waits to be shown.
      inert={asked !== undefined || confirmingRemoval !== undefined}
      traySlots={
        choice && !peeking && !floats(choice)
          ? [
              ...choiceSlots(choice),
              ...(choice.source?.slot == null ? [] : [choice.source.slot]),
            ]
          : undefined
      }
    />
  );
  const overlays = (
    <>
      {confirmingRemoval && (
        <div
          onKeyDown={(event) => {
            if (event.key === "Escape") {
              event.preventDefault();
              setRemoval(undefined);
            }
          }}
        >
          <FloatPrompt
            title={`Target your ${view.objects.find((o) => o.slot === confirmingRemoval.slot)?.name ?? "permanent"}?`}
          >
            <PromptRow>
              <Button
                onClick={() => {
                  setRemoval(undefined);
                  submit(confirmingRemoval.index, confirmingRemoval.then);
                }}
              >
                Confirm
              </Button>
              <Button
                variant="secondary"
                className="removal-cancel"
                autoFocus
                onClick={() => setRemoval(undefined)}
              >
                Cancel
              </Button>
            </PromptRow>
          </FloatPrompt>
        </div>
      )}
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
          view={held?.view ?? view}
          choice={choice}
          busy={waiting || held !== undefined}
          onPlan={plan}
          onViewBattlefield={() => setPeek(choice.key)}
          onCancel={decision?.cancellable ? cancelCast : undefined}
        />
      )}
      {choice && peeking && (
        <Dock>
          <Button
            variant="secondary"
            className="overlay-return"
            onClick={() => setPeek(undefined)}
          >
            Back to <RulesText text={choice.title} />
          </Button>
        </Dock>
      )}
      {over && (
        <GameOver
          outcome={status.kind === "over" ? status.outcome : undefined}
          onClose={result.putAside}
        />
      )}
      {result.state === "aside" && (
        <Dock>
          <Button
            variant="secondary"
            className="overlay-return"
            onClick={result.show}
          >
            Back to{" "}
            {resultTitle(status.kind === "over" ? status.outcome : undefined)}
          </Button>
        </Dock>
      )}
      {error && (
        <p className="game-error game-toast" role="alert">
          {error}
        </p>
      )}
    </>
  );
  if (!phone) {
    return (
      <Stage>
        {board}
        {overlays}
      </Stage>
    );
  }
  // On a phone the board is drawn for the phone; what shows over it is still the desktop's,
  // on its stage over the board, until the phone has its own (by-7pm's designs).
  const overlaid =
    opening ||
    choice !== undefined ||
    confirmingRemoval !== undefined ||
    over ||
    result.state === "aside" ||
    error !== undefined;
  return (
    <>
      <PhoneBoard
        view={view}
        waiting={waiting || confirmingRemoval !== undefined}
        choose={answer}
        passOn={passOnward}
        cancel={cancelCast}
        onLeave={leave}
      />
      {overlaid && <Stage>{overlays}</Stage>}
    </>
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
  const choosingFirst =
    !spinning && starting === null && flipWinner === "opponent";
  const considering = view.handling?.zone === "hand";
  const picking = decision?.prompt.purpose === "put-on-bottom";
  // The cards picked for the bottom, kept as they were: those sent are gone from the hand.
  const [bottom, setBottom] = useState<ObjectView[]>([]);
  const inHand = view.objects.filter(
    (o) => o.zone === "hand" && o.owner === "you",
  );
  const hand = inHand.filter((o) => !bottom.some((b) => b.slot === o.slot));
  const unsent = bottom.filter((b) => inHand.some((o) => o.slot === b.slot));
  // One card per mulligan goes on the bottom; any already there count.
  const toBottom = picking
    ? Math.max(1, inHand.length - (STARTING_HAND - you.mulligans))
    : 0;
  const slots = Math.max(toBottom, bottom.length);
  // The cards are small to fit seven: hovering one shows it whole.
  const [inspector, inspect] = useHoverInspect();
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
        kind: "chooseObject",
        purpose: "put-on-bottom",
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
      {/* Fanned on a gentle arc, as the hand is held: each card's place from the middle. */}
      <div className="opening-hand">
        {starting !== null &&
          hand.map((o, i) => (
            <div
              key={o.slot}
              className="opening-fan"
              style={{ "--fan": i - (hand.length - 1) / 2 } as CSSProperties}
            >
              {facedown ? (
                <CardBack width={176} />
              ) : (
                <OpeningCard
                  object={o}
                  hover={inspect(o)}
                  onPick={
                    picking && bottom.length < toBottom
                      ? () => toggle(o)
                      : undefined
                  }
                />
              )}
            </div>
          ))}
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
      {(picking || buttons.length > 0) && (
        <Dock>
          <DockButtons className="opening-actions">
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
          </DockButtons>
        </Dock>
      )}
      <div className="opening-buttons">
        {considering ? (
          <span role="status" aria-label="Opponent is considering their hand" />
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

/** The result over the table once the game has ended, its final events have played and the
 * table has held a beat more (`HOLD.ending`), so the play that ended it is seen: shown
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
      live ? eventsHold(view) + HOLD.ending : 0,
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

type ShownLossReason = Exclude<LossReason, "life">;

/** Why the game ended, in a word beside its icon. */
const LOSSES: Record<ShownLossReason, string> = {
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

/** The answer to `decision` when it asks for a target that only one choice answers (one
 * object of each set of interchangeable ones, architecture §7.2), the opponent's or something
 * they control, which there is nothing to ask about: as Counterspell's target with only their
 * spell on the stack (it can't target itself, CR 115.5). "Up to one target" with one such
 * choice takes it too, as Subtlety's with only their creature spell on the stack: the count
 * of one, whose target the engine then takes as the only one (CR 115.6: "A spell or ability
 * that requires targets may allow zero targets to be chosen"). A lone target of the player's
 * own is still asked, as Oust's when their own creature is the only one: aiming at it
 * unasked would hurt them. Returning a spell to hand can save either player's spell. Full control asks anyway. */
function loneTarget(
  decision: DecisionView,
  view: PlayerView,
): CandidateIndex | undefined {
  if (view.stops.fullControl) return undefined;
  const opposing = (target: TargetRef) =>
    target.kind === "player"
      ? target.player === "opponent"
      : view.objects.find((o) => o.slot === target.slot)?.controller ===
        "opponent";
  const candidates = decision.candidates;
  if (decision.kind === "target") {
    if (candidates.length !== 1) return undefined;
    const option = candidates[0]!.option;
    if (option.kind === "player") return opposing(option) ? 0 : undefined;
    if (option.kind !== "object") return undefined;
    const target = view.objects.find((o) => o.slot === option.slot);
    const source = view.objects.find((o) => o.slot === decision.source);
    const returningSpell =
      source?.zone === "stack" &&
      source.name &&
      /^Return target spell to its owner's hand\./m.test(
        faceEntry(source.name)?.oracle_text ?? "",
      );
    return target?.controller === "opponent" ||
      (target?.zone === "stack" && returningSpell)
      ? 0
      : undefined;
  }
  if (decision.kind !== "targetCount" || candidates.length !== 2)
    return undefined;
  const one = candidates.findIndex(
    (c) => c.option.kind === "number" && c.option.n === 1,
  );
  const targets = candidates[one]?.targets ?? [];
  return targets.length === 1 && opposing(targets[0]!) ? one : undefined;
}
