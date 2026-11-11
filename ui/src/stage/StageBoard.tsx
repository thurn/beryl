// The game's table drawn on the stage (plan K25), in place of game/Board.tsx behind `?stage`
// (switch.tsx) until it takes over: the cloth, both life totals, and one node an object or
// ability where the page's stage has it (playback.ts). The stage plays whatever the session
// sent; this only gives it somewhere to draw. The board's controls and the pointer are not
// here yet, but the click that skips what is playing.
import { useLayoutEffect, useRef, useState, useSyncExternalStore } from "react";
import { useReducedMotion } from "motion/react";
import { LifeOrb } from "../components/Player";
import { useStageWidth } from "../game/Stage";
import { desktop } from "./desktop.ts";
import { DomSurface } from "./dom.ts";
import { Faces } from "./Faces.tsx";
import { pageStage } from "./playback.ts";
import type { ScriptStage } from "./stage.ts";
import { snap, timingOf } from "./timing.ts";
import "./stage.css";

export function StageBoard({
  onSkip,
}: {
  /** Plays out at once what the stage is playing, while it is. */
  onSkip?: () => void;
}) {
  const { stage } = pageStage();
  const layer = useRef<HTMLDivElement>(null);
  const [surface, setSurface] = useState<DomSurface>();
  const reduce = useReducedMotion() ?? false;
  const width = useStageWidth();
  useLayoutEffect(() => {
    const surface = new DomSurface(layer.current!);
    stage.mount(surface);
    setSurface(surface);
    return () => stage.unmount(surface);
  }, [stage]);
  // Reduced motion is durations of zero: every step is over in the tick that begins it.
  useLayoutEffect(() => {
    stage.timings = reduce ? snap : timingOf;
  }, [stage, reduce]);
  // The layout reads the stage's width as it places (game/layout.ts STAGE).
  useLayoutEffect(() => stage.relayout(), [stage, width]);
  return (
    <div className="table" onPointerDown={onSkip}>
      <div className="far-shade" />
      <Life stage={stage} />
      <div className="card-layer" ref={layer} />
      {surface && <Faces stage={stage} surface={surface} />}
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
