// First, so its clocks are the ones Motion takes (freeze.ts).
import "./freeze";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import "./design/fonts";
import "./design/tokens.css";
import "./design/components";
import "./screens/styles";
import { appInfo, isNativeApp } from "./ipc";
import { installDiagnostics } from "./diagnostics";
import { installAudio } from "./audio/sound";
import { setProfile } from "./profile";

installDiagnostics();

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
  void appInfo().then((info) => {
    setProfile(info.profile);
    render();
  }, render);
} else {
  render();
}
