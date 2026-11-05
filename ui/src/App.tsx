import { useEffect, useSyncExternalStore } from "react";
import { MotionConfig } from "motion/react";
import { appInfo, uiReady } from "./ipc";
import { Gallery } from "./screens/Gallery";
import { GameScreen, StartScreen } from "./screens/Game";
import { usePlayback } from "./game/playback";

// Routes live in the hash so they work from the Tauri asset server and from Vite alike.
const subscribe = (onChange: () => void) => {
  window.addEventListener("hashchange", onChange);
  return () => window.removeEventListener("hashchange", onChange);
};
const route = () => window.location.hash.replace(/^#/, "") || "/";

export function App() {
  const path = useSyncExternalStore(subscribe, route);
  // Listening from the start, so a game's first view is never missed.
  const { playback, starting, start } = usePlayback();
  const status = playback?.update.view.status.kind;
  const inProgress = playback && status !== "over" && status !== "stopped";

  // The app hears the UI has loaded once it has answered a first command.
  useEffect(() => {
    void appInfo().then(() => uiReady());
  }, []);

  // Motion honors the reduced-motion setting, which `just ui-shots` turns on for still captures.
  return (
    <MotionConfig reducedMotion="user">
      {path === "/gallery" ? (
        <Gallery />
      ) : path === "/game" ? (
        <GameScreen playback={playback} starting={starting} />
      ) : (
        <StartScreen inProgress={!!inProgress} onStart={start} />
      )}
    </MotionConfig>
  );
}
