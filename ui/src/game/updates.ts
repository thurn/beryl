// Hearing the game: the views the session sends, from the page's start, and the game already
// in progress as the page loads. What shows them, and at what pace, is its caller's
// (playback.ts; stage/playback.ts behind `?stage`).
import { useEffect, useRef, useState } from "react";
import {
  currentGame,
  onGameUpdate,
  resumeGame,
  type GameUpdate,
  type SessionId,
} from "../ipc";

/** Listens for the game's views for as long as the caller is mounted, and takes up the game
 * in progress once listening, so no view falls between: a reloaded page takes it up where it
 * is, and a launched app the newest unfinished game saved. `arrive` hears each view, `past`
 * when it is from before the page listened; `resume` hears the game in progress, if any, or
 * that a game resumed from its save is on its way (`awaiting`). Returns the game resumed from
 * its save as the page loaded, if one was. */
export function useGameUpdates(handlers: {
  arrive(update: GameUpdate, past: boolean): void;
  resume(update: GameUpdate | undefined, awaiting: boolean): void;
}): SessionId | undefined {
  const [resumed, setResumed] = useState<SessionId>();
  const latest = useRef(handlers);
  latest.current = handlers;
  useEffect(() => {
    // Stopping a listener takes a moment, in which it may still hear views (StrictMode's
    // second mount hears them too): only the effect still mounted acts on them.
    let mounted = true;
    const unlisten = onGameUpdate(
      (update, past) => mounted && latest.current.arrive(update, past),
    );
    const resume = (update?: GameUpdate, awaiting = false) =>
      mounted && latest.current.resume(update, awaiting);
    void unlisten
      .then(() => currentGame())
      .then(
        async (update) => {
          if (update) {
            resume(update);
            return;
          }
          const session = await resumeGame().catch(() => null);
          if (mounted && session !== null) setResumed(session);
          resume(undefined, session !== null);
        },
        () => resume(),
      );
    return () => {
      mounted = false;
      void unlisten.then((stop) => stop());
    };
  }, []);
  return resumed;
}

/** Tells a spec page framing the game that it has a view to show (screens/Testing.tsx), once
 * `showing`. */
export function useShownToFrame(showing: boolean) {
  useEffect(() => {
    if (showing && window.parent !== window) {
      window.parent.postMessage(
        { kind: "beryl-shown" },
        window.location.origin,
      );
    }
  }, [showing]);
}
