// First, so its clocks are the ones Motion takes (freeze.ts).
import "./freeze";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import "./design/fonts";
import "./design/tokens.css";
import "./design/components";
import "./screens/screens.css";
import { installDiagnostics } from "./diagnostics";
import { installAudio } from "./audio/sound";

installDiagnostics();
void installAudio();

createRoot(document.getElementById("root") as HTMLElement).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
