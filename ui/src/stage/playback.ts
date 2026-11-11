// The game as the player watches it on the stage (plan K25), behind `?stage`: game/playback.ts's
// `usePlayback` with the stage in place of its beats and timers. Each view goes to the page's
// stage with its script (feed.ts), and the screen takes it when the stage says its script has
// played out.
import { useEffect, useState, useSyncExternalStore } from "react";
import { soundSteps } from "../audio/steps.ts";
import { startGame } from "../ipc";
import type { usePlayback } from "../game/playback";
import { useGameUpdates, useShownToFrame } from "../game/updates";
import { animationFrames } from "./clock.ts";
import { desktop } from "./desktop.ts";
import { Feed } from "./feed.ts";
import { ScriptStage } from "./stage.ts";
import { timingOf } from "./timing.ts";

/** The stage, for a test to step and read (`window.berylStage`, under a driven browser only,
 * as playback.ts's `berylShown`): `clock` (`now`, `advance`, `hold`, `release`), `idle`,
 * `settle`, `step`, `pose` and `shown`. */
type Hooked = Window & { berylStage?: ScriptStage; berylBehind?: boolean };

let page: { stage: ScriptStage; feed: Feed } | undefined;

/** The page's one stage and what feeds it, made when first asked for: the model lasts as long
 * as the page, whatever of the screen is mounted, so a table shown again finds every object
 * where its scripts left it. It is laid out for the desktop until a board says otherwise
 * (StageBoard.tsx, PhoneStageBoard.tsx), and its steps sound as they play. */
export function pageStage() {
  if (!page) {
    const stage = new ScriptStage({
      layout: desktop,
      timings: timingOf,
      frames: animationFrames,
    });
    soundSteps(stage);
    page = { stage, feed: new Feed(stage) };
    if (navigator.webdriver) (window as Hooked).berylStage = stage;
  }
  return page;
}

/** The game in progress as the player watches it on the stage, and how to start one. */
export function useStagePlayback(): ReturnType<typeof usePlayback> {
  const { feed } = pageStage();
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
  const { shown, behind, latest, log } = useSyncExternalStore(
    feed.subscribe,
    feed.fed,
  );
  // Whether the stage is still playing, for a capture that waits for the table to settle
  // (`by specs --shots`).
  useEffect(() => {
    (window as Hooked).berylBehind = behind;
  }, [behind]);
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
      // The screen takes a view once the stage has played it: nothing in it is still to be
      // announced.
      live: false,
      behind,
      skip: () => feed.skip(),
      gather: () => feed.gather(),
      release: () => feed.release(),
      latest,
      log,
    },
    starting,
    resuming,
    resumed,
    start,
  };
}
