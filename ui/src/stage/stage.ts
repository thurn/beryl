// The stage (plan K25): it holds the table, plays the scripts the session sends over it on its
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
// over on the way.
//
// One verb's subject does not go where the table has it as the step begins: what a `vanish`
// takes goes from sight where it was, dressed as it was, for as long as the verb takes, and
// only as the step ends is it at its new place, or no more (`#lay`, `#left`).
//
// Steps are sequenced inside the tick: a beat begins at the stage time the beat before it
// ended, however late the tick that finds it over, so a script's timeline is a function of the
// script alone and a pose is the same whether its time is reached in frames or in one jump.
// Promises are for those who watch (`idle`), never for sequencing.
//
// What follows a step besides motion (a sound) is told its beginning and its end the same way
// (`follow`): inside the tick, in the order of their stage times, each with its own time.
//
// One pose is not the clock's: a card the pointer holds (pointer.ts). What the pointer does
// enters the model as data, and the held card's pose is computed from it here like any other,
// so `pose` returns it, a step that moves the card starts from where it is held, and a card
// let go glides home on the stage's clock.
import type {
  Act,
  PlayerView,
  Script,
  ScriptStep,
  Verb,
} from "../generated/protocol";
import { Clock, type Frames, type StageClock } from "./clock.ts";
import { entering } from "./enter.ts";
import type { Dress, Name, Resting, StageLayout } from "./layout.ts";
import { heldPose, Pointer } from "./pointer.ts";
import { samePose, type Pose } from "./pose.ts";
import { apply, tableOf, type Table } from "./table.ts";
import {
  dissolve,
  drawsToRead,
  glide,
  moverOf,
  subjectOf,
  along,
  TURN,
  turning,
  type Track,
} from "./tracks.ts";

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

/** Where the stage draws: one node a name, made and removed only as the stage says, and
 * written only with what the model computed. */
export interface Surface {
  make(name: Name): void;
  remove(name: Name): void;
  /** Dresses the node for where its object or ability now rests. */
  dress(name: Name, resting: Resting): void;
  /** Writes the node's pose, its frame `frame` wide. */
  draw(name: Name, pose: Pose, frame: number): void;
}

/** What the stage shows now, for what is drawn from it besides poses (faces, the chrome): a
 * new one whenever a record, a dress or the table changes, a script has played out, or the
 * stage starts or stops playing, and never for motion alone. */
export interface Shown {
  /** The table as the steps so far have left it. */
  table: PlayerView | undefined;
  resting: ReadonlyMap<Name, Resting>;
  /** The view last shown whole, its script played out: the one the prompt and the chrome
   * around the table are drawn from. */
  view: PlayerView | undefined;
  /** Something given to `play` has not played out. */
  busy: boolean;
  /** The announcements passing over the table: the `announce` steps under way, in the order
   * they began. The banners around the table show for as long as their step is here, on the
   * stage's time. */
  notices: readonly ScriptStep[];
  /** The numbers changing now: the `count` steps under way, in the order they began, each
   * with the table it found. What shows that a number changed (the change beside a life
   * total) shows for as long as its step is here. */
  counts: readonly Counting[];
}

/** A `count` step under way, and the table as it found it. */
export interface Counting {
  step: ScriptStep;
  found?: Table;
}

/** The step playing now: its place in its script counting every step, its verb, its start
 * and end in stage milliseconds, and the step itself. */
export interface Playing {
  index: number;
  verb: Verb;
  from: number;
  to: number;
  step: ScriptStep;
}

/** A step beginning or ending, for what follows steps besides motion (`ScriptStage.follow`). */
export interface Edge {
  edge: "start" | "end";
  /** The stage time it happens at, whenever the tick that finds it runs. */
  at: number;
  step: ScriptStep;
  /** The table as the step found it, before its act. */
  table: Table;
  /** The steps of its beat before it, in order. */
  earlier: readonly ScriptStep[];
}

interface Entry {
  resting: Resting;
  track?: Track;
  /** Its pose is to be written at the tick's end. */
  moved: boolean;
  /** The `vanish` taking it from where `resting` still has it, until that step ends. */
  leaving?: Run;
  /** What it is drawn as while it has yet to turn over on its way (`Track.turn`): as it
   * rests, but for its back. */
  covered?: Resting;
}

/** One step of the beat under way. */
interface Run extends Playing {
  ease(progress: number): number;
  begun: boolean;
  /** The table as the step found it, once it has begun. */
  found?: Table;
  ended: boolean;
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

/** What a resting thing's face is drawn from. */
const faceOf = ({ object, ability, source }: Resting) =>
  object ? { object } : { ability, source };

const sameFace = (a: Resting, b: Resting) =>
  JSON.stringify(faceOf(a)) === JSON.stringify(faceOf(b));

const sameDress = (a: Dress, b: Dress) =>
  a.zone === b.zone &&
  a.look === b.look &&
  a.frame === b.frame &&
  a.back === b.back &&
  a.rim === b.rim &&
  a.count === b.count;

export class ScriptStage {
  readonly clock: StageClock;
  /** How each step is played, given the table it finds. */
  timings: (act: Act, found?: PlayerView) => Timing;
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
  #watching: (() => void)[] = [];
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
    timings: (act: Act, found?: PlayerView) => Timing;
    frames?: Frames;
  }) {
    this.#layout = options.layout;
    this.#surface = options.surface;
    this.timings = options.timings;
    this.clock = new Clock((_, to) => this.#tick(to), options.frames);
    this.clock.release();
    this.pointer = new Pointer({
      pose: (name) => this.pose(name),
      lifted: (name) => {
        const entry = this.#entries.get(name)!;
        this.#lifted = { name, from: poseAt(entry, this.clock.now) };
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
          entry.track = {
            from: this.clock.now,
            to: Infinity,
            pose: () => pose,
            stays: true,
          };
        } else if (pose) {
          entry.track = this.#home(pose, this.clock.now);
        }
        this.#draw(name, entry);
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

  /** Takes every node off the surface it draws on, `surface` if given, and draws on none: the
   * model plays on. */
  unmount(surface = this.#surface) {
    if (!surface || surface !== this.#surface) return;
    for (const name of this.#entries.keys()) surface.remove(name);
    this.#surface = undefined;
  }

  /** Shows `view`, by playing the `script` that came with it over the table as it stands.
   * With no script, or one that resets (a seat's first view, an undone one), the stage starts
   * over from the view at once: whatever was playing or waiting is dropped, and nothing
   * moves. */
  play(view: PlayerView, script?: Script) {
    if (script && !script.reset && this.#table) {
      if (!this.#busy) this.#at = this.clock.now;
      this.#queue.push({ view, script });
      this.#write(this.clock.now);
      return;
    }
    this.#queue = [];
    this.#playback = undefined;
    // What was in flight is where it rests, whether or not the view moves that.
    for (const entry of this.#entries.values()) {
      entry.moved ||= entry.track !== undefined;
      entry.track = undefined;
      entry.leaving = undefined;
    }
    let table = tableOf(view, script?.reset);
    // A reset's steps are only the spans to show, begun at once.
    for (const step of script?.beats.flat() ?? [])
      table = apply(table, step.act);
    this.#set(table);
    this.#view = view;
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

  /** The pose at `clock.now` of the object in the slot `name`, or of the ability on the stack
   * it names (`abilityName`), from the model: no layout is read. */
  pose(name: Name): Pose | null {
    const entry = this.#entries.get(name);
    return entry ? this.#poseOf(name, entry, this.clock.now) : null;
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
      // Steps begin in the script's order: each changes the table the next one reads. One
      // that began ends first if its time comes no later than the next one's beginning.
      const next = playback.steps.find((s) => !s.begun);
      const over = playback.steps
        .filter((s) => s.begun && !s.ended)
        .reduce<Run | undefined>(
          (a, b) => (a && a.to <= b.to ? a : b),
          undefined,
        );
      if (over && over.to <= to && (!next || over.to <= next.from)) {
        over.ended = true;
        this.#stale ||= over.verb === "announce" || over.verb === "count";
        if (over.verb === "vanish") this.#left(over);
        this.#tell(over, "end");
        continue;
      }
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
        // The script has played: the view's prompt and chrome replace the last one's whole,
        // and a card kept where it was let go, which no step moved, goes home.
        this.#playback = undefined;
        for (const entry of this.#entries.values()) {
          if (!entry.track?.stays) continue;
          entry.track = this.#home(
            entry.track.pose(0, entry.resting.pose),
            this.#at,
          );
          entry.moved = true;
        }
        this.#set({ ...this.#table!, view: playback.view });
        this.#view = playback.view;
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
    // Each step is timed by the table it will find: the beat's earlier steps played.
    let table = this.#table!;
    playback.steps = playback.beats[playback.beat]!.map((step, i) => {
      const timing = this.timings(step.act, table.view);
      table = apply(table, step.act);
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
        ended: false,
      };
    });
  }

  /** Begins a step at its own time: the table is as the step leaves it, and what its change
   * moves glides to where it now rests for as long as the verb takes. */
  #begin(run: Run) {
    run.begun = true;
    run.found = this.#table!;
    this.#tell(run, "start");
    const mover = moverOf(run.step.act);
    const moved = mover === undefined ? undefined : this.#entries.get(mover);
    const was = moved && {
      resting: moved.covered ?? moved.resting,
      pose: this.#poseOf(mover!, moved, run.from),
    };
    this.#set(apply(this.#table!, run.step.act));
    // A step about what an earlier one is still taking from sight finds it gone already.
    const subject = subjectOf(run.step.act);
    const entry =
      subject === undefined ? undefined : this.#entries.get(subject);
    if (entry?.leaving) {
      entry.leaving = undefined;
      entry.track = undefined;
    }
    if (entry && run.verb === "vanish" && run.to > run.from) {
      const start = this.#poseOf(subject!, entry, run.from);
      entry.leaving = run;
      entry.track = {
        from: run.from,
        to: run.to,
        pose: dissolve(start, run.ease),
      };
    }
    this.#lay(run.from, run);
    if (run.to > run.from) this.#enter(run, was);
  }

  /** The `vanish` `run` is over: what it took is where the table has had it since the step
   * began, with no flight, or is no more. */
  #left(run: Run) {
    const entry = [...this.#entries.values()].find((e) => e.leaving === run);
    if (!entry) return;
    entry.leaving = undefined;
    entry.track = undefined;
    entry.moved = true;
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
      several: this.#playback!.steps.some(
        (other) => other !== run && drawsToRead(other.step.act),
      ),
    });
    const entry = found && this.#entries.get(found.name);
    if (!entry) return;
    const track = { ...found.track, from: run.from, to: run.to };
    this.#fly(found.name, entry, track, found.cover);
  }

  /** Tells those who follow that `run` begins or ends. */
  #tell(run: Run, edge: Edge["edge"]) {
    if (this.#skipping || this.#following.size === 0) return;
    const steps = this.#playback!.steps;
    const told: Edge = {
      edge,
      at: edge === "start" ? run.from : run.to,
      step: run.step,
      table: run.found!,
      earlier: steps.slice(0, steps.indexOf(run)).map((s) => s.step),
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
   * carries it. */
  #lay(at: number, run?: Run) {
    const rest = this.#layout.rest(this.#table!);
    for (const [name, entry] of [...this.#entries]) {
      if (rest.has(name) || entry.leaving) continue;
      this.#surface?.remove(name);
      this.#entries.delete(name);
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
      // What is on its own step's track goes on along it, to wherever it now rests.
      const flying = entry.track?.own && at < entry.track.to;
      const moves = !still && !flying && run !== undefined && run.to > at;
      const start = moves ? this.#poseOf(name, entry, at) : undefined;
      // What did not change stays the very same, so that nothing drawn from it is drawn again.
      entry.resting = {
        ...(face ? before : resting),
        pose: resting.pose,
        dress: dress ? before.dress : resting.dress,
      };
      if (!face || !dress) {
        entry.covered &&= {
          ...entry.resting,
          dress: { ...entry.resting.dress, back: true },
        };
        this.#surface?.dress(name, entry.covered ?? entry.resting);
        // A frame of another width scales otherwise for the same pose.
        entry.moved = true;
        this.#stale = true;
      }
      if (start) {
        // Only the card a `fly` is about travels over what rests; what another verb's
        // change moves (a record that lays its card elsewhere) goes there as what makes room
        // does. It turns over on the way if it leaves face down and lands face up.
        const flies = run!.verb === "fly" && subjectOf(run!.step.act) === name;
        const turns =
          flies && start.face === "back" && resting.pose.face === "front";
        this.#fly(name, entry, {
          from: at,
          to: run!.to,
          pose: glide(start, run!.ease, flies, turns),
          turn: turns ? TURN : undefined,
          own: flies,
        });
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
    for (const [name, entry] of this.#entries) {
      // It has turned over, or what was turning it is no longer its track.
      if (entry.covered && !turning(entry.track, now)) {
        entry.covered = undefined;
        this.#surface?.dress(name, entry.resting);
        entry.moved = true;
        this.#stale = true;
      }
      if (!entry.moved && !entry.track) continue;
      this.#draw(name, entry, now);
      entry.moved = false;
      if (entry.track && now >= entry.track.to) entry.track = undefined;
    }
    const busy = this.#busy;
    if (!this.#stale && busy === this.#shown.busy) return;
    this.#stale = false;
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
      notices: this.#under("announce", this.#shown.notices, (run) => run.step),
      counts: this.#under("count", this.#shown.counts, (run) => run),
    };
    for (const listener of [...this.#listeners]) listener();
  }

  /** Where an entry is at stage time `at`: where the pointer holds it, if it does. */
  #poseOf(name: Name, entry: Entry, at: number): Pose {
    const held = this.pointer.held;
    return held?.name === name && this.#lifted
      ? heldPose(this.#lifted.from, held.start, held.at)
      : poseAt(entry, at);
  }

  /** Writes one entry's pose, as the pointer moves it between ticks. */
  #draw(name: Name, entry: Entry, now = this.clock.now) {
    const pose = this.#poseOf(name, entry, now);
    this.#surface?.draw(name, pose, entry.resting.dress.frame);
  }

  /** The way home from `from`, begun at stage time `at`, of a card let go: none if that takes
   * no time. */
  #home(from: Pose, at: number): Track | undefined {
    const { duration, ease } = this.release;
    if (duration <= 0) return undefined;
    return { from: at, to: at + duration, pose: glide(from, ease, true) };
  }

  /** The steps of `verb` under way, each as `of` gives it: `before`, the list last shown,
   * while none begins or ends. */
  #under<T>(verb: Verb, before: readonly T[], of: (run: Run) => T) {
    const now = (this.#playback?.steps ?? [])
      .filter((run) => run.verb === verb && run.begun && !run.ended)
      .map(of);
    return now.length === before.length && now.every((x, i) => x === before[i])
      ? before
      : now;
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
  return track.pose(along(track, at), entry.resting.pose);
}
