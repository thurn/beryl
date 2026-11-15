// First, so its clocks are the ones Motion takes (freeze.ts).
import "./freeze";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import "./design/fonts";
import "./design/tokens.css";
import "./design/components";
import "./screens/styles";
import { appInfo, isNativeApp, stageTimings } from "./ipc";
import { installDiagnostics, note } from "./diagnostics";
import { ownTimings } from "./stage/timing";
import { installAudio } from "./audio/sound";
import { setProfile } from "./profile";

installDiagnostics();

// The stage times the steps it makes itself while a prompt is open (its marks, arrows and
// held cards) by a table the session gives once, which it has before the first view plays.
// Without it they are over as they begin. A missing or malformed table still renders the app:
// the catch follows `ownTimings`, so a table it cannot read is caught too.
const timed = stageTimings()
  .then(ownTimings)
  .catch((error: unknown) => {
    note("warn", "stage", "no timing for the stage's own steps", String(error));
  });

function render() {
  void installAudio();
  createRoot(document.getElementById("root") as HTMLElement).render(
    <StrictMode>
      <App />
    </StrictMode>,
  );
}

// A native app needs its profile before reading settings and its platform before drawing art.
if (isNativeApp()) {
  void appInfo().then(
    async (info) => {
      setProfile(info.profile);
      await timed;
      render();
    },
    () => void timed.then(render),
  );
} else {
  void timed.then(render);
}
