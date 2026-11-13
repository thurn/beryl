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

installDiagnostics();
void installAudio();

function render() {
  createRoot(document.getElementById("root") as HTMLElement).render(
    <StrictMode>
      <App />
    </StrictMode>,
  );
}

// Native card art URLs need the platform before the first render of a resumed game.
if (isNativeApp()) {
  void appInfo().then(render, render);
} else {
  render();
}
