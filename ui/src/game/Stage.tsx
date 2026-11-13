// The stage every game screen is drawn on, 900 high and as wide as the window's shape within
// bounds, scaled to fit the window, and the brand at its top left.
import {
  createContext,
  useContext,
  useEffect,
  useState,
  type CSSProperties,
  type ReactNode,
} from "react";
import { setStageWidth, STAGE, STAGE_WIDTHS } from "./perspective";

/** How many window pixels a stage pixel takes, for turning pointer motion into stage pixels. */
export const StageScale = createContext(1);

/** How wide the stage is (layout.ts STAGE), for what places things on it: reading it re-renders
 * them when the window changes shape. */
const StageWidth = createContext(STAGE_WIDTHS.base);
export const useStageWidth = () => useContext(StageWidth);

/** How a window `width`×`height` shows the stage: as wide as the window's shape asks, within
 * `STAGE_WIDTHS`, so that it fills the window and only a window narrower or wider than that
 * leaves room past the stage's edges; scaled whole into its middle; and how far the window
 * reaches past the stage's sides (`x`) and its top and bottom (`y`), in stage pixels. */
export function fitStage(width: number, height: number) {
  const stage = Math.min(
    STAGE_WIDTHS.most,
    Math.max(STAGE_WIDTHS.least, (STAGE.height * width) / height),
  );
  const scale = Math.min(width / stage, height / STAGE.height);
  return {
    width: stage,
    scale,
    x: (width / scale - stage) / 2,
    y: (height / scale - STAGE.height) / 2,
  };
}

/** Scales the stage to fit the window, centered, as wide as the window's shape asks (fitStage).
 * Where the window's shape is beyond the stage's, the table runs on to the window's edges rather
 * than leave bands there: the table's cloth, and the cards beyond the stage's edge (the hands,
 * held at its top and bottom), reach as far as the window does (screens.css `--bleed`). What is
 * placed from the stage's right edge or its middle in CSS adds `--shift` or `--shift-mid`, how
 * far that edge or middle lies from where it would on the base stage. */
export function Stage({ children }: { children: ReactNode }) {
  const [fit, setFit] = useState(() => fitWindow());
  useEffect(() => {
    const onResize = () => setFit(fitWindow());
    window.addEventListener("resize", onResize);
    return () => window.removeEventListener("resize", onResize);
  }, []);
  // Set before the table under it is placed, which reads it. Every Stage fits the same window,
  // so all agree on it.
  setStageWidth(fit.width);
  const shift = fit.width - STAGE_WIDTHS.base;
  const style = {
    width: fit.width,
    height: STAGE.height,
    transform: `scale(${fit.scale})`,
    // What a shake of the stage is scaled by (stage/stage.css `.stage[data-shaking]`).
    "--stage-scale": fit.scale,
    "--bleed-x": `${fit.x}px`,
    "--bleed-y": `${fit.y}px`,
    "--shift": `${shift}px`,
    "--shift-mid": `${shift / 2}px`,
  } as CSSProperties;
  return (
    <div className="stage-frame">
      <div className="stage" style={style}>
        <StageWidth.Provider value={fit.width}>
          <StageScale.Provider value={fit.scale}>
            {children}
          </StageScale.Provider>
        </StageWidth.Provider>
      </div>
    </div>
  );
}

const fitWindow = () => fitStage(window.innerWidth, window.innerHeight);
