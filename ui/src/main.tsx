// First, so its clocks are the ones Motion takes (freeze.ts).
import "./freeze";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import "./design/fonts";
import "./design/tokens.css";
import "./design/components.css";
import "./screens/screens.css";
import { installDiagnostics } from "./diagnostics";

installDiagnostics();

createRoot(document.getElementById("root") as HTMLElement).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
