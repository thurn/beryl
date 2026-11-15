// A board's hold on the page's stage: the element the stage draws its nodes in, for as long as
// the board is mounted.
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { useReducedMotion } from "motion/react";
import { DomSurface } from "./dom.ts";
import type { PointerClient } from "./pointer.ts";
import { SNAP, type ScriptStage } from "./stage.ts";
import { played, RELEASE, snap } from "./timing.ts";

/** Mounts the stage on `layer` and attaches the board's pointer client while it is shown. */
export function useSurface(
  stage: ScriptStage,
  client: PointerClient,
  setClientSurface?: (surface: DomSurface | undefined) => void,
) {
  const layer = useRef<HTMLDivElement>(null);
  const [surface, setSurface] = useState<DomSurface>();
  const reduce = useReducedMotion() ?? false;
  useLayoutEffect(() => stage.pointer.listen(client), [stage, client]);
  useLayoutEffect(() => {
    const surface = new DomSurface(layer.current!);
    stage.mount(surface);
    const unpoint = surface.point(stage.pointer);
    setSurface(surface);
    setClientSurface?.(surface);
    return () => {
      setClientSurface?.(undefined);
      unpoint();
      stage.unmount(surface);
    };
  }, [stage, setClientSurface]);
  // Reduced motion is durations of zero: every step is over in the tick that begins it, a
  // card let go is home at once, and no step begins an effect.
  useLayoutEffect(() => {
    stage.timings = reduce ? snap : (step) => played(step).timing;
    stage.effects = reduce ? () => [] : (step) => played(step).effects;
    stage.release = reduce ? SNAP : RELEASE;
  }, [stage, reduce]);
  // What the stage shows changed with no motion of the pointer: it may be on another node.
  useEffect(() => stage.subscribe(() => surface?.repoint()), [stage, surface]);
  return { layer, surface };
}
