// The stage's effects (UI Conventions, Effects): light and changes of surface on one node or
// on a player's life total, a glow as a token appears, a flash as a card lands, a card
// breaking up as it leaves, a veil over one that turns over, a burst where a blow lands, a
// life total flaring as it changes. A step of a verb begins the effects the design language
// gives it (data/ui-conventions.json, timing.ts `effectsOf`) as it starts, as it ends or part
// of the way through, and each then plays on the stage's clock for its own time, whatever the
// script goes on to do: an effect moves nothing, holds up no step and outlives the one that
// began it. Like a pose, how far through an effect is at a stage time is a function of that
// time alone, and the surface only draws what the model says, once a tick. An effect goes
// with its node: when the stage no longer holds what it is on, it is over.
import type { Side } from "../generated/protocol";
import type { Name } from "./layout.ts";

/** What an effect is on: a node, by its name, or a player, whose life total it is drawn
 * about. */
export type On = Name | Side;

const player = (on: On): on is Side => on === "you" || on === "opponent";

/** Where an effect is drawn against the card it is on: under it, over what lies beneath, or
 * over its face. */
export type Layer = "under" | "over";

/** An effect a step begins: which, where it is drawn, at which end of the step, and how it is
 * played, in stage milliseconds. With no duration it lasts as long as its step. */
export interface Effect {
  name: string;
  layer: Layer;
  when: "start" | "end";
  /** What it is on, where that is not what its step is about: what a strike lands on. */
  on?: On;
  /** How far into its step after that end it begins, as a share of the step: a blow lands
   * part of the way through its strike. */
  into?: number;
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

/** Where effects are drawn: on the node of what the stage names `on`, or about that player's
 * life total. With no progress the effect is over and nothing of it stays. */
export interface EffectSurface {
  effect(on: On, effect: Omit<Drawn, "progress">, progress?: number): void;
}

interface Running {
  on: On;
  effect: Effect;
  from: number;
  to: number;
}

/** The effects under way on a stage. */
export class Effects {
  #running: Running[] = [];

  /** Begins those of `effects` that a step begins at this `edge` of it, which is at stage
   * time `at`, the step showing its card for `span`: each on what it says it is on, or else on
   * `about`, what the step is about, and only on a node the stage holds (`held`). One that
   * would take no time is not begun; one of its name already there gives way to it. */
  begin(
    about: On | undefined,
    effects: readonly Effect[],
    edge: Effect["when"],
    at: number,
    span: number,
    held: (name: Name) => boolean,
  ) {
    for (const effect of effects) {
      const on = effect.on ?? about;
      if (on === undefined || !(player(on) || held(on))) continue;
      const from = at + effect.delay + (effect.into ?? 0) * span;
      const to = from + (effect.duration ?? span);
      if (effect.when !== edge || to <= from) continue;
      this.#running = this.#running.filter(
        (other) => other.on !== on || other.effect.name !== effect.name,
      );
      this.#running.push({ on, effect, from, to });
    }
  }

  /** The effects on `on` at stage time `now`, in the order they began. */
  on(on: On, now: number): Drawn[] {
    return this.#running
      .filter((run) => run.on === on && now >= run.from && now < run.to)
      .map(({ effect: { name, layer, ease }, from, to }) => ({
        name,
        layer,
        progress: ease((now - from) / (to - from)),
      }));
  }

  /** The effects under way at stage time `now` that are named in `names`, each with what it
   * is on, how long ago it began and how long it lasts, in stage milliseconds. */
  under(now: number, names: ReadonlySet<string>) {
    return this.#running
      .filter(
        (run) => names.has(run.effect.name) && now >= run.from && now < run.to,
      )
      .map(({ on, effect, from, to }) => ({
        on,
        name: effect.name,
        age: now - from,
        span: to - from,
      }));
  }

  /** The stage no longer holds `on`: its effects went with its node. */
  gone(on: On) {
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
