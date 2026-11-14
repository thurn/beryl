// The stage: it holds the table, plays the scripts the session sends over it on its
// own clock, and says where every object is at any stage time. One entry per game object, named
// as scripts name it: a card by its slot, which it keeps through every zone, a token's being
// used again only after the step that ended it, which removes its node; and an ability on the
// stack by the number its `emerge` gave it (script/mod.rs, Names).
//
// An entry's pose is a function of stage time: its resting pose from the layout, or the track
// of a step under way. Poses are data. `pose` reads the model, never the page, and the surface
// only writes what the model computed, once a tick. No browser-timed animation runs here: no
// CSS transition, no Web Animation left to play, no timer (the findings on by-w8gv.21.3). The
// model outlives what it is drawn on: a surface is mounted and taken away as the page shows
// and hides the table, and finds every node where the model has it.
//
// How a step moves things: at its start its act is applied to the table and the table is laid
// out again, and everything whose resting place that moved glides there for as long as the
// verb takes, which is all a `fly` is. The step's own card or ability travels over everything
// at rest; what only makes room for it (a row closing up, a phone's rows as its stack comes
// and goes) moves at the depth it rests at, under what flies. A verb whose subject does not
// travel from where it was gives it a track of its own (enter.ts): an `emerge` starts on the
// ability's source, an `appear` is there at once and only fades in, a card you draw is held up
// on its way, a `tap` turns its permanent, a `strike` is its creature's lunge, and a `flip`
// turns a permanent over where it lies. A card that leaves face down and lands face up turns
// over on the way, and a spell cast, a permanent resolving and an ability lifting leave the
// straight line in an arc (tracks.ts `glide`). The act of a step the table opens for, a
// token's, is applied as its delay begins, so that its row has made room when it shows.
//
// One verb's subject does not go where the table has it as the step begins: what a `vanish`
// takes goes from sight where it was, dressed as it was, and only as the step ends is it at
// its new place, or no more (`#lay`, `#left`). What it leaves room for closes up after the
// step, once it has gone, and the beat lasts until it has (tracks.ts CLOSES, CLOSED).
//
// Steps are sequenced inside the tick: a beat begins at the stage time the beat before it
// ended, however late the tick that finds it over, so a script's timeline is a function of the
// script alone and a pose is the same whether its time is reached in frames or in one jump.
// Promises are for those who watch (`idle`), never for sequencing.
//
// What follows a step besides motion (a sound) is told its beginning and its end the same way
// (`follow`): inside the tick, in the order of their stage times, each with its own time. What
// a step draws besides nodes (an effect, an arrow, a streak, an announcement) is overlay.ts's.
//
// One pose is not the clock's: a card the pointer holds (pointer.ts). What the pointer does
// enters the model as data, and the held card's pose is computed from it here like any other,
// so `pose` returns it, a step that moves the card starts from where it is held, and a card
// let go glides home on the stage's clock.
import type { PlayerView } from "../generated/protocol";
import { laid, under } from "./beat.ts";
import type { Edge, Playback, Playing, Run } from "./beat.ts";
import { Clock, type Frames, type StageClock } from "./clock.ts";
import { StageIdle } from "./idle.ts";
import { awaited, backed, entering, showingOn, stillShown } from "./enter.ts";
import type { Effect, On } from "./effects.ts";
import { sameDress, sameFace } from "./layout.ts";
import type { Name, Resting, StageLayout } from "./layout.ts";
import { heldPose, Pointer } from "./pointer.ts";
import type { Shown } from "./shown.ts";
import { samePose, type Pose } from "./pose.ts";
import { reachOfCard, reachOfLife, reachOfPool } from "./lines.ts";
import type { Reach } from "./lines.ts";
import { Overlay, type Surface } from "./overlay.ts";
export type { Surface } from "./overlay.ts";
import type { Line, Marked } from "./spans.ts";
import type { Streak } from "./streaks.ts";
import { apply, tabled, tableOf, traded } from "./table.ts";
import type { Act, Script, Table } from "./table.ts";
import {
  carried,
  carries,
  piled,
  dissolve,
  halfTurned,
  home,
  keep,
  moverOf,
  paying,
  subjectOf,
  closedBy,
  poseOn,
  SNAP,
  turning,
  type Timing,
  type Track,
} from "./tracks.ts";

export type { Edge, Playing } from "./beat.ts";
export type { Counting, Shown } from "./shown.ts";
export { SNAP, type Timing } from "./tracks.ts";

interface Entry {
  resting: Resting;
  track?: Track;
  /** Its pose is to be written at the tick's end. */
  moved: boolean;
  /** The `vanish` taking it from where `resting` still has it, until that step ends. */
  leaving?: Run;
  /** The `fly` taking it to a pile, until that step ends: the pile shows the top it had. */
  landing?: Run;
  /** What it is drawn as while it has yet to turn over on its way (`Track.turn`): as it
   * rests, but for its back. */
  covered?: Resting;
}

export class ScriptStage {
  readonly clock: StageClock;
  /** How each step is played, given the table it finds. */
  timings: Parameters<typeof laid>[4];
  /** The effects each step begins on what it is about, given the table it finds. */
  effects: (act: Act, found?: PlayerView) => readonly Effect[];
  /** What is drawn besides the nodes: effects, spans, streaks and announcements. */
  readonly #over = new Overlay(
    (on, at, pool) => this.reach(on, at, pool),
    () => this.#layout.sag,
  );
  /** How a held card let go travels back to where it rests. */
  release: Timing = SNAP;
  /** What the pointer does on the stage's nodes, for whoever listens (pointer.ts). */
  readonly pointer: Pointer;
  /** The held card's pose as it was picked up. */
  #lifted: { name: Name; from: Pose } | undefined;
  #layout: StageLayout;
  #surface: Surface | undefined;
  #table: Table | undefined;
  #view: PlayerView | undefined;
  readonly #entries = new Map<Name, Entry>();
  #queue: { view: PlayerView; script: Script }[] = [];
  #playback: Playback | undefined;
  /** When the next beat begins: the end of the last one. */
  #at = 0;
  readonly #idle: StageIdle;
  readonly #listeners = new Set<() => void>();
  readonly #following = new Set<(edge: Edge) => void>();
  /** Stage time is passing over what plays (`skip`): no step's edge is told. */
  #skipping = false;
  #shown: Shown = {
    table: undefined,
    resting: new Map(),
    view: undefined,
    busy: false,
    notices: [],
    counts: [],
  };
  #stale = false;

  /** A stage, drawing on `surface` if given one. With `frames` it follows them from the
   * start; with none its clock is held, and only `clock.advance` moves it. */
  constructor(options: {
    layout: StageLayout;
    surface?: Surface;
    timings: ScriptStage["timings"];
    effects?: ScriptStage["effects"];
    frames?: Frames;
  }) {
    this.#layout = options.layout;
    this.#surface = options.surface;
    this.timings = options.timings;
    this.effects = options.effects ?? (() => []);
    this.clock = new Clock((_, to) => this.#tick(to), options.frames);
    this.#idle = new StageIdle(this.clock);
    this.clock.release();
    this.pointer = new Pointer({
      pose: (name) => this.pose(name),
      lifted: (name) => {
        const entry = this.#entries.get(name)!;
        this.#lifted = { name, from: poseOn(entry, this.clock.now) };
        entry.track = undefined;
        this.#draw(name, entry);
      },
      moved: (name) => {
        const entry = this.#entries.get(name);
        if (entry) this.#draw(name, entry);
      },
      dropped: (name, pose, taken) => {
        this.#lifted = undefined;
        const entry = this.#entries.get(name);
        if (!entry) return;
        if (pose && taken) {
          const at = { from: this.clock.now, to: Infinity };
          entry.track = { ...at, pose: () => pose, stays: true };
        } else if (pose) {
          entry.track = home(this.release, pose, this.clock.now);
        }
        this.#draw(name, entry);
        if (!entry.track) this.#write(this.clock.now); // Home at once: it has arrived.
      },
    });
  }

  /** Draws on `surface` from now on: a node for everything the model holds, where it is. */
  mount(surface: Surface) {
    this.unmount();
    this.#surface = surface;
    for (const [name, entry] of this.#entries) {
      surface.make(name);
      surface.dress(name, entry.covered ?? entry.resting);
      entry.moved = true;
    }
    this.#write(this.clock.now);
  }

  /** Takes every node off the surface it draws on (`surface`, if given): the model plays on. */
  unmount(surface = this.#surface) {
    if (!surface || surface !== this.#surface) return;
    for (const name of this.#entries.keys()) surface.remove(name);
    this.#over.erase(surface);
    this.#surface = undefined;
  }

  /** Shows `view`, by playing the `script` that came with it over the table as it stands.
   * With no script, or one that resets (a seat's first view, an undone one), the stage starts
   * over from the view at once: whatever was playing or waiting is dropped, and nothing
   * moves. */
  play(view: PlayerView, script?: Script) {
    this.#idle.playing();
    if (script && !script.reset && this.#table) {
      if (!this.#busy) this.#at = this.clock.now;
      this.#queue.push({ view, script });
      this.#write(this.clock.now);
      return;
    }
    this.#queue = [];
    this.#playback = undefined;
    this.#over.clear(this.#surface);
    // What was in flight is where it rests, whether or not the view moves that.
    for (const entry of this.#entries.values()) {
      entry.moved ||= entry.track !== undefined;
      entry.track = undefined;
      entry.leaving = entry.landing = undefined;
    }
    let table = tableOf(view, script?.reset, this.#table);
    // A reset's steps are only the spans to show, begun at once.
    for (const { act } of script?.beats.flat() ?? []) {
      table = apply(table, act);
      this.#over.play(act, -Infinity, -Infinity);
    }
    this.#set(table);
    this.#view = view;
    this.#lay(this.clock.now);
    this.#write(this.clock.now);
    this.#idle.settled(this.#view?.status.kind);
  }

  /** Resolves when everything given to `play` has played out. Settled inside the tick. */
  idle(): Promise<void> {
    return this.#idle.wait(this.#busy);
  }

  /** Holds the clock, then advances it to the end of everything given to `play`; returns the
   * stage milliseconds that took. Throws rather than pass `limit`. */
  settle(limit = 60_000): number {
    this.clock.hold();
    return this.#finish(limit);
  }

  /** Plays out at once everything given to `play`, as a click skips what is playing: stage
   * time passes over all of it, and the clock goes on as it was, held or following frames.
   * Those who `follow` are told none of what is passed over. */
  skip() {
    this.#skipping = true;
    try {
      this.#finish(Infinity);
    } finally {
      this.#skipping = false;
    }
  }

  #finish(limit: number): number {
    const start = this.clock.now;
    while (this.#busy) {
      const due = Math.max(this.#due(), this.clock.now);
      if (due - start > limit) {
        throw new Error(`the stage did not settle within ${limit} ms`);
      }
      this.clock.advance(due - this.clock.now);
    }
    return this.clock.now - start;
  }

  /** The step playing now: the first of the beat under way that is not over. */
  get step(): Playing | null {
    const now = this.clock.now;
    const run = this.#playback?.steps.find((s) => !s.begun || now < s.to);
    if (!run) return null;
    const { index, verb, from, to, step } = run;
    return { index, verb, from, to, step };
  }

  /** The cards in slots `a` and `b`, alike in every way, trade places: each takes the other's
   * turn in its hand, pose and track, and the pointer and the surface take each for the
   * other, so nothing on screen changes. Not while a script plays. */
  trade(a: number, b: number) {
    const [one, other] = [this.#entries.get(a), this.#entries.get(b)];
    if (!one || !other || !this.#table || this.#busy) return;
    this.#set(traded(this.#table, a, b));
    [one.track, other.track] = [other.track, one.track];
    const pose = one.resting.pose;
    one.resting = { ...one.resting, pose: other.resting.pose };
    other.resting = { ...other.resting, pose };
    one.moved = other.moved = true;
    this.pointer.trade(a, b);
    this.#surface?.trade?.(a, b);
    this.#lay(this.clock.now);
    this.#write(this.clock.now);
  }

  /** The pose at `clock.now` of the object in the slot `name`, or of the ability on the stack
   * it names (`abilityName`), from the model: no layout is read. */
  pose(name: Name): Pose | null {
    const entry = this.#entries.get(name);
    return entry ? this.#poseOf(name, entry, this.clock.now) : null;
  }

  /** The arrows and lines the stage holds, each as drawn at `clock.now` (overlay.ts). The line
   * to an ability's source is hidden while the view shown asks for more than priority. */
  lines(now = this.clock.now): Line[] {
    return this.#over.lines(now, this.#prompt);
  }

  /** The streaks the stage draws at `clock.now` (streaks.ts). */
  streaks(now = this.clock.now): Streak[] {
    return this.#over.streaks(now);
  }

  /** The marks the stage holds at `clock.now`, on `on` or on anything. */
  marks(on?: On): Marked[] {
    return this.#over.marks(this.clock.now, on);
  }

  /** The effects under way at `clock.now` on `name`, each as far through as it is. */
  effect(name: NonNullable<Effect["on"]>) {
    return this.#over.effect(name, this.clock.now);
  }

  /** What the stage shows now (`Shown`). */
  get shown(): Shown {
    return this.#shown;
  }

  /** Calls `listener` whenever `shown` is replaced; returns how to stop. */
  readonly subscribe = (listener: () => void) => {
    this.#listeners.add(listener);
    return () => {
      this.#listeners.delete(listener);
    };
  };

  /** Calls `follower` as each step begins and as it ends, inside the tick that finds it, in
   * the order of their stage times; returns how to stop. A step that takes no time ends as it
   * begins; one dropped while it plays (a reset, a skip) is told no end. */
  readonly follow = (follower: (edge: Edge) => void) => {
    this.#following.add(follower);
    return () => {
      this.#following.delete(follower);
    };
  };

  /** Lays the table out afresh, with no motion: the stage changed shape, or is `layout`'s
   * from now on. */
  relayout(layout = this.#layout) {
    this.#layout = layout;
    if (!this.#table) return;
    this.#lay(this.clock.now);
    this.#write(this.clock.now);
  }

  /** Stops the clock and takes every node off the surface. */
  close() {
    this.clock.hold();
    this.unmount();
    this.#entries.clear();
  }

  get #busy() {
    return this.#playback !== undefined || this.#queue.length > 0;
  }

  /** The view shown, its script played out, asks the player something other than priority. */
  get #prompt() {
    const status = this.#busy ? undefined : this.#view?.status;
    return status?.kind === "deciding" && status.decision.kind !== "priority";
  }

  /** When the next thing happens: something of the beat under way, or the beat ends. */
  #due() {
    const playback = this.#playback;
    if (!playback) return this.#at;
    return this.#next(playback)?.[0] ?? playback.end;
  }

  /** What happens next in the beat under way, and when: a step that began ends; one whose
   * act went ahead of it begins, and is told; or the next step's act is applied, which
   * begins it unless it went ahead (`Run.opens`). Steps' acts are applied in the script's
   * order: each changes the table the next one reads. Of what happens at one time, an end
   * comes first. */
  #next(playback: Playback) {
    const { steps, opened } = playback;
    const over = steps
      .filter((s) => s.begun && !s.ended && !s.passes)
      .reduce<Run | undefined>(
        (a, b) => (a && a.to <= b.to ? a : b),
        undefined,
      );
    const [seen] = opened;
    const next = steps.find((s) => !s.begun);
    const all: ([at: number, happens: () => void] | undefined)[] = [
      over && [over.to, () => this.#end(over)],
      seen && [seen.from, () => this.#tell(opened.shift()!, "start")],
      next && [next.opens?.at ?? next.from, () => this.#begin(next)],
    ];
    return all.reduce((a, b) => (b && (!a || b[0] < a[0]) ? b : a), undefined);
  }

  /** Runs everything due up to `to`, each thing at its own time, then writes poses once. */
  #tick(to: number) {
    for (;;) {
      if (!this.#playback) {
        const next = this.#queue.shift();
        if (!next) break;
        this.#playback = {
          view: next.view,
          beats: next.script.beats,
          beat: -1,
          played: 0,
          steps: [],
          opened: [],
          end: this.#at,
        };
      }
      const playback = this.#playback;
      const due = this.#next(playback);
      if (due) {
        if (due[0] > to) break;
        this.#pass(due[0]);
        due[1]();
        continue;
      }
      // What closes up after a card that left is of its beat.
      playback.end = closedBy(this.#entries.values(), playback.end);
      if (playback.end > to) break;
      // The beat is over, at its own end and not at this tick's: what follows begins there.
      this.#pass(playback.end);
      this.#at = playback.end;
      if (playback.beat + 1 < playback.beats.length) {
        this.#beat(playback);
      } else {
        // The script has played: the view's prompt and chrome replace the last one's whole,
        // and a card kept where it was let go, which no step moved, goes home.
        this.#playback = undefined;
        for (const entry of this.#entries.values()) {
          if (!entry.track?.stays) continue;
          const from = entry.track.pose(0, entry.resting.pose);
          entry.track = home(this.release, from, this.#at);
          entry.moved = true;
        }
        this.#set({ ...this.#table!, view: tabled(playback.view) });
        this.#view = playback.view;
        this.#lay(this.#at);
      }
    }
    this.#pass(to);
    this.#write(to);
    if (!this.#busy) this.#idle.settled(this.#view?.status.kind);
  }

  /** Ends, each at its own time, the announcements passing over the table that are over by
   * stage time `to`: before whatever else happens then. */
  #pass(to: number) {
    const over = this.#over.notices.filter((run) => run.passes && run.to <= to);
    for (const run of over.sort((a, b) => a.to - b.to)) this.#end(run);
  }

  /** Ends the step `run`, at its own time. */
  #end(run: Run) {
    run.ended = true;
    this.#stale ||= run.verb === "announce" || run.verb === "count";
    if (run.verb === "vanish" || run.verb === "fly") this.#left(run);
    if (run.verb === "announce") this.#over.over(this.#surface, run);
    this.#tell(run, "end");
    this.#affect(run, "end");
  }

  /** Lays out the next beat's steps in time from where the last beat ended (beat.ts). */
  #beat(playback: Playback) {
    playback.played += playback.steps.length;
    playback.beat += 1;
    const beat = playback.beats[playback.beat]!;
    const { steps } = laid(
      beat,
      playback.played,
      this.#at,
      this.#table!,
      this.timings,
      (act) => this.#over.whole(act),
      this.#over.passed(),
    );
    const rests = (name: Name) => this.#entries.get(name)?.resting;
    awaited(steps, this.#layout, this.#table!.view, rests);
    // What passes over the table holds the beat only until it has begun (beat.ts).
    const waited = steps.map((run) => (run.passes ? run.from : run.to));
    const end = Math.max(this.#at, ...waited);
    Object.assign(playback, { steps, end });
  }

  /** Begins a step at its own time: the table is as the step leaves it, and what its change
   * moves glides to where it now rests for as long as the verb takes. One whose act goes
   * ahead of it (`Run.opens`) has it applied then, and what makes room for it glides until
   * the step begins, which is when it is told and what it is about is first seen. */
  #begin(run: Run) {
    const at = run.opens?.at ?? run.from;
    run.begun = true;
    run.found = this.#table!;
    if (run.verb === "announce" && run.to > run.from) {
      this.#over.notices.push(run);
    }
    if (at < run.from) this.#playback!.opened.push(run);
    else this.#tell(run, "start");
    const mover = moverOf(run.step.act);
    const moved = mover === undefined ? undefined : this.#entries.get(mover);
    const was = moved && {
      resting: moved.covered ?? moved.resting,
      pose: this.#poseOf(mover!, moved, at),
    };
    keep(run.step.act, at, this.#entries, (name, entry) =>
      this.#poseOf(name, entry, at),
    );
    this.#set(apply(this.#table!, run.step.act, run.step.cause));
    this.#over.play(run.step.act, run.from, run.to, run.ease);
    // A step about what an earlier one is still taking from sight finds it gone already.
    const subject = subjectOf(run.step.act);
    const entry =
      subject === undefined ? undefined : this.#entries.get(subject);
    if (entry?.leaving) {
      entry.leaving = undefined;
      entry.track = undefined;
    }
    if (entry) entry.landing = piled(run) && run.to > at ? run : undefined;
    if (entry && run.verb === "vanish" && run.to > run.from) {
      const start = this.#poseOf(subject!, entry, run.from);
      entry.leaving = run;
      entry.track = {
        from: run.from,
        to: run.to,
        pose: dissolve(start, run.ease),
      };
    }
    this.#lay(at, run);
    if (run.to > run.from) this.#enter(run, was);
    this.#affect(run, "start");
  }

  /** Begins the effects the step `run` has at this `edge` of it (overlay.ts). Time passed
   * over (`skip`) begins none. */
  #affect(run: Run, edge: Edge["edge"]) {
    if (this.#skipping) return;
    const begun = this.effects(run.step.act, run.found!.view);
    this.#over.affect(run, edge, begun, (name) => this.#entries.has(name));
  }

  /** The `vanish` `run` is over: what it took is where the table has had it since the step
   * began, with no flight, or is no more. Or the `fly` `run` is, which took a card to a pile:
   * it has landed, and is the pile's top. */
  #left(run: Run) {
    const entries = [...this.#entries.values()];
    const entry = entries.find((e) => e.leaving === run || e.landing === run);
    if (!entry) return;
    if (entry.leaving) {
      entry.track = undefined;
      entry.moved = true;
    }
    entry.leaving = entry.landing = undefined;
    this.#lay(run.to);
  }

  /** Gives what the step moves its own motion, where that is no glide from where it was
   * (enter.ts), `was` being how it was before the step's act. */
  #enter(run: Run, was: { resting: Resting; pose: Pose } | undefined) {
    const { act } = run.step;
    const mover = moverOf(act);
    const found = entering(act, run.ease, {
      layout: this.#layout,
      table: this.#table!.view,
      found: run.found!.view,
      placed: (name) => {
        const entry = this.#entries.get(name);
        return (
          entry && {
            resting: entry.resting,
            pose: this.#poseOf(name, entry, run.from),
          }
        );
      },
      was,
      moving:
        mover !== undefined && this.#entries.get(mover)?.track !== undefined,
      beat: this.#playback!.steps.map((other) => other.step.act),
    });
    for (const { name, track, cover, rides } of found) {
      const entry = this.#entries.get(name);
      const ridden = rides && entry?.track?.own ? entry.track : undefined;
      const on = ridden ?? { ...track, from: run.from, to: run.to };
      if (entry) this.#fly(name, entry, { ...on, turn: track.turn }, cover);
    }
  }

  /** Tells those who follow that `run` begins or ends. */
  #tell(run: Run, edge: Edge["edge"]) {
    if (this.#skipping || this.#following.size === 0) return;
    // One that passes over the table may end after its beat, with none earlier in it.
    const steps = this.#playback?.steps ?? [];
    const told: Edge = {
      edge,
      at: edge === "start" ? run.from : run.to,
      span: { from: run.from, to: run.to, turn: run.turn },
      step: run.step,
      table: run.found!,
      earlier: steps
        .slice(0, Math.max(0, steps.indexOf(run)))
        .map((s) => s.step),
      beat: steps.map((s) => s.step),
    };
    for (const follower of [...this.#following]) follower(told);
  }

  #set(table: Table) {
    this.#table = table;
    this.#stale = true;
  }

  /** Lays the table out at stage time `at`: nodes for what it newly holds, none for what it
   * no longer does, and each entry's resting place. An entry whose place moved glides there
   * over the `run` that moved it, from wherever it is, aloft if the run is about it; with no
   * run, or one that takes no time, it is there at once, unless a glide still under way
   * carries it. The layout is told what is still leaving, or flying to a pile: it has yet to
   * arrive. */
  #lay(at: number, run?: Run) {
    const leaving = new Set<Name>();
    for (const [name, entry] of this.#entries)
      if (entry.leaving || entry.landing) leaving.add(name);
    const rest = this.#layout.rest(this.#table!, leaving);
    for (const [name, entry] of [...this.#entries]) {
      if (rest.has(name) || entry.leaving) continue;
      this.#surface?.remove(name);
      this.#entries.delete(name);
      this.#over.gone(name);
      this.pointer.gone(name);
      this.#stale = true;
    }
    for (const [name, resting] of rest) {
      const entry = this.#entries.get(name);
      if (!entry) {
        this.#surface?.make(name);
        this.#surface?.dress(name, resting);
        this.#entries.set(name, { resting, moved: true });
        this.#stale = true;
        continue;
      }
      // What a `vanish` is taking stays as it was until that step is over (`#left`).
      if (entry.leaving) continue;
      const before = entry.resting;
      const face = sameFace(before, resting);
      const dress = sameDress(before.dress, resting.dress);
      const still = samePose(before.pose, resting.pose);
      if (still && face && dress) continue;
      // What is on its own step's track goes on along it, as what a tray kept waits for one.
      const flying =
        (entry.track?.own && at < entry.track.to) ||
        (entry.track?.waits && moverOf(run?.step.act) !== name);
      const moves = !still && !flying && run !== undefined && carries(run, at);
      const start = moves ? this.#poseOf(name, entry, at) : undefined;
      const showing = stillShown(entry.covered, entry.track, at);
      // What did not change stays the very same, so that nothing drawn from it is drawn again.
      entry.resting = {
        ...(face ? before : resting),
        pose: resting.pose,
        dress: dress ? before.dress : resting.dress,
      };
      if (!face || !dress) {
        entry.covered &&= showing ?? backed(entry.resting);
        this.#surface?.dress(name, entry.covered ?? entry.resting);
        // A frame of another width scales otherwise for the same pose.
        entry.moved = true;
        this.#stale = true;
      }
      if (start) {
        const [track, kept] = carried(
          run!,
          at,
          [name, start, before, resting],
          this.#table!,
          (pose, near) =>
            halfTurned(pose, this.#layout.middle(entry.resting, pose), near),
        );
        this.#fly(name, entry, ...showingOn(track, kept, showing));
      }
      entry.moved ||= !still;
    }
  }

  /** Puts `entry` on `track`: until the track turns it over, it is dressed as `cover`, or
   * with none as a card back. */
  #fly(name: Name, entry: Entry, track: Track, cover?: Resting) {
    entry.track = track;
    const covered =
      track.turn !== undefined && (cover || !entry.resting.dress.back);
    if (!covered && !entry.covered) return;
    entry.covered = covered
      ? (cover ?? {
          ...entry.resting,
          dress: { ...entry.resting.dress, back: true },
        })
      : undefined;
    this.#surface?.dress(name, entry.covered ?? entry.resting);
    entry.moved = true;
    this.#stale = true;
  }

  /** Writes the pose at stage time `now` of every entry that moved or is moving, and tells
   * those who draw from `shown` if it changed. */
  #write(now: number) {
    const stirred = this.#over.stirred(now);
    for (const [name, entry] of this.#entries) {
      // It has turned over, or what was turning it is no longer its track.
      if (entry.covered && !turning(entry.track, now)) {
        entry.covered = undefined;
        this.#surface?.dress(name, entry.resting);
        entry.moved = true;
        this.#stale = true;
      }
      if (!entry.moved && !entry.track && !stirred.has(name)) continue;
      this.#draw(name, entry, now);
      entry.moved = false;
      if (entry.track && now >= entry.track.to) entry.track = undefined;
    }
    // A card let go, or flown under the pointer, came to rest: those who draw from `shown`
    // say what the pointer is on now.
    if (this.pointer.settle((name) => !!this.#entries.get(name)?.track))
      this.#stale = true;
    this.#over.write(this.#surface, now, this.#prompt, this.#entries);
    const busy = this.#busy;
    if (!this.#stale && busy === this.#shown.busy) return;
    this.#stale = false;
    const steps = this.#playback?.steps;
    this.#shown = {
      table: this.#table?.view,
      resting: new Map(
        [...this.#entries].map(([name, entry]) => [
          name,
          entry.covered ?? entry.resting,
        ]),
      ),
      view: this.#view,
      busy,
      notices: under(
        this.#over.notices,
        "announce",
        this.#shown.notices,
        (run) => run.step,
      ),
      counts: under(steps, "count", this.#shown.counts, (run) => run),
    };
    for (const listener of [...this.#listeners]) listener();
  }

  /** Where an entry is at stage time `at`: where the pointer holds it, if it does; and turned
   * as a mark on it turns it, a permanent picked to pay a cost (spans.ts). */
  #poseOf(name: Name, entry: Entry, at: number): Pose {
    const held = this.pointer.held;
    const pose =
      held?.name === name && this.#lifted
        ? heldPose(this.#lifted.from, held.start, held.at)
        : poseOn(entry, at);
    const { object, dress } = entry.resting;
    if (dress.zone !== "battlefield" || !object) return pose;
    const turned = this.#over.turned(name, at);
    return turned ? paying(pose, entry.resting.pose, object, turned) : pose;
  }

  /** How one end of a line lies at stage time `at` (lines.ts): a node as the model has it, a
   * player's life total, or with `pool` their unspent mana; none for what it does not hold. */
  reach(on: On, at = this.clock.now, pool = false): Reach | undefined {
    const layout = this.#layout;
    if (on === "you" || on === "opponent") {
      const anchors = layout.anchors(this.#table!.view);
      return pool
        ? reachOfPool(anchors.pool(on))
        : reachOfLife(anchors.life(on));
    }
    const entry = this.#entries.get(on);
    const pose = entry && this.#poseOf(on, entry, at);
    return pose && reachOfCard(layout, entry.resting, pose);
  }

  /** Writes one entry's pose, as the pointer moves it between ticks. */
  #draw(name: Name, entry: Entry, now = this.clock.now) {
    const pose = this.#poseOf(name, entry, now);
    this.#surface?.draw(name, pose, entry.resting.dress.frame);
  }
}
