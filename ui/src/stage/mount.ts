// A board's hold on the page's stage: the element the stage draws its nodes in, for as long as
// the board is mounted.
import { useLayoutEffect, useRef, useState } from "react";
import { useReducedMotion } from "motion/react";
import { DomSurface } from "./dom.ts";
import type { ScriptStage } from "./stage.ts";
import { snap, timingOf } from "./timing.ts";

/** Mounts `stage` on the element `layer` is given, and returns its surface once it is. */
export function useSurface(stage: ScriptStage) {
  const layer = useRef<HTMLDivElement>(null);
  const [surface, setSurface] = useState<DomSurface>();
  const reduce = useReducedMotion() ?? false;
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
  return { layer, surface };
}
