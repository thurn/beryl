import { useEffect, useState, useSyncExternalStore } from "react";
import { MotionConfig } from "motion/react";
import { appInfo, canReportBugs, uiReady, type OutcomeView } from "./ipc";
import { BugReporter } from "./components/BugReport";
import { Gallery } from "./screens/Gallery";
import { Specs } from "./screens/Specs";
import { GameScreen, StartScreen } from "./screens/Game";
import { usePlayback } from "./game/playback";
import { useMusic } from "./audio/SoundControls";

// Routes live in the hash so they work from the Tauri asset server and from Vite alike.
const subscribe = (onChange: () => void) => {
  window.addEventListener("hashchange", onChange);
  return () => window.removeEventListener("hashchange", onChange);
};
const route = () => window.location.hash.replace(/^#/, "") || "/";

export function App() {
  // The spec pages live at real paths, served by Vite (`just specs`).
  const page = window.location.pathname;
  return page === "/specs" || page.startsWith("/specs/") ? (
    <Specs path={page} />
  ) : (
    <Main />
  );
}

function Main() {
  const path = useSyncExternalStore(subscribe, route);
  // Listening from the start, so a game's first view is never missed.
  const { playback, starting, resuming, resumed, start } = usePlayback();
  const status = playback?.update.view.status;
  const inProgress =
    playback && status?.kind !== "over" && status?.kind !== "stopped"
      ? playback.update.session
      : undefined;

  // The game resumed at launch replaces without asking until the player goes back to it.
  const [returned, setReturned] = useState(false);
  useEffect(() => {
    if (path === "/game") setReturned(true);
  }, [path]);

  // The menu's music, the game's, then a sting for how it ended; the gallery is quiet.
  useMusic(
    path === "/gallery"
      ? undefined
      : path !== "/game"
        ? "menu"
        : status?.kind !== "over"
          ? "game"
          : won(status.outcome)
            ? "win"
            : "loss",
  );

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
        <GameScreen
          playback={playback}
          starting={starting}
          resuming={resuming}
        />
      ) : (
        <StartScreen
          inProgress={inProgress}
          resumed={
            inProgress !== undefined && inProgress === resumed && !returned
          }
          onStart={start}
        />
      )}
      {canReportBugs() && <BugReporter route={path} />}
    </MotionConfig>
  );
}

const won = (outcome: OutcomeView) =>
  outcome.kind === "win" && outcome.winner === "you";
