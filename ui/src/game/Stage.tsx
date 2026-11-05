// The fixed 1600×900 stage every game screen is drawn on, scaled to fit the window, and the
// brand at its top left.
import { createContext, useEffect, useState, type ReactNode } from "react";
import { Crest } from "../components/Card";
import { STAGE } from "./layout";

/** How many window pixels a stage pixel takes, for turning pointer motion into stage pixels. */
export const StageScale = createContext(1);

/** Scales a fixed-size stage to fit the window, letterboxed on the table. */
export function Stage({ children }: { children: ReactNode }) {
  const fit = () =>
    Math.min(
      window.innerWidth / STAGE.width,
      window.innerHeight / STAGE.height,
    );
  const [scale, setScale] = useState(fit);
  useEffect(() => {
    const onResize = () => setScale(fit());
    window.addEventListener("resize", onResize);
    return () => window.removeEventListener("resize", onResize);
  }, []);
  return (
    <div className="stage-frame">
      <div className="stage" style={{ ...STAGE, transform: `scale(${scale})` }}>
        <StageScale.Provider value={scale}>{children}</StageScale.Provider>
      </div>
    </div>
  );
}

/** The crest and wordmark at the top left. */
export function Brand() {
  return (
    <div className="brand">
      <Crest className="brand-crest" />
      <span>Project Beryl</span>
    </div>
  );
}
