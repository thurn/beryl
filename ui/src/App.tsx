import { useEffect, useState, useSyncExternalStore } from "react";
import { MotionConfig } from "motion/react";
import {
  appInfo,
  canExportBugs,
  canReportBugs,
  isMobile,
  isRelayedGame,
  uiReady,
  type OutcomeView,
} from "./ipc";
import { BugReporter, bugReportPreview } from "./components/BugReport";
import { Gallery } from "./screens/Gallery";
import { Testing } from "./screens/Testing";
import { GameScreen } from "./screens/Game";
import { StartScreen } from "./screens/StartScreen";
import { usePlayback } from "./stage/playback";
import { useMusic } from "./audio/SoundControls";
import { setMobile, usePhone } from "./mobile/phone";

// Routes live in the hash so they work from the Tauri asset server and from Vite alike.
const subscribe = (onChange: () => void) => {
  window.addEventListener("hashchange", onChange);
  return () => window.removeEventListener("hashchange", onChange);
};
const route = () => window.location.hash.replace(/^#/, "") || "/";

export function App() {
  // The spec pages live at real paths, served by Vite (`just specs`).
  const page = window.location.pathname;
  return /^\/(playground|scenarios|specs|spec)(\/|$)/.test(page) ? (
    <Testing path={page} />
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
  // A spec page can keep many live games open. Its frames play only sound effects.
  useMusic(
    path === "/gallery" || isRelayedGame()
      ? undefined
      : path !== "/game"
        ? "menu"
        : status?.kind !== "over"
          ? "game"
          : won(status.outcome)
            ? "win"
            : "loss",
  );

  // The app hears the UI has loaded once it has answered a first command, which says whether
  // it runs on a phone.
  const [exportReports, setExportReports] = useState(false);
  useEffect(() => {
    void appInfo().then((info) => {
      setMobile(isMobile(info.platform));
      setExportReports(canExportBugs());
      uiReady();
    });
  }, []);
  const phone = usePhone();
  useEffect(() => {
    document.body.classList.toggle("phone", phone);
  }, [phone]);

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
          exportReports={exportReports}
        />
      )}
      {(canReportBugs() || bugReportPreview()) && <BugReporter route={path} />}
    </MotionConfig>
  );
}

const won = (outcome: OutcomeView) =>
  outcome.kind === "win" && outcome.winner === "you";
