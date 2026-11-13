// A game against an agent: the start screen, then the table on the stage (stage/StageBoard.tsx,
// or a phone's, stage/PhoneStageBoard.tsx) with the opening and game-over screens over it.
// Views arrive as `game-update` events and the stage plays their scripts (stage/playback.ts).
// The stage draws and answers the open prompt (stage/prompt.ts); this answers the opening's
// decisions through `choose`. A reloaded page takes up the game in progress.
import { PhoneStageBoard } from "../stage/PhoneStageBoard";
import { pageStage, type Playback } from "../stage/playback";
import { usePrompt } from "../stage/PromptBar";
import { StageBoard } from "../stage/StageBoard";
import { Button, Dock } from "../components/Controls";
import {
  type CandidateIndex,
  CommandError,
  type PlayerView,
  type StopsSetting,
  cancel,
  choose,
  concede,
  setStops,
  undo,
} from "../ipc";
import { type Intent, follow, isOpening } from "../game/decisions";
import { SpecOpponentMove, SpecReplayPlay } from "./Testing";
import { Stage } from "../game/Stage";
import { play } from "../audio/sound";
import { sameStops, saveStops, savedStops } from "../game/stops";
import { useEffect, useRef, useState } from "react";
import { usePhone } from "../mobile/phone";
import { GameOver, resultTitle, useResult } from "./GameOver";
import { Opening } from "./Opening";

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
  const phone = usePhone();
  if (starting || !playback) {
    const screen = (
      <div
        className={
          phone ? "table start-screen phone-start-status" : "table start-screen"
        }
      >
        {(!resuming || playback) && (
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
        )}
      </div>
    );
    return phone ? screen : <Stage>{screen}</Stage>;
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

/** A game in progress: the table, and over it the opening or the game's end. */
function Playing({ playback }: { playback: Playback }) {
  const [error, setError] = useState<string>();
  // The view an answer was sent from: until another arrives, the opponent is acting.
  const [answered, setAnswered] = useState<PlayerView>();
  // The opening's picks still to send (a mulligan's cards for the bottom), one per decision
  // as each arrives.
  const planned = useRef<Intent[]>([]);
  const { update, behind, skip, log } = playback;
  const { session, view } = update;
  const status = view.status;
  // What the stage asks: it draws the prompt and answers it by clicks itself.
  const onStage = usePrompt(pageStage().prompts);
  // An answer is on its way, or the game plays on without the player: the opponent deciding,
  // or a step passing, shown or still to show.
  const waiting =
    answered === view ||
    onStage.answered ||
    behind ||
    status.kind === "thinking" ||
    status.kind === "playing";
  const fail = (e: unknown) => {
    setAnswered(undefined);
    planned.current = [];
    const message = failure(e);
    if (message) {
      play("no");
      setError(message);
    }
  };
  const clickFailed = onStage.error;
  useEffect(() => {
    if (clickFailed !== undefined) fail(clickFailed);
  }, [clickFailed]);
  const decision = status.kind === "deciding" ? status.decision : undefined;
  /** Sends an answer of the opening's, the player's or one planned with it. */
  const send = (index: CandidateIndex, then: Intent[] = []) => {
    if (!decision || waiting) return;
    setError(undefined);
    setAnswered(view);
    planned.current = then;
    choose(decision.id, index).catch(fail);
  };
  /** Answers the opening's decision as the player picked: heard, or refused while the game
   * is busy. */
  const answer = (index: CandidateIndex, then: Intent[] = []) => {
    if (!decision || waiting) {
      play("no");
      return;
    }
    play("select");
    send(index, then);
  };
  /** Takes back the cast in progress. */
  const cancelCast = () => {
    if (!decision?.cancellable || waiting) return;
    play("back");
    setError(undefined);
    setAnswered(view);
    cancel(decision.id).catch(fail);
  };
  /** Takes back the player's latest answer. */
  const undoAnswer = () => {
    if (!decision?.undo || waiting) return;
    play("back");
    setError(undefined);
    setAnswered(view);
    planned.current = [];
    undo(decision.id).catch(fail);
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
  // over and the player decides.
  useEffect(() => {
    if (!decision) return;
    const { index, rest } = follow(decision, planned.current);
    planned.current = [];
    if (index !== undefined) send(index, rest);
    // Only a new view answers (`send` changes with every render).
  }, [view]);

  /** Sends the picks the opening planned, the first now and the rest as they are asked. */
  const planOpening = (picks: Intent[]) => {
    if (!decision) return;
    const { index, rest } = follow(decision, picks);
    if (index !== undefined) answer(index, rest);
  };
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
  const result = useResult(ended);
  const over = result.state === "shown";
  const phone = usePhone();
  const leave = () => (window.location.hash = "#/");
  const onConcede = ended
    ? undefined
    : () =>
        void concede(session).catch((e: unknown) => {
          // A second concession, or one the game's end overtook.
          if (!(e instanceof CommandError && e.error.kind === "ended")) {
            fail(e);
          }
        });
  const openingScreen = opening && (
    <Opening
      view={view}
      decision={openDecision}
      onPick={answer}
      onPlan={planOpening}
    />
  );
  const overlays = (
    <>
      {over && (
        <GameOver
          outcome={status.kind === "over" ? status.outcome : undefined}
          onClose={result.putAside}
        />
      )}
      {result.state === "aside" && (
        <Dock className={phone ? "phone-result-return" : undefined}>
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
        <StageBoard
          view={view}
          log={log}
          waiting={waiting}
          onSkip={behind ? skip : undefined}
          cancel={cancelCast}
          undo={undoAnswer}
          stops={stops}
          setStops={chooseStops}
          onLeave={leave}
          onConcede={onConcede}
          dimmed={opening || over}
        />
        {openingScreen}
        {overlays}
      </Stage>
    );
  }
  return (
    <>
      <PhoneStageBoard
        view={view}
        waiting={waiting}
        cancel={cancelCast}
        onLeave={leave}
        log={log}
        undo={undoAnswer}
        stops={stops}
        setStops={chooseStops}
        onConcede={onConcede}
      />
      {/* The opening still uses the desktop layout. */}
      {openingScreen && <Stage>{openingScreen}</Stage>}
      {overlays}
    </>
  );
}
