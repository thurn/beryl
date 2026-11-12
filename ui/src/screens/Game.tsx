// A game against an agent: the start screen, then the battlefield (game/Board.tsx) with the
// mulligan, choice (game/ChoiceOverlay.tsx) and game-over screens over it. Views arrive as
// `game-update` events and play back at a readable pace (game/playback.ts); the screen answers
// the open decision through `choose`. A reloaded page takes up the game in progress.
import { Board, PhoneBoard, useAsked } from "../stage/switch";
import { Button, Dock } from "../components/Controls";
import {
  type CandidateIndex,
  CommandError,
  type DecisionView,
  type PlayerView,
  type StopsSetting,
  type TargetRef,
  cancel,
  choose,
  concede,
  passOn,
  setStops,
  undo,
} from "../ipc";
import { type Choice, choiceOf, choiceSlots, floats } from "../game/choices";
import { ChoiceOverlay } from "../game/ChoiceOverlay";
import { FloatPrompt, PromptRow } from "../game/FloatPrompt";
import {
  type Intent,
  follow,
  isOpening,
  warnedTarget,
} from "../game/decisions";
import { type Playback, eventsHold } from "../game/playback";
import { RulesText } from "../components/Mana";
import { SpecOpponentMove, SpecReplayPlay } from "./Testing";
import { Stage } from "../game/Stage";
import { faceEntry } from "../cards";
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
  // What the stage asks, where it draws the prompt and answers it by clicks itself.
  const onStage = useAsked();
  const confirmingRemoval = removal?.view === view ? removal : undefined;
  // An answer is on its way, or the game plays on without the player: the opponent deciding,
  // or a step passing, shown or still to show.
  const waiting =
    answered === view ||
    onStage?.answered === true ||
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
  const clickFailed = onStage?.error;
  useEffect(() => {
    if (clickFailed !== undefined) fail(clickFailed);
  }, [clickFailed]);
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
    // Keep intermediate engine questions off the table for a declaration the player
    // confirmed as one action.
    if (
      then.length > 0 &&
      (decision.kind === "attacker" || decision.kind === "blocker")
    )
      gather("combat");
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
  const onConcede = ended
    ? undefined
    : () =>
        void concede(session).catch((e: unknown) => {
          // A second concession, or one the game's end overtook.
          if (!(e instanceof CommandError && e.error.kind === "ended")) {
            fail(e);
          }
        });
  const takeBack = confirmingRemoval ? undefined : undoAnswer;
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
      undo={takeBack}
      passOn={passOnward}
      stops={stops}
      setStops={chooseStops}
      onLeave={leave}
      onConcede={onConcede}
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
        log={log}
        undo={takeBack}
        stops={stops}
        setStops={chooseStops}
        onConcede={onConcede}
      />
      {overlaid && <Stage>{overlays}</Stage>}
    </>
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
