// The game as the player watches it on the stage. Each view the session sends goes
// to the page's stage with its script (feed.ts), and the screen takes it when the stage says
// its script has played out.
import { useEffect, useState, useSyncExternalStore } from "react";
import { play } from "../audio/sound";
import { note } from "../diagnostics";
import { soundSteps } from "../audio/steps.ts";
import {
  click,
  stageAnimating,
  startGame,
  type GameUpdate,
  type SessionId,
} from "../ipc";
import type { LogEntry } from "../game/log";
import { useGameUpdates, useShownToFrame } from "../game/updates";
import { animationFrames } from "./clock.ts";
import { desktop } from "./desktop.ts";
import { skipped, skipping } from "./fault.ts";
import { Feed } from "./feed.ts";
import { timeFrames } from "./frames.ts";
import { Prompts } from "./prompt.ts";
import { ScriptStage } from "./stage.ts";
import { played } from "./timing.ts";

/** The stage, for a test to step and read (`window.berylStage`, under a driven browser only):
 * `clock` (`now`, `advance`, `hold`, `release`), `idle`, `settle`, `step`, `pose` and
 * `shown`; what draws its prompts (`window.berylPrompts`), for a capture of the stage's own
 * motion to shut; and how a step is played (`window.berylPlayed`), whose case a capture
 * names its steps by, as the data does. */
type Hooked = Window & {
  berylStage?: ScriptStage;
  berylPrompts?: Prompts;
  berylPlayed?: typeof played;
  berylBehind?: boolean;
};

export type BoardStage = ReturnType<typeof createBoardStage>;

let page: BoardStage | undefined;

/** The page's one stage, what feeds it and what draws the session's prompts on it, made when
 * first asked for: the model lasts as long as the page, whatever of the screen is mounted, so
 * a table shown again finds every object where its scripts left it. Its steps sound as they
 * play. The mounted board attaches its pointer client and sets the layout, which defaults
 * to desktop. */
export function pageStage() {
  return (page ??= createBoardStage(true));
}

/** The manual table has its own stage so opening it leaves the engine game's playback intact. */
export function createBoardStage(engine: boolean) {
  const stage = new ScriptStage({
    layout: desktop,
    timings: (step) => played(step).timing,
    effects: (step) => played(step).effects,
    frames: animationFrames,
  });
  soundSteps(stage);
  if (animationFrames && !navigator.webdriver) {
    timeFrames(stage, animationFrames, (run) => {
      const message = `${run.dropped} of ${run.frames} frames dropped`;
      const lagged = run.dropped >= 3 && run.dropped * 10 >= run.frames;
      note(lagged ? "warn" : "info", "frames", message, run);
    });
  }
  if (engine) {
    // While the stage animates, the opponent's search leaves it the cores it needs.
    let animating = false;
    stage.follow(() => {
      if (animating) return;
      animating = true;
      stageAnimating(true);
      void stage.idle().then(() => {
        animating = false;
        stageAnimating(false);
      });
    });
  }
  const prompts = new Prompts(stage, click);
  prompts.heard = () => play("select");
  // A check of the audits plants a fault here (fault.ts).
  const verb = skipped();
  const fed = engine
    ? verb === null
      ? prompts
      : skipping(prompts, verb)
    : stage;
  const board = { stage, feed: new Feed(fed), prompts };
  if (engine) exposeBoardStage(board);
  return board;
}

export function exposeBoardStage({ stage, prompts }: BoardStage) {
  if (!navigator.webdriver) return;
  (window as Hooked).berylStage = stage;
  (window as Hooked).berylPrompts = prompts;
  (window as Hooked).berylPlayed = played;
}

/** The game as the screen around the table draws it. */
export interface Playback {
  /** The latest view whose script has played out. */
  update: GameUpdate;
  /** More views wait for their scripts to play. */
  behind: boolean;
  /** Plays out at once everything the stage was given, as a click skips Arena's animations. */
  skip(): void;
  /** What has happened in the game up to it, oldest first. */
  log: LogEntry[];
}

/** The game in progress as the player watches it on the stage, and how to start one. */
export function usePlayback(): {
  playback?: Playback;
  /** A game is starting: its first view has not arrived. */
  starting: boolean;
  /** The game in progress, if any, is still being asked for, as the page loads. */
  resuming: boolean;
  /** The game resumed from its save as the page loaded, if one was. */
  resumed?: SessionId;
  /** Starts a game against `agent`, replacing any in progress; a `fair` one is dealt the
   * deal the AI judges fairest of many. */
  start(agent: string, seed?: number, fair?: boolean): Promise<void>;
} {
  const { stage, feed } = pageStage();
  const [starting, setStarting] = useState(false);
  const [resuming, setResuming] = useState(true);
  const resumed = useGameUpdates({
    arrive(update, past) {
      if (update.view.table) return;
      if (feed.arrive(update, past)) setStarting(false);
      setResuming(false);
    },
    resume(update, awaiting) {
      // A view that arrived meanwhile is as new as the game's latest, or newer; a game
      // resumed from its save shows as its first view arrives.
      if (update && !feed.fed().shown) feed.arrive(update, true);
      if (!awaiting || feed.fed().shown) setResuming(false);
    },
  });
  const { shown, behind, log } = useSyncExternalStore(feed.subscribe, feed.fed);
  // Whether the stage is still playing, a prompt's marks coming up too, for a capture that
  // waits for the table to settle (`by specs --shots`).
  const { busy } = useSyncExternalStore(stage.subscribe, () => stage.shown);
  useEffect(() => {
    (window as Hooked).berylBehind = behind || busy;
  }, [behind, busy]);
  useShownToFrame(shown !== undefined);

  const start = async (agent: string, seed?: number, fair?: boolean) => {
    setStarting(true);
    try {
      await startGame(agent, seed, fair);
    } catch (e) {
      setStarting(false);
      throw e;
    }
  };
  return {
    playback: shown && {
      update: shown,
      behind,
      skip: () => feed.skip(),
      log,
    },
    starting,
    resuming,
    resumed,
    start,
  };
}
