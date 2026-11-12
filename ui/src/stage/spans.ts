// The stage's spans (script/mod.rs, Spans): the arrows, lines and marks its scripts have begun
// and not ended. A `point` or `mark` step begins one and a later step ends it, in the same
// script or in another, and between the two the stage keeps it drawn: nothing here is worked
// out again from a view. A span is one thing from its beginning to its end, so what draws it
// is one element for as long (UI Conventions, "A target that moves": the arrow that was drawn
// afresh from every view outlived what it pointed at, and the combat line went and came back).
//
// A line is drawn from where the model has its two ends on every tick (lines.ts), so it
// follows an end that moves exactly, and the script's rule invariants (script/check.rs) see
// that none is left on a card that moves. Like an effect, how far a span has come in at a
// stage time is a function of that time alone. It comes in over its beginning step (a line
// is drawn out, a mark's light comes up) and is cut as its end begins, as Arena cuts its own.
import type { Act, Arrow, Mark } from "../generated/protocol";
import type { On } from "./effects.ts";
import { abilityName } from "./layout.ts";
import type { Shape } from "./lines.ts";
import type { Timing } from "./tracks.ts";

/** A step that begins or ends a span. */
export type SpanAct = Extract<Act, { verb: "point" | "mark" }>;

type Subject = Extract<Act, { verb: "mark" }>["on"];

/** What a script's subject is on the stage: a node's name, or a player. */
export const namedBy = (subject: Subject): On =>
  subject.kind === "object"
    ? subject.slot
    : subject.kind === "ability"
      ? abilityName(subject.id)
      : subject.player;

/** An arrow or line the stage holds, as it is at one stage time: `shown` of it drawn in, and
 * its shape; no shape while there is nothing to draw of it, or while it is `hidden`. */
export interface Line {
  /** What names it among the stage's lines, and its element on the page. */
  key: string;
  arrow: Arrow;
  from: On;
  to: On;
  shown: number;
  hidden: boolean;
  shape: Shape | null;
}

/** A mark the stage holds, as it is at one stage time: `shown` of the way to its full look. */
export interface Marked {
  mark: Mark;
  on: On;
  shown: number;
}

/** Where spans are drawn. With no shape a line's element stays and draws nothing; with no
 * arrow the line is over and nothing of it stays. A mark is drawn on the node of what the
 * stage names `on`, or about that player's life total; with nothing shown it is over. */
export interface SpanSurface {
  line(key: string, arrow?: Arrow, shape?: Shape | null): void;
  mark(on: On, mark: Mark, shown?: number): void;
}

interface Open {
  act: SpanAct;
  key: string;
  /** The stage times it comes in over. */
  from: number;
  to: number;
  ease: Timing["ease"];
  /** The stage time it is over, once the step that ends it has begun: that step's end. */
  until?: number;
}

const keyOf = (act: SpanAct) =>
  act.verb === "point"
    ? `${act.arrow} ${namedBy(act.from)} ${namedBy(act.to)}`
    : `${act.mark} ${namedBy(act.on)}`;

const touches = ({ act }: Open, on: On) =>
  act.verb === "point"
    ? namedBy(act.from) === on || namedBy(act.to) === on
    : namedBy(act.on) === on;

const shownAt = ({ from, to, ease }: Open, now: number) =>
  now >= to ? 1 : now < from ? 0 : ease((now - from) / (to - from));

/** The spans open on a stage, in the order they began. */
export class Spans {
  #open: Open[] = [];
  /** What has ended since it was last drawn, to be taken off the surface. */
  #over: Open[] = [];
  /** The nodes a mark that changes a pose has begun on or ended on since `stirred`. */
  #stirred = new Set<On>();

  /** Plays the step `act`, which runs from `from` to `to` in stage time: a beginning opens
   * its span, which comes in over the step; an end closes it as the step ends, at once for
   * a step of no time (an ability and the line to its source go in one tick, the step that
   * ends the line being all that stands before the one that takes the ability). */
  play(act: SpanAct, from: number, to: number, ease: Timing["ease"]) {
    const key = keyOf(act);
    const at = this.#open.findIndex((open) => open.key === key);
    if (act.span === "end") {
      if (at >= 0) this.#open[at]!.until = to;
      this.#close(from);
    } else if (at < 0) {
      this.#open.push({ act, key, from, to, ease });
    } else {
      // One already open is the same span: it is not drawn afresh, and no longer ends.
      delete this.#open[at]!.until;
    }
    if (act.verb === "mark" && act.mark === "paying") {
      this.#stirred.add(namedBy(act.on));
    }
  }

  /** The stage time from which the span the step `act` begins or ends has been whole: none
   * for a step of no span, or of one not open. */
  whole(act: Act): number | undefined {
    if (act.verb !== "point" && act.verb !== "mark") return undefined;
    const key = keyOf(act);
    return this.#open.find((open) => open.key === key)?.to;
  }

  /** Closes every span that is over by stage time `now`. */
  #close(now: number) {
    const over = (open: Open) => open.until !== undefined && now >= open.until;
    this.#over.push(...this.#open.filter(over));
    this.#open = this.#open.filter((open) => !over(open));
  }

  /** The stage no longer holds `on`: what was on it went with its node. */
  gone(on: On) {
    const [over, open] = [
      this.#open.filter((span) => touches(span, on)),
      this.#open.filter((span) => !touches(span, on)),
    ];
    this.#open = open;
    this.#over.push(...over);
  }

  /** The lines open at stage time `now`, each with the shape `shapeOf` gives it for how far
   * it is drawn, but for one `hidden`. */
  lines(
    now: number,
    hidden: (arrow: Arrow) => boolean,
    shapeOf: (line: Omit<Line, "shape" | "hidden">) => Shape | null,
  ): Line[] {
    this.#close(now);
    return this.#open.flatMap((open): Line[] => {
      if (open.act.verb !== "point") return [];
      const line = {
        key: open.key,
        arrow: open.act.arrow,
        from: namedBy(open.act.from),
        to: namedBy(open.act.to),
        shown: shownAt(open, now),
      };
      const hid = hidden(line.arrow);
      return [{ ...line, hidden: hid, shape: hid ? null : shapeOf(line) }];
    });
  }

  /** The marks open at stage time `now`: on `on`, or with none given, all. */
  marks(now: number, on?: On): Marked[] {
    this.#close(now);
    return this.#open.flatMap((open): Marked[] => {
      if (open.act.verb !== "mark") return [];
      const marked = namedBy(open.act.on);
      if (on !== undefined && marked !== on) return [];
      return [{ mark: open.act.mark, on: marked, shown: shownAt(open, now) }];
    });
  }

  /** How far the permanent `on` is turned by a mark at stage time `now`, from 0 to 1 of a
   * quarter: one picked to pay a cost turns as it would tap (UI Conventions, Marks, paying). */
  turned(on: On, now: number): number {
    const paying = this.#open.find(
      ({ act }) =>
        act.verb === "mark" && act.mark === "paying" && namedBy(act.on) === on,
    );
    return paying ? shownAt(paying, now) : 0;
  }

  /** The nodes whose pose a mark is changing at stage time `now`, or has begun or stopped
   * changing since this was last asked. */
  stirred(now: number): ReadonlySet<On> {
    const stirred = this.#stirred;
    this.#stirred = new Set();
    for (const open of this.#open) {
      if (open.act.verb !== "mark" || open.act.mark !== "paying") continue;
      if (now >= open.to) continue;
      // Now, and once more as it comes to its full turn.
      stirred.add(namedBy(open.act.on));
      this.#stirred.add(namedBy(open.act.on));
    }
    return stirred;
  }

  /** Draws every span on `surface` as `lines` and the marks are at stage time `now`, and
   * takes off it what has ended. */
  write(surface: SpanSurface | undefined, now: number, lines: Line[]) {
    this.#erase(surface, this.#over);
    this.#over = [];
    for (const { key, arrow, shape } of lines) surface?.line(key, arrow, shape);
    for (const { on, mark, shown } of this.marks(now)) {
      surface?.mark(on, mark, shown);
    }
  }

  /** Takes every span off `surface`, which the stage no longer draws on: each is still open. */
  erase(surface: SpanSurface | undefined) {
    this.#erase(surface, [...this.#open, ...this.#over]);
  }

  /** Ends every span at once: the stage starts over. */
  clear(surface: SpanSurface | undefined) {
    this.erase(surface);
    for (const open of this.#open) {
      if (open.act.verb === "mark") this.#stirred.add(namedBy(open.act.on));
    }
    this.#open = [];
    this.#over = [];
  }

  #erase(surface: SpanSurface | undefined, spans: readonly Open[]) {
    for (const { act, key } of spans) {
      if (act.verb === "point") surface?.line(key);
      else surface?.mark(namedBy(act.on), act.mark);
    }
  }
}
