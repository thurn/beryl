// The fixed 1600×900 stage every game screen is drawn on, scaled to fit the window, and the
// brand at its top left.
import {
  createContext,
  useEffect,
  useState,
  type CSSProperties,
  type ReactNode,
} from "react";
import { Crest } from "../components/Card";
import { STAGE } from "./layout";

/** How many window pixels a stage pixel takes, for turning pointer motion into stage pixels. */
export const StageScale = createContext(1);

/** How a window `width`×`height` shows the stage: scaled whole into its middle, and how far the
 * window reaches past the stage's sides (`x`) and its top and bottom (`y`), in stage pixels. */
export function fitStage(width: number, height: number) {
  const scale = Math.min(width / STAGE.width, height / STAGE.height);
  return {
    scale,
    x: (width / scale - STAGE.width) / 2,
    y: (height / scale - STAGE.height) / 2,
  };
}

/** Scales a fixed-size stage to fit the window, centered. Where the window's shape differs
 * from the stage's, the table runs on to the window's edges rather than leave bands there:
 * the table's cloth, and the cards beyond the stage's edge (the hands, held at its top and
 * bottom), reach as far as the window does (screens.css `--bleed`). */
export function Stage({ children }: { children: ReactNode }) {
  const fit = () => fitStage(window.innerWidth, window.innerHeight);
  const [{ scale, x, y }, setFit] = useState(fit);
  useEffect(() => {
    const onResize = () => setFit(fit());
    window.addEventListener("resize", onResize);
    return () => window.removeEventListener("resize", onResize);
  }, []);
  const style = {
    ...STAGE,
    transform: `scale(${scale})`,
    "--bleed-x": `${x}px`,
    "--bleed-y": `${y}px`,
  } as CSSProperties;
  return (
    <div className="stage-frame">
      <div className="stage" style={style}>
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
