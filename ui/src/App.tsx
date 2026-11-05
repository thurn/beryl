import { lazy, Suspense, useEffect, useSyncExternalStore } from "react";
import { MotionConfig } from "motion/react";
import { appInfo, uiReady } from "./ipc";
import { Gallery } from "./screens/Gallery";
import { GameScreen, StartScreen, useGameUpdates } from "./screens/Game";
import { TableScreen } from "./screens/Table";

// Routes live in the hash so they work from the Tauri asset server and from Vite alike.
const subscribe = (onChange: () => void) => {
  window.addEventListener("hashchange", onChange);
  return () => window.removeEventListener("hashchange", onChange);
};
const route = () => window.location.hash.replace(/^#/, "") || "/";

// `?replica=<mockup>` (src/replica/) exists in development only: production builds drop the
// branch, and with it the replica chunk.
const Replica = import.meta.env.DEV
  ? lazy(() => import("./replica/Replica"))
  : undefined;
const replica =
  Replica && new URLSearchParams(window.location.search).get("replica");

export function App() {
  const path = useSyncExternalStore(subscribe, route);
  // Listening from the start, so a game's first view is never missed.
  const game = useGameUpdates();

  // The app hears the UI has loaded once it has answered a first command.
  useEffect(() => {
    void appInfo().then(() => uiReady());
  }, []);

  // Motion honors the reduced-motion setting, which `just ui-shots` turns on for still captures.
  return (
    <MotionConfig reducedMotion="user">
      {Replica && replica ? (
        <Suspense>
          <Replica name={replica} />
        </Suspense>
      ) : path === "/gallery" ? (
        <Gallery />
      ) : path === "/table" ? (
        <TableScreen />
      ) : path === "/table/scry" ? (
        <TableScreen overlay="scry" />
      ) : path === "/game" ? (
        <GameScreen update={game} />
      ) : (
        <StartScreen />
      )}
    </MotionConfig>
  );
}
