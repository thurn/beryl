// A board's hold on the page's stage: the element the stage draws its nodes in, for as long as
// the board is mounted.
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { useReducedMotion } from "motion/react";
import { DomSurface } from "./dom.ts";
import { SNAP, type ScriptStage } from "./stage.ts";
import { RELEASE, snap, timingOf } from "./timing.ts";

/** Mounts `stage` on the element `layer` is given, and returns its surface once it is. */
export function useSurface(stage: ScriptStage) {
  const layer = useRef<HTMLDivElement>(null);
  const [surface, setSurface] = useState<DomSurface>();
  const reduce = useReducedMotion() ?? false;
  useLayoutEffect(() => {
    const surface = new DomSurface(layer.current!);
    stage.mount(surface);
    const unpoint = surface.point(stage.pointer);
    setSurface(surface);
    return () => {
      unpoint();
      stage.pointer.cancel();
      stage.unmount(surface);
    };
  }, [stage]);
  // Reduced motion is durations of zero: every step is over in the tick that begins it, and
  // a card let go is home at once.
  useLayoutEffect(() => {
    stage.timings = reduce ? snap : timingOf;
    stage.release = reduce ? SNAP : RELEASE;
  }, [stage, reduce]);
  // What the stage shows changed with no motion of the pointer: it may be on another node.
  useEffect(() => stage.subscribe(() => surface?.repoint()), [stage, surface]);
  return { layer, surface };
}
