// The stage's streaks (UI Conventions, Verbs, streak): light that leaves one thing and lands
// on another, mana from what made it into the spell or ability it pays for, and damage from
// a spell or ability to what it hits. A `streak` step draws one for as long as it plays and
// no longer: it is no span, and what it lands on shows that it landed with an effect of its
// own (effects.ts), begun as the step ends.
//
// A streak is a comet: a bright head on a low arc from where its source lay as the step began
// to where its target is now, with a tail that stretches out of the source, follows, and is
// drawn in as the head lands. Beryl's cards are whole and lie in rows, so several lands'
// streaks into one spell rise from the row side by side and close on it together (Arena's
// curl out of each land and run straight: `TbrYIs2juCw 33:32.45`). Like a pose, a streak's
// shape at a stage time is a function of that time and of where the model has its ends, so
// it is the same however the time is reached, and the surface only writes it.
import type { Act } from "../generated/protocol";
import type { On } from "./effects.ts";
import type { Point } from "./layout.ts";
import type { Reach } from "./lines.ts";
import { namedBy } from "./spans.ts";
import type { Timing } from "./tracks.ts";

/** A step that draws a streak. */
export type StreakAct = Extract<Act, { verb: "streak" }>;

/** A streak as it is drawn at one stage time: its light in stage pixels, a closed path from
 * its `tail` round its `head`. */
export interface StreakShape {
  d: string;
  head: Point;
  tail: Point;
  /** Half the width of its head, in stage pixels: its halo is drawn as wide again. */
  wide: number;
}

/** A streak the stage holds, as it is at one stage time: no shape while there is nothing to
 * draw of it, its source having no place on the table (a permanent sacrificed in an earlier
 * beat) or its target none. */
export interface Streak {
  /** What names it among the stage's streaks, and its element on the page. */
  key: string;
  /** What it is made of: `damage`, or mana's color. */
  look: string;
  to: On;
  /** How far its head is along its way, from 0 to 1. */
  through: number;
  shape: StreakShape | null;
}

/** Where streaks are drawn: with no look the streak is over and nothing of it stays. */
export interface StreakSurface {
  streak(key: string, look?: string, shape?: StreakShape | null): void;
}

/** How a streak is drawn. Its arc rises `bow` of the way between its ends, and never more
 * than `rise` of its target's height, so that a streak across the whole table stays low;
 * each streak that leaves one place for one target with another rises `fan` as high again
 * as the one before, so that the mana of a pile of lands is seen as so many. Its tail is
 * `tail` of its way long. Its head is `head` of its target's half-width across, so that it
 * is as large against a phone's cards as against the desktop's, and no less than `least`
 * nor more than `most` stage pixels, `more` as wide again for each unit past the first that
 * it carries, up to `units`. Its light is drawn through `points`. */
const COMET = {
  bow: 0.16,
  rise: 1.1,
  fan: 0.6,
  tail: 0.42,
  head: 0.1,
  least: 1.6,
  most: 6.5,
  more: 0.18,
  units: 4,
  points: 14,
};

/** What a streak is made of, as its look is named (stage.css). */
const lookOf = ({ stream }: StreakAct) =>
  stream.kind === "mana" ? stream.color : "damage";

/** The point `t` of the way along the arc from `a` to `b` by way of `c`. */
const along = (a: Point, c: Point, b: Point, t: number): Point => ({
  x: (1 - t) * (1 - t) * a.x + 2 * (1 - t) * t * c.x + t * t * b.x,
  y: (1 - t) * (1 - t) * a.y + 2 * (1 - t) * t * c.y + t * t * b.y,
});

const round = (n: number) => Math.round(n * 100) / 100;

/** The comet between the reaches `from` and `to`, its head `through` of the way there and
 * carrying `amount` units: from the middle of one to the middle of the other on an arc that
 * rises up the stage, to the left of a way straight up or down it. It is the `nth` after
 * the first of those that leave that place for that target together. */
export function cometOf(
  from: Reach,
  to: Reach,
  through: number,
  amount = 1,
  nth = 0,
): StreakShape {
  const [a, b] = [from.middle, to.middle];
  const [dx, dy] = [b.x - a.x, b.y - a.y];
  const far = Math.hypot(dx, dy) || 1;
  const rise =
    Math.min(COMET.bow * far, COMET.rise * 2 * to.half.y) *
    (1 + COMET.fan * nth);
  // The side of the way that lies up the stage; for a way straight up or down, its left.
  const side = dx === 0 ? 1 : Math.sign(dx);
  const c = {
    x: (a.x + b.x) / 2 + ((side * dy) / far) * rise,
    y: (a.y + b.y) / 2 - ((side * dx) / far) * rise,
  };
  const units = Math.min(Math.max(amount, 1), COMET.units);
  const wide =
    Math.min(Math.max(COMET.head * to.half.x, COMET.least), COMET.most) *
    (1 + COMET.more * (units - 1));
  // The tail is drawn in as the head comes to land.
  const back = Math.max(0, through - COMET.tail * (1 - through ** 3));
  // Each side of the light from its tail, a point, to its head, as wide as it gets.
  const left: Point[] = [];
  const right: Point[] = [];
  let before = along(a, c, b, back);
  for (let i = 1; i <= COMET.points; i++) {
    const share = i / COMET.points;
    const at = along(a, c, b, back + (through - back) * share);
    const [ux, uy] = [at.x - before.x, at.y - before.y];
    const step = Math.hypot(ux, uy) || 1;
    const half = wide * share * share;
    left.push({ x: at.x - (uy / step) * half, y: at.y + (ux / step) * half });
    right.push({ x: at.x + (uy / step) * half, y: at.y - (ux / step) * half });
    before = at;
  }
  const tail = along(a, c, b, back);
  const head = along(a, c, b, through);
  const r = right.at(-1)!;
  const line = (points: Point[]) =>
    points.map((p) => `L${round(p.x)} ${round(p.y)}`).join("");
  return {
    d: `M${round(tail.x)} ${round(tail.y)}${line(left)}A${round(wide)} ${round(wide)} 0 0 0 ${round(r.x)} ${round(r.y)}${line(right.reverse().slice(1))}Z`,
    head,
    tail,
    wide,
  };
}

interface Running {
  key: string;
  act: StreakAct;
  /** How many streaks under way left its place for its target as it did, before it. */
  nth: number;
  origin: Reach | undefined;
  from: number;
  to: number;
  ease: Timing["ease"];
}

/** The streaks under way on a stage, in the order they began. */
export class Streaks {
  #running: Running[] = [];
  #made = 0;

  /** Plays the step `act`, which runs from `from` to `to` in stage time: its streak leaves
   * `origin`, where its source lay as the step began, none for a source with no place on
   * the table. A step of no time draws none. */
  play(
    act: StreakAct,
    origin: Reach | undefined,
    from: number,
    to: number,
    ease: Timing["ease"],
  ) {
    if (to <= from) return;
    this.#made += 1;
    const key = `${lookOf(act)} ${this.#made}`;
    const beside = (other: Running) =>
      other.from === from &&
      namedBy(other.act.to) === namedBy(act.to) &&
      other.origin?.middle.x === origin?.middle.x &&
      other.origin?.middle.y === origin?.middle.y;
    const nth = this.#running.filter(beside).length;
    this.#running.push({ key, act, nth, origin, from, to, ease });
  }

  /** The streaks at stage time `now`, `reach` saying how what each lands on lies then. */
  at(now: number, reach: (on: On) => Reach | undefined): Streak[] {
    return this.#running.flatMap((run) => {
      const { key, act, origin, from, to, ease } = run;
      if (now < from || now >= to) return [];
      const through = ease((now - from) / (to - from));
      const on = namedBy(act.to);
      const target = reach(on);
      const shape =
        origin && target
          ? cometOf(origin, target, through, act.amount, run.nth)
          : null;
      return [{ key, look: lookOf(act), to: on, through, shape }];
    });
  }

  /** Draws every streak on `surface` as `streaks` has them at stage time `now`, and takes
   * off it those that have landed. */
  write(surface: StreakSurface | undefined, now: number, streaks: Streak[]) {
    for (const { key, to } of this.#running) {
      if (now >= to) surface?.streak(key);
    }
    this.#running = this.#running.filter((run) => now < run.to);
    for (const { key, look, shape } of streaks) {
      surface?.streak(key, look, shape);
    }
  }

  /** Takes every streak off `surface`, which the stage no longer draws on; and with `over`,
   * ends them all: the stage starts over. */
  clear(surface: StreakSurface | undefined, over: boolean) {
    for (const { key } of this.#running) surface?.streak(key);
    if (over) this.#running = [];
  }
}
