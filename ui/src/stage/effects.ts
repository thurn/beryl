// The stage's effects (UI Conventions, Effects): light and changes of surface on one node, a
// glow as a token appears, a flash as a card lands, a card breaking up as it leaves, a veil
// over one that turns over. A step of a verb begins the effects the design language gives it
// (data/ui-conventions.json, timing.ts `effectsOf`) as it starts or as it ends, and each then
// plays on the stage's clock for its own time, whatever the script goes on to do: an effect
// moves nothing, holds up no step and outlives the one that began it. Like a pose, how far
// through an effect is at a stage time is a function of that time alone, and the surface only
// draws what the model says, once a tick. An effect goes with its node: when the stage no
// longer holds what it is on, it is over.
import type { Name } from "./layout.ts";

/** Where an effect is drawn against the card it is on: under it, over what lies beneath, or
 * over its face. */
export type Layer = "under" | "over";

/** An effect a step begins: which, where it is drawn, at which end of the step, and how it is
 * played, in stage milliseconds. With no duration it lasts as long as its step. */
export interface Effect {
  name: string;
  layer: Layer;
  when: "start" | "end";
  duration?: number;
  delay: number;
  ease(progress: number): number;
}

/** An effect as it is drawn at one stage time: `progress` of the way through, eased. */
export interface Drawn {
  name: string;
  layer: Layer;
  progress: number;
}

/** Where effects are drawn: on the node of what the stage names `on`. With no progress the
 * effect is over and nothing of it stays. */
export interface EffectSurface {
  effect(on: Name, effect: Omit<Drawn, "progress">, progress?: number): void;
}

interface Running {
  on: Name;
  effect: Effect;
  from: number;
  to: number;
}

/** The effects under way on a stage. */
export class Effects {
  #running: Running[] = [];

  /** Begins `effect` on the node `on` for a step that begins or ends at stage time `at` and
   * takes `span`. One that would take no time is not begun; one of its name already on the
   * node gives way to it. */
  begin(on: Name, effect: Effect, at: number, span: number) {
    const from = at + effect.delay;
    const to = from + (effect.duration ?? span);
    if (to <= from) return;
    this.#running = this.#running.filter(
      (other) => other.on !== on || other.effect.name !== effect.name,
    );
    this.#running.push({ on, effect, from, to });
  }

  /** The effects on the node `on` at stage time `now`, in the order they began. */
  on(on: Name, now: number): Drawn[] {
    return this.#running
      .filter((run) => run.on === on && now >= run.from && now < run.to)
      .map(({ effect: { name, layer, ease }, from, to }) => ({
        name,
        layer,
        progress: ease((now - from) / (to - from)),
      }));
  }

  /** The stage no longer holds `on`: its effects went with its node. */
  gone(on: Name) {
    this.#running = this.#running.filter((run) => run.on !== on);
  }

  /** Draws every effect on `surface` as it is at stage time `now`, and ends those over. */
  write(surface: EffectSurface | undefined, now: number) {
    for (const { on, effect, from, to } of this.#running) {
      if (now >= to) surface?.effect(on, effect);
      else if (now >= from) {
        surface?.effect(on, effect, effect.ease((now - from) / (to - from)));
      }
    }
    this.#running = this.#running.filter((run) => now < run.to);
  }

  /** Ends every effect at once: the stage starts over. */
  clear(surface: EffectSurface | undefined) {
    for (const { on, effect } of this.#running) surface?.effect(on, effect);
    this.#running = [];
  }
}
