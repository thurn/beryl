import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import "./design/fonts";
import "./design/tokens.css";
import "./design/components.css";
import "./screens/screens.css";

createRoot(document.getElementById("root") as HTMLElement).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
