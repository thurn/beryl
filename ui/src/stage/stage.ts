// The stage (plan K25): it holds the table, plays the scripts the session sends over it on its
// own clock, and says where every object is at any stage time. One entry per game object, named
// as scripts name it, by slot: a card keeps its slot through every zone, and a token's slot is
// used again only after the step that ended it, which removes its node (script/mod.rs, Names).
//
// An entry's pose is a function of stage time: its resting pose from the layout, or the track
// of a step under way. Poses are data. `pose` reads the model, never the page, and the surface
// only writes what the model computed, once a tick. No browser-timed animation runs here: no
// CSS transition, no Web Animation left to play, no timer (the findings on by-w8gv.21.3).
//
// Steps are sequenced inside the tick: a beat begins at the stage time the beat before it
// ended, however late the tick that finds it over, so a script's timeline is a function of the
// script alone and a pose is the same whether its time is reached in frames or in one jump.
// Promises are for those who watch (`idle`), never for sequencing.
import type {
  ObjectView,
  PlayerView,
  Script,
  ScriptStep,
  Verb,
} from "../generated/protocol";
import { Clock, type Frames, type StageClock } from "./clock.ts";
import type { Dress, Resting, StageLayout } from "./layout.ts";
import { between, samePose, type Pose } from "./pose.ts";
import { apply, tableOf, type Table } from "./table.ts";

/** How a verb is played, in stage milliseconds: how long its motion takes, how long after its
 * turn comes it begins, how long the next step of its beat waits for it, and its curve from 0
 * to 1. The numbers are the design language's (data/ui-conventions.json, timing.ts). */
export interface Timing {
  duration: number;
  delay: number;
  stagger: number;
  ease(progress: number): number;
}

/** No motion: the step is over in the tick that begins it. Reduced motion is every verb's. */
export const SNAP: Timing = {
  duration: 0,
  delay: 0,
  stagger: 0,
  ease: (progress) => progress,
};

/** Where the stage draws: one node an object, made and removed only as the stage says, and
 * written only with what the model computed. */
export interface Surface {
  make(slot: number): void;
  remove(slot: number): void;
  /** Dresses the node for where its object now rests. */
  dress(slot: number, resting: Resting): void;
  /** Writes the node's pose, its frame `frame` wide. */
  draw(slot: number, pose: Pose, frame: number): void;
}

/** What the stage shows now, for what is drawn from it besides poses (faces, the chrome): a
 * new one whenever a record, a dress or the table changes, and never for motion alone. */
export interface Shown {
  table: PlayerView | undefined;
  resting: ReadonlyMap<number, Resting>;
}

/** The step playing now: its place in its script counting every step, its verb, and its start
 * and end in stage milliseconds. */
export interface Playing {
  index: number;
  verb: Verb;
  from: number;
  to: number;
}

/** The glide of an entry from the pose it had when a step moved its resting place. */
interface Track {
  from: number;
  to: number;
  start: Pose;
  ease(progress: number): number;
}

interface Entry {
  resting: Resting;
  track?: Track;
  /** Its pose is to be written at the tick's end. */
  moved: boolean;
}

/** One step of the beat under way. */
interface Run extends Playing {
  step: ScriptStep;
  ease(progress: number): number;
  begun: boolean;
}

interface Playback {
  view: PlayerView;
  beats: ScriptStep[][];
  /** The beat under way, none before the first. */
  beat: number;
  /** How many steps the beats before it held. */
  played: number;
  steps: Run[];
  /** When the beat under way is over: when its last motion is. */
  end: number;
}

const sameRecord = (a: ObjectView, b: ObjectView) =>
  a === b || JSON.stringify(a) === JSON.stringify(b);

const sameDress = (a: Dress, b: Dress) =>
  a.zone === b.zone &&
  a.look === b.look &&
  a.frame === b.frame &&
  a.back === b.back &&
  a.rim === b.rim &&
  a.count === b.count;

export class ScriptStage {
  readonly clock: StageClock;
  /** How each verb is played. */
  timings: (verb: Verb) => Timing;
  readonly #layout: StageLayout;
  readonly #surface: Surface;
  #table: Table | undefined;
  readonly #entries = new Map<number, Entry>();
  #queue: { view: PlayerView; script: Script }[] = [];
  #playback: Playback | undefined;
  /** When the next beat begins: the end of the last one. */
  #at = 0;
  #watching: (() => void)[] = [];
  readonly #listeners = new Set<() => void>();
  #shown: Shown = { table: undefined, resting: new Map() };
  #stale = false;

  /** A stage drawing on `surface`. With `frames` it follows them from the start; with none
   * its clock is held, and only `clock.advance` moves it. */
  constructor(options: {
    layout: StageLayout;
    surface: Surface;
    timings: (verb: Verb) => Timing;
    frames?: Frames;
  }) {
    this.#layout = options.layout;
    this.#surface = options.surface;
    this.timings = options.timings;
    this.clock = new Clock((_, to) => this.#tick(to), options.frames);
    this.clock.release();
  }

  /** Shows `view`, by playing the `script` that came with it over the table as it stands.
   * With no script, or one that resets (a seat's first view, an undone one), the stage starts
   * over from the view at once: whatever was playing or waiting is dropped, and nothing
   * moves. */
  play(view: PlayerView, script?: Script) {
    if (script && !script.reset && this.#table) {
      if (!this.#busy) this.#at = this.clock.now;
      this.#queue.push({ view, script });
      return;
    }
    this.#queue = [];
    this.#playback = undefined;
    // What was in flight is where it rests, whether or not the view moves that.
    for (const entry of this.#entries.values()) {
      entry.moved ||= entry.track !== undefined;
      entry.track = undefined;
    }
    let table = tableOf(view, script?.reset);
    // A reset's steps are only the spans to show, begun at once.
    for (const step of script?.beats.flat() ?? [])
      table = apply(table, step.act);
    this.#set(table);
    this.#lay(this.clock.now);
    this.#write(this.clock.now);
    this.#release();
  }

  /** Resolves when everything given to `play` has played out. Settled inside the tick. */
  idle(): Promise<void> {
    if (!this.#busy) return Promise.resolve();
    return new Promise((resolve) => this.#watching.push(resolve));
  }

  /** Holds the clock, then advances it to the end of everything given to `play`; returns the
   * stage milliseconds that took. Throws rather than pass `limit`. */
  settle(limit = 60_000): number {
    this.clock.hold();
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
    return run
      ? { index: run.index, verb: run.verb, from: run.from, to: run.to }
      : null;
  }

  /** The pose of the object in `slot` at `clock.now`, from the model: no layout is read. */
  pose(slot: number): Pose | null {
    const entry = this.#entries.get(slot);
    return entry ? poseAt(entry, this.clock.now) : null;
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

  /** Lays the table out afresh, with no motion: the stage changed shape. */
  relayout() {
    if (!this.#table) return;
    this.#lay(this.clock.now);
    this.#write(this.clock.now);
  }

  /** Stops the clock and takes every node off the surface. */
  close() {
    this.clock.hold();
    for (const slot of this.#entries.keys()) this.#surface.remove(slot);
    this.#entries.clear();
  }

  get #busy() {
    return this.#playback !== undefined || this.#queue.length > 0;
  }

  /** When the next thing happens: a step of the beat under way begins, or the beat ends. */
  #due() {
    const playback = this.#playback;
    if (!playback) return this.#at;
    return playback.steps.find((s) => !s.begun)?.from ?? playback.end;
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
          end: this.#at,
        };
      }
      const playback = this.#playback;
      // Steps begin in the script's order: each changes the table the next one reads.
      const next = playback.steps.find((s) => !s.begun);
      if (next) {
        if (next.from > to) break;
        this.#begin(next);
        continue;
      }
      if (playback.end > to) break;
      // The beat is over, at its own end and not at this tick's: what follows begins there.
      this.#at = playback.end;
      if (playback.beat + 1 < playback.beats.length) {
        this.#beat(playback);
      } else {
        // The script has played: the view's prompt and chrome replace the last one's whole.
        this.#playback = undefined;
        this.#set({ ...this.#table!, view: playback.view });
        this.#lay(this.#at);
      }
    }
    this.#write(to);
    if (!this.#busy) this.#release();
  }

  /** Lays out the next beat's steps in time from where the last beat ended. */
  #beat(playback: Playback) {
    playback.played += playback.steps.length;
    playback.beat += 1;
    let from = this.#at;
    let turn = this.#at;
    playback.end = this.#at;
    playback.steps = playback.beats[playback.beat]!.map((step, i) => {
      const timing = this.timings(step.act.verb);
      from = Math.max(from, turn + timing.delay);
      turn += timing.stagger;
      const to = from + timing.duration;
      playback.end = Math.max(playback.end, to);
      return {
        index: playback.played + i,
        verb: step.act.verb,
        from,
        to,
        step,
        ease: timing.ease,
        begun: false,
      };
    });
  }

  /** Begins a step at its own time: the table is as the step leaves it, and what its change
   * moves glides to where it now rests for as long as the verb takes. */
  #begin(run: Run) {
    run.begun = true;
    this.#set(apply(this.#table!, run.step.act));
    this.#lay(run.from, run);
  }

  #set(table: Table) {
    this.#table = table;
    this.#stale = true;
  }

  /** Lays the table out at stage time `at`: nodes for what it newly holds, none for what it
   * no longer does, and each entry's resting place. An entry whose place moved glides there
   * over the `run` that moved it, from wherever it is; with no run, or one that takes no
   * time, it is there at once, unless a glide still under way carries it. */
  #lay(at: number, run?: Run) {
    const rest = this.#layout.rest(this.#table!.view);
    for (const slot of [...this.#entries.keys()]) {
      if (rest.has(slot)) continue;
      this.#surface.remove(slot);
      this.#entries.delete(slot);
      this.#stale = true;
    }
    for (const [slot, resting] of rest) {
      const entry = this.#entries.get(slot);
      if (!entry) {
        this.#surface.make(slot);
        this.#surface.dress(slot, resting);
        this.#entries.set(slot, { resting, moved: true });
        this.#stale = true;
        continue;
      }
      const before = entry.resting;
      const object = sameRecord(before.object, resting.object)
        ? before.object
        : resting.object;
      const dress = sameDress(before.dress, resting.dress)
        ? before.dress
        : resting.dress;
      const still = samePose(before.pose, resting.pose);
      if (still && object === before.object && dress === before.dress) continue;
      if (!still) {
        if (run && run.to > at) {
          const start = poseAt(entry, at);
          entry.track = { from: at, to: run.to, start, ease: run.ease };
        }
        entry.moved = true;
      }
      entry.resting = { object, pose: resting.pose, dress };
      if (object !== before.object || dress !== before.dress) {
        this.#surface.dress(slot, entry.resting);
        // A frame of another width scales otherwise for the same pose.
        entry.moved = true;
        this.#stale = true;
      }
    }
  }

  /** Writes the pose at stage time `now` of every entry that moved or is moving, and tells
   * those who draw from `shown` if it changed. */
  #write(now: number) {
    for (const [slot, entry] of this.#entries) {
      if (!entry.moved && !entry.track) continue;
      this.#surface.draw(slot, poseAt(entry, now), entry.resting.dress.frame);
      entry.moved = false;
      if (entry.track && now >= entry.track.to) entry.track = undefined;
    }
    if (!this.#stale) return;
    this.#stale = false;
    this.#shown = {
      table: this.#table?.view,
      resting: new Map(
        [...this.#entries].map(([slot, entry]) => [slot, entry.resting]),
      ),
    };
    for (const listener of [...this.#listeners]) listener();
  }

  #release() {
    const watching = this.#watching;
    this.#watching = [];
    for (const resolve of watching) resolve();
  }
}

/** Where an entry is at stage time `at`: on its track while one runs, else at rest. */
function poseAt(entry: Entry, at: number): Pose {
  const track = entry.track;
  if (!track || at >= track.to) return entry.resting.pose;
  const progress = Math.max(0, (at - track.from) / (track.to - track.from));
  return between(track.start, entry.resting.pose, track.ease(progress));
}
