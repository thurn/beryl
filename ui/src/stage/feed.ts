// What feeds the stage (plan K25): each view the session sends goes to the stage with its
// script, in the order it came, and the stage plays them one after another on its own clock.
// The screen around the table (the prompt, the buttons, the log, a choice over the table)
// takes a view only once its script has played out, which the stage says from inside its
// tick: nothing here waits on a timer, and nothing is rebuilt from a view's events
// (game/playback.ts and game/beats.ts, which this replaces behind `?stage`).
import type { GameUpdate } from "../ipc";
import { logged, UNLOGGED, type LogEntry } from "../game/log.ts";
import type { ScriptStage } from "./stage.ts";

/** What the screen around the table draws from. */
export interface Fed {
  /** The latest update whose script has played out. */
  shown?: GameUpdate;
  /** Something sent to the stage has not played out. */
  behind: boolean;
  /** The latest update held back while gathering (`Feed.gather`). */
  latest?: GameUpdate;
  /** What has happened in the game up to `shown`, oldest first. */
  log: LogEntry[];
}

/** The stage as the feed uses it. */
type Player = Pick<ScriptStage, "play" | "skip" | "shown" | "subscribe">;

export class Feed {
  readonly #stage: Player;
  /** The updates the stage has been given whose scripts have not played out, oldest first. */
  #sent: GameUpdate[] = [];
  /** The updates held back while gathering, oldest first; none when not. */
  #held: GameUpdate[] | undefined;
  #logged = UNLOGGED;
  #fed: Fed = { behind: false, log: UNLOGGED.log };
  readonly #listeners = new Set<() => void>();

  constructor(stage: Player) {
    this.#stage = stage;
    stage.subscribe(this.#played);
  }

  /** What the screen draws from now: a new one whenever any of it changes. */
  readonly fed = () => this.#fed;

  /** Calls `listener` whenever `fed` is replaced; returns how to stop. */
  readonly subscribe = (listener: () => void) => {
    this.#listeners.add(listener);
    return () => {
      this.#listeners.delete(listener);
    };
  };

  /** Takes a view from the session; `past` when it is from before the page listened, which
   * shows at once. Returns whether it began a game. */
  arrive(update: GameUpdate, past: boolean): boolean {
    const session = this.#latest?.session;
    if (session !== undefined && update.session < session) return false;
    const began = session === undefined || update.session > session;
    if (began) {
      // A new game: its first view starts the stage over, whatever was left of the last.
      this.#sent = [];
      this.#held = undefined;
      this.#logged = UNLOGGED;
    }
    if (this.#held && !past) {
      this.#held.push(update);
      this.#publish();
    } else {
      this.#give(update, past);
    }
    return began;
  }

  /** Holds back the views that arrive from now on, as the player's picks for a choice
   * already made go out one decision at a time, so that the table stays as it was under the
   * choice. */
  gather() {
    this.#held ??= [];
  }

  /** Ends `gather`: the views held back play one after another, as the choice gives way to
   * the table. */
  release() {
    const held = this.#held ?? [];
    this.#held = undefined;
    for (const update of held) this.#give(update, false);
    this.#publish();
  }

  /** Plays out at once everything the stage was given, as a click skips Arena's animations. */
  skip() {
    this.#stage.skip();
  }

  /** The latest update taken, shown or not. */
  get #latest(): GameUpdate | undefined {
    return this.#held?.at(-1) ?? this.#sent.at(-1) ?? this.#fed.shown;
  }

  #give(update: GameUpdate, past: boolean) {
    this.#sent.push(update);
    this.#stage.play(update.view, update.view.script);
    if (past) this.#stage.skip();
  }

  /** The stage shows something new: the updates whose scripts have played out are shown. */
  readonly #played = () => {
    const { view } = this.#stage.shown;
    const last = this.#sent.findIndex((update) => update.view === view);
    const played = this.#sent.splice(0, last + 1);
    if (played.length > 0) {
      this.#logged = logged(
        this.#logged,
        played.map((update) => update.view),
      );
    }
    this.#publish(played.at(-1));
  };

  #publish(shown = this.#fed.shown) {
    const before = this.#fed;
    const fed: Fed = {
      shown,
      behind: this.#stage.shown.busy,
      latest: this.#held?.at(-1),
      log: this.#logged.log,
    };
    if (
      fed.shown === before.shown &&
      fed.behind === before.behind &&
      fed.latest === before.latest &&
      fed.log === before.log
    ) {
      return;
    }
    this.#fed = fed;
    for (const listener of [...this.#listeners]) listener();
  }
}
