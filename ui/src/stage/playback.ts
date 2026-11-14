// The game as the player watches it on the stage. Each view the session sends goes
// to the page's stage with its script (feed.ts), and the screen takes it when the stage says
// its script has played out.
import { useEffect, useState, useSyncExternalStore } from "react";
import { play } from "../audio/sound";
import { note } from "../diagnostics";
import { soundSteps } from "../audio/steps.ts";
import { click, startGame, type GameUpdate, type SessionId } from "../ipc";
import type { LogEntry } from "../game/log";
import { useGameUpdates, useShownToFrame } from "../game/updates";
import { animationFrames } from "./clock.ts";
import { desktop } from "./desktop.ts";
import { skipped, skipping } from "./fault.ts";
import { Feed } from "./feed.ts";
import { timeFrames } from "./frames.ts";
import { Prompts } from "./prompt.ts";
import { ScriptStage } from "./stage.ts";
import { caseOf, timingOf } from "./timing.ts";

/** The stage, for a test to step and read (`window.berylStage`, under a driven browser only,
 * only): `clock` (`now`, `advance`, `hold`, `release`), `idle`,
 * `settle`, `step`, `pose` and `shown`; what draws its prompts (`window.berylPrompts`), for a
 * capture of the stage's own motion to shut; and the case a step is of (`window.berylCase`),
 * for a capture to name its steps as the data does. */
type Hooked = Window & {
  berylStage?: ScriptStage;
  berylPrompts?: Prompts;
  berylCase?: typeof caseOf;
  berylBehind?: boolean;
};

let page: { stage: ScriptStage; feed: Feed; prompts: Prompts } | undefined;

/** The page's one stage, what feeds it and what draws the session's prompts on it, made when
 * first asked for: the model lasts as long as the page, whatever of the screen is mounted, so
 * a table shown again finds every object where its scripts left it. It is laid out for the
 * desktop until a board says otherwise (StageBoard.tsx, PhoneStageBoard.tsx), and its steps
 * sound as they play. */
export function pageStage() {
  if (!page) {
    const stage = new ScriptStage({
      layout: desktop,
      timings: timingOf,
      frames: animationFrames,
    });
    soundSteps(stage);
    // The player's frames, not a driven browser's, whose clock a check may pause.
    if (animationFrames && !navigator.webdriver) {
      timeFrames(stage, animationFrames, (run) => {
        const message = `${run.dropped} of ${run.frames} frames dropped`;
        // A run that dropped several frames, a tenth of them or more, also reaches the app's
        // journal, which outlives the page.
        const lagged = run.dropped >= 3 && run.dropped * 10 >= run.frames;
        note(lagged ? "warn" : "info", "frames", message, run);
      });
    }
    const prompts = new Prompts(stage, click);
    prompts.heard = () => play("select");
    // A check of the audits plants a fault here (fault.ts): none otherwise.
    const verb = skipped();
    const fed = verb === null ? prompts : skipping(prompts, verb);
    page = { stage, feed: new Feed(fed), prompts };
    if (navigator.webdriver) {
      (window as Hooked).berylStage = stage;
      (window as Hooked).berylPrompts = prompts;
      (window as Hooked).berylCase = caseOf;
    }
  }
  return page;
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
  /** Starts a game against `agent`, replacing any in progress. */
  start(agent: string, seed?: number): Promise<void>;
} {
  const { stage, feed } = pageStage();
  const [starting, setStarting] = useState(false);
  const [resuming, setResuming] = useState(true);
  const resumed = useGameUpdates({
    arrive(update, past) {
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

  const start = async (agent: string, seed?: number) => {
    setStarting(true);
    try {
      await startGame(agent, seed);
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
