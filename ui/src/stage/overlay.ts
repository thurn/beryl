// What the stage draws besides its nodes, held together for it (stage.ts): the effects its
// steps begin (effects.ts), the arrows, lines and marks its scripts have begun and not ended
// (spans.ts), the streaks its steps draw (streaks.ts), and how far through each announcement
// passing over the table is, which the chrome that draws it reads (shown.ts). Each plays on
// the stage's clock and is a function of stage time and of where the model has what it is
// on, which the stage says (`reach`); nothing here is a node, moves one or holds up a step.
// The lines, and the marks of the creatures in combat, are drawn as light where the surface
// can paint it (light.ts): the scene of it is worked out here, each tick.
import type { Arrow } from "../generated/protocol";
import type { Edge, Run } from "./beat.ts";
import {
  Effects,
  type Effect,
  type EffectSurface,
  type On,
} from "./effects.ts";
import type { Name, Resting } from "./layout.ts";
import { lightOf, type Fighter, type Lit, type LitLine } from "./light.ts";
import { shapeOf, wayOf, type Reach } from "./lines.ts";
import type { NoticeSurface } from "./shown.ts";
import {
  namedBy,
  Spans,
  type Line,
  type Marked,
  type SpanSurface,
} from "./spans.ts";
import { Streaks, type Streak, type StreakSurface } from "./streaks.ts";
import type { Act } from "./table.ts";
import { litBy, SNAP } from "./tracks.ts";

/** Where all of it is drawn. One that paints light is given all that is lit each tick, and
 * none once the stage no longer draws on it. */
export interface OverlaySurface
  extends EffectSurface, SpanSurface, StreakSurface, NoticeSurface {
  light?(lit?: Lit): void;
}

/** What the stage holds at rest, by name. */
export type Held = ReadonlyMap<Name, { resting: Resting }>;

/** Since when each creature in combat has fought as it does, and been blocked. */
type Fought = Map<Name, Pick<Fighter, "fights" | "since" | "blocked">>;

/** How what the stage names `on` lies at stage time `at`, as an end of a line or a streak: a
 * node, or a player's life total, or with `pool` their unspent mana; none for what the stage
 * does not hold. */
export type Reaching = (
  on: On,
  at: number,
  pool?: boolean,
) => Reach | undefined;

export class Overlay {
  readonly #effects = new Effects();
  readonly #spans = new Spans();
  readonly #streaks = new Streaks();
  /** The `announce` steps under way, in the order they began: one that passes over the table
   * (`Timing.passes`) is here after its beat and its script are over, until its own end. */
  notices: Run[] = [];
  readonly #reach: Reaching;
  #fought: Fought = new Map();
  /** Nothing has been drawn since the stage started over: what fights then has fought since
   * before it, and its mark does not pop. */
  #fresh = true;

  constructor(reach: Reaching) {
    this.#reach = reach;
  }

  /** Plays the step `act` over the stage times `from` to `to`, if it begins or ends a span
   * or draws a streak: a streak leaves where its source lies as the step begins, and mana a
   * player spends leaves their pool. */
  play(act: Act, from: number, to: number, ease = SNAP.ease) {
    if (act.verb === "point" || act.verb === "mark") {
      this.#spans.play(act, from, to, ease);
    } else if (act.verb === "streak") {
      const origin = act.from && this.#reach(namedBy(act.from), from, true);
      this.#streaks.play(act, origin, from, to, ease);
    }
  }

  /** Begins the effects `begun` that the step `run` has at this `edge` of it, on what they
   * are on if the stage holds that (`held`): one that lasts its step, for as long as the
   * step. */
  affect(
    run: Run,
    edge: Edge["edge"],
    begun: readonly Effect[],
    held: (name: Name) => boolean,
  ) {
    const at = edge === "start" ? run.from : run.to;
    const span = run.to - run.from;
    this.#effects.begin(litBy(run.step.act), begun, edge, at, span, held);
  }

  /** The stage time from which the span the step `act` begins or ends has been whole. */
  whole(act: Act): number | undefined {
    return this.#spans.whole(act);
  }

  /** The arrows and lines held at stage time `now`, each as it is drawn then: from where the
   * model has its two ends, never from the page. The line from an ability to its source is
   * hidden while the stage shows a `prompt` (UI Conventions, "A line from an ability to its
   * source"). */
  lines(now: number, prompt: boolean): Line[] {
    return this.#spans.lines(
      now,
      (arrow: Arrow) => prompt && arrow === "source",
      ({ arrow, from, to, shown }) => {
        const [a, b] = [this.#reach(from, now), this.#reach(to, now)];
        return a && b ? shapeOf(arrow, a, b, shown) : null;
      },
    );
  }

  /** Everything lit at stage time `now` (light.ts): the `lines` held then, and the marks of
   * the creatures among `held` that attack or block. A creature's mark begins as the stage
   * first draws it in combat, which is when the view that says so is shown. */
  light(now: number, lines: readonly Line[], held: Held): Lit {
    const lit = lines.flatMap((line): LitLine[] => {
      const [a, b] = [this.#reach(line.from, now), this.#reach(line.to, now)];
      const way = line.shape && a && b ? wayOf(line.arrow, a, b) : null;
      return way && a && b ? [{ ...line, way, from: a, to: b }] : [];
    });
    const first = this.#fresh ? -Infinity : now;
    const fought: Fought = new Map();
    const fighters: Fighter[] = [];
    for (const [name, { resting }] of held) {
      const combat = resting.object?.combat;
      if (!combat || resting.dress.zone !== "battlefield") continue;
      const before = this.#fought.get(name);
      const fights = combat.kind === "attacking" ? "attacks" : "blocks";
      const since = before?.fights === fights ? before.since : first;
      const blocked =
        combat.kind === "attacking" && combat.blocked
          ? (before?.blocked ?? first)
          : undefined;
      fought.set(name, { fights, since, blocked });
      const reach = this.#reach(name, now);
      if (reach) fighters.push({ reach, fights, since, blocked });
    }
    this.#fought = fought;
    this.#fresh = false;
    return lightOf(now, lit, fighters);
  }

  /** The marks held at stage time `now`, on `on` or on anything. */
  marks(now: number, on?: On): Marked[] {
    return this.#spans.marks(now, on);
  }

  /** How far the permanent `on` is turned by a mark at stage time `now` (spans.ts). */
  turned(on: On, now: number): number {
    return this.#spans.turned(on, now);
  }

  /** The nodes whose pose a mark is changing at stage time `now` (spans.ts). */
  stirred(now: number): ReadonlySet<On> {
    return this.#spans.stirred(now);
  }

  /** The streaks drawn at stage time `now`, each from where its source lay as its step began
   * to where the model has what it lands on. */
  streaks(now: number): Streak[] {
    return this.#streaks.at(now, (on) => this.#reach(on, now));
  }

  /** The effects under way at stage time `now` on `on`, each as far through as it is. */
  effect(on: On, now: number): ReturnType<Effects["on"]> {
    return this.#effects.on(on, now);
  }

  /** The stage no longer holds `name`: what was on it went with its node. */
  gone(name: Name) {
    this.#effects.gone(name);
    this.#spans.gone(name);
  }

  /** The stage time by which every announcement now passing over the table is over; none
   * passing, the beginning of time. */
  passed(): number {
    const passing = this.notices.filter((run) => run.passes);
    return Math.max(-Infinity, ...passing.map((run) => run.to));
  }

  /** The announcement `run` makes is over. */
  over(surface: OverlaySurface | undefined, run: Run) {
    this.notices = this.notices.filter((other) => other !== run);
    if (run.step.act.verb === "announce") {
      surface?.notice(run.step.act.notice.kind);
    }
  }

  /** Draws everything on `surface` as it is at stage time `now`, and takes off it what has
   * ended. */
  write(
    surface: OverlaySurface | undefined,
    now: number,
    prompt: boolean,
    held: Held,
  ) {
    this.#effects.write(surface, now);
    const lines = this.lines(now, prompt);
    this.#spans.write(surface, now, lines);
    surface?.light?.(this.light(now, lines, held));
    this.#streaks.write(surface, now, this.streaks(now));
    for (const { step, from, to } of this.notices) {
      if (step.act.verb !== "announce") continue;
      surface?.notice(step.act.notice.kind, (now - from) / (to - from));
    }
  }

  /** Takes every span and streak off `surface`, which the stage no longer draws on: each is
   * still under way. */
  erase(surface: OverlaySurface | undefined) {
    this.#spans.erase(surface);
    this.#streaks.clear(surface, false);
    surface?.light?.();
  }

  /** Ends everything at once: the stage starts over. */
  clear(surface: OverlaySurface | undefined) {
    this.#effects.clear(surface);
    this.#spans.clear(surface);
    this.#streaks.clear(surface, true);
    this.#fought = new Map();
    this.#fresh = true;
    for (const run of this.notices) this.over(surface, run);
  }
}
