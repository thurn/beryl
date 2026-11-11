// The game's table drawn on the stage (plan K25), in place of game/Board.tsx behind `?stage`
// (switch.tsx) until it takes over: the cloth, both life totals, and one node an object where
// the layout rests it. It draws any view, and plays a script when one comes with it. The
// board's controls and the pointer are not here yet.
import { useLayoutEffect, useRef, useState, useSyncExternalStore } from "react";
import { useReducedMotion } from "motion/react";
import type { PlayerView, Script } from "../generated/protocol";
import { LifeOrb } from "../components/Player";
import { useStageWidth } from "../game/Stage";
import { animationFrames } from "./clock.ts";
import { desktop } from "./desktop.ts";
import { DomSurface } from "./dom.ts";
import { Faces } from "./Faces.tsx";
import { ScriptStage } from "./stage.ts";
import { snap, timingOf } from "./timing.ts";
import "./stage.css";

/** The stage, for a test to step and read (`window.berylStage`, under a driven browser only,
 * as playback.ts's `berylShown`): `clock` (`now`, `advance`, `hold`, `release`), `idle`,
 * `settle`, `step` and `pose`. */
type Hooked = Window & { berylStage?: ScriptStage };

export function StageBoard({
  view,
  script,
}: {
  view: PlayerView;
  /** What the screen does between the view before and this one; none starts over from it. */
  script?: Script;
}) {
  const layer = useRef<HTMLDivElement>(null);
  const [mounted, setMounted] = useState<{
    stage: ScriptStage;
    surface: DomSurface;
  }>();
  const stage = mounted?.stage;
  const reduce = useReducedMotion() ?? false;
  const width = useStageWidth();
  useLayoutEffect(() => {
    const surface = new DomSurface(layer.current!);
    const stage = new ScriptStage({
      layout: desktop,
      surface,
      timings: timingOf,
      frames: animationFrames,
    });
    setMounted({ stage, surface });
    const hooked: Hooked = window;
    if (navigator.webdriver) hooked.berylStage = stage;
    return () => {
      stage.close();
      if (hooked.berylStage === stage) delete hooked.berylStage;
    };
  }, []);
  // Reduced motion is durations of zero: every step is over in the tick that begins it.
  useLayoutEffect(() => {
    if (stage) stage.timings = reduce ? snap : timingOf;
  }, [stage, reduce]);
  useLayoutEffect(() => stage?.play(view, script), [stage, view, script]);
  // The layout reads the stage's width as it places (game/layout.ts STAGE).
  useLayoutEffect(() => stage?.relayout(), [stage, width]);
  return (
    <div className="table">
      <div className="far-shade" />
      {stage && <Life stage={stage} />}
      <div className="card-layer" ref={layer} />
      {mounted && <Faces {...mounted} />}
    </div>
  );
}

/** Both life totals, from the stage's table: what a step counts shows as the step plays. */
function Life({ stage }: { stage: ScriptStage }) {
  const { table } = useSyncExternalStore(stage.subscribe, () => stage.shown);
  // The orbs hold to the stage's middle.
  useStageWidth();
  if (!table) return null;
  const anchors = desktop.anchors(table);
  return (["opponent", "you"] as const).map((side) => {
    const { corner } = anchors.life(side);
    return (
      <div
        key={side}
        className="hud life"
        data-life={side}
        style={{ left: corner.x, top: corner.y }}
      >
        <LifeOrb
          life={table.players[side].life}
          active={table.turn.active === side}
          seat={side}
        />
      </div>
    );
  });
}
