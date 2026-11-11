// The game as the player watches it. The session streams a view after every step change and
// every decision of the opponent's (`thinking` and `playing` views), often faster than anyone
// could follow; here they queue and show one at a time, each held long enough to see what its
// events did, so the opponent's turn plays out rather than jumps. A view the player must
// answer ends each run of them. Views from before the screen started listening (a fixture's
// recorded past) show at once, and the player may skip the rest of a run (`Playback.skip`), as
// a click skips Arena's animations.
import {
  useEffect,
  useLayoutEffect,
  useReducer,
  useSyncExternalStore,
} from "react";
import { isFrozen, subscribeFrozen } from "../freeze";
import {
  startGame,
  type GameUpdate,
  type PlayerView,
  type SessionId,
} from "../ipc";
import { beats, eventsHold, HOLD, type Beat } from "./beats";
import { FLIGHT_HOLD } from "./flight";
import { soundBeat } from "../audio/events";
import { liftsHandling, type Spotlight } from "./layout";
import { logged, UNLOGGED, type LogEntry, type Logged } from "./log";
import { useGameUpdates, useShownToFrame } from "./updates";

export { eventsHold, HOLD } from "./beats";

/** `window.berylShown` keeps this many views. */
const SHOWN_LOG = 500;

/** How long `view` stays before a queued view replaces it. A view the player answers, or the
 * game's end, waits for them; a view with nothing in it (the opponent starting to think) goes
 * at once when another is waiting. */
export function holdFor(view: PlayerView): number {
  const { kind } = view.status;
  if (kind !== "thinking" && kind !== "playing") return 0;
  return Math.max(eventsHold(view), liftsHandling(view) ? HOLD.handling : 0);
}

/** Presentation data kept with queued updates, separate from the engine's view. */
const BEATS = new WeakMap<GameUpdate, Beat>();

/** How long the beat stays before playback may replace it. */
function holdOf(update: GameUpdate, skipped = false): number {
  const beat = BEATS.get(update);
  const hold = beat?.hold ?? holdFor(update.view);
  // A transient trigger gets a full flight even if its resolution is already queued.
  return !skipped && beat?.triggerFlights ? Math.max(hold, FLIGHT_HOLD) : hold;
}

/** `update` as the views a player can follow (beats.ts), after `before`, the view last shown
 * or queued. */
function split(
  before: GameUpdate | undefined,
  update: GameUpdate,
): GameUpdate[] {
  return beats(before, update).map((beat) => {
    BEATS.set(beat.update, beat);
    return beat.update;
  });
}

export interface Playback {
  /** The view on screen, carrying the events since the one shown before it. */
  update: GameUpdate;
  /** It was shown as the game played on, so what happened in it is announced; a view from
   * the past just shows. */
  live: boolean;
  /** Cards it shows apart from where they lie (beats.ts). */
  spotlight?: Spotlight;
  /** Stack entries starting their trigger flight in this beat. */
  triggerFlights?: ReadonlySet<number>;
  /** More views wait to be shown after it. */
  behind: boolean;
  /** Shows every view waiting at once, the last with all their events. */
  skip(): void;
  /** Holds back the views that arrive from now on, as the player's picks for a choice already
   * made go out one decision at a time (an overlay's six cards to exile), so the table stays
   * as it was under the choice rather than playing each pick's events in turn. */
  gather(kind?: "combat"): void;
  /** Ends `gather`: the views held back show as one, with all their events, as the choice
   * gives way to the table, so what the picks did plays together where it can be seen. */
  release(): void;
  /** The latest view held back by `gather`, which the picks still to go answer. */
  latest?: GameUpdate;
  /** What has happened in the game up to it, oldest first. */
  log: LogEntry[];
}

interface State extends Logged {
  shown?: GameUpdate;
  live: boolean;
  spotlight?: Spotlight;
  triggerFlights?: ReadonlySet<number>;
  /** When the view shown may give way to the next, on `Date.now()`'s clock. */
  until: number;
  queue: GameUpdate[];
  /** A game was asked to start and its first view has not arrived. */
  starting: boolean;
  /** The game in progress has not been asked for yet (`currentGame`). */
  resuming: boolean;
  /** The views held back while gathering (`Playback.gather`), oldest first; none when not. */
  gathered?: { updates: GameUpdate[]; kind?: "combat" };
}

type Action =
  | { kind: "arrive"; update: GameUpdate; past: boolean; now: number }
  | { kind: "advance"; now: number }
  | { kind: "skip"; now: number }
  | { kind: "gather"; for?: "combat" }
  | { kind: "release"; now: number }
  | { kind: "starting"; on: boolean }
  | { kind: "resume"; update?: GameUpdate; awaiting?: boolean; now: number };

const INITIAL: State = {
  ...UNLOGGED,
  live: false,
  until: 0,
  queue: [],
  starting: false,
  resuming: true,
};

/** `state` showing `updates` (the first ones views that held no time), shown at `now`. */
function show(
  state: State,
  updates: GameUpdate[],
  live: boolean,
  now: number,
  skipped = false,
): State {
  const last = updates.at(-1)!;
  // The views passed over lend the one shown their events, so it animates all of them.
  const update = merged(updates);
  const beat = live ? BEATS.get(last) : undefined;
  return {
    ...state,
    shown: update,
    live,
    spotlight: beat?.spotlight,
    triggerFlights: skipped ? undefined : beat?.triggerFlights,
    until: now + (live ? holdOf(last, skipped) : 0),
    ...logged(
      state,
      updates.map((u) => u.view),
    ),
  };
}

/** The last of `updates` with the events of them all. */
function merged(updates: GameUpdate[]): GameUpdate {
  const last = updates.at(-1)!;
  if (updates.length === 1) return last;
  return {
    ...last,
    view: { ...last.view, events: updates.flatMap((u) => u.view.events) },
  };
}

/** `state` with `update` shown now, if nothing waits and the view shown has had its time, or
 * else queued; a live update as the views a player can follow (`split`). */
function enqueue(
  state: State,
  update: GameUpdate,
  past: boolean,
  now: number,
): State {
  const parts = past
    ? [update]
    : split(state.queue.at(-1) ?? state.shown, update);
  if (state.queue.length === 0 && (past || now >= state.until)) {
    return show({ ...state, queue: parts.slice(1) }, [parts[0]!], !past, now);
  }
  return { ...state, queue: [...state.queue, ...parts] };
}

function reduce(state: State, action: Action): State {
  switch (action.kind) {
    case "starting":
      return { ...state, starting: action.on };
    case "resume": {
      // A view that arrived meanwhile is as new as the game's latest, or newer.
      const { update, awaiting, now } = action;
      // A game resumed from its save shows as its first view arrives.
      if (awaiting && !state.shown) return state;
      if (!update || state.shown) return { ...state, resuming: false };
      return show({ ...state, resuming: false }, [update], false, now);
    }
    case "arrive": {
      const { update, past, now } = action;
      const shown = state.shown;
      if (shown && update.session < shown.session) return state;
      if (!shown || update.session > shown.session) {
        // A new game: its first view shows at once, whatever was queued from the last.
        return show(
          { ...INITIAL, starting: false, resuming: false },
          [update],
          !past,
          now,
        );
      }
      if (state.gathered && !past) {
        return {
          ...state,
          gathered: {
            ...state.gathered,
            updates: [...state.gathered.updates, update],
          },
        };
      }
      return enqueue(state, update, past, now);
    }
    case "gather":
      return {
        ...state,
        gathered: state.gathered ?? { updates: [], kind: action.for },
      };
    case "release": {
      const gathered = state.gathered;
      if (!gathered) return state;
      const rest = { ...state, gathered: undefined };
      const { updates } = gathered;
      if (updates.length === 0) return rest;
      // Collapse the decisions that assembled the declaration. Once combat plays
      // on, keep the successive steps even if they arrived in the same burst.
      const firstPlaying =
        gathered.kind === "combat"
          ? updates.findIndex(({ view }) => view.status.kind !== "deciding")
          : -1;
      const end = firstPlaying < 0 ? updates.length : firstPlaying + 1;
      return updates
        .slice(end)
        .reduce(
          (state, update) => enqueue(state, update, false, action.now),
          enqueue(rest, merged(updates.slice(0, end)), false, action.now),
        );
    }
    case "advance": {
      if (state.queue.length === 0) return state;
      const queue = [...state.queue];
      const taken: GameUpdate[] = [];
      do {
        taken.push(queue.shift()!);
      } while (queue.length > 0 && holdOf(taken.at(-1)!) === 0);
      return show({ ...state, queue }, taken, true, action.now);
    }
    case "skip":
      if (state.queue.length === 0) return state;
      return show({ ...state, queue: [] }, state.queue, true, action.now, true);
  }
}

/** The game in progress as the player watches it, and how to start one. */
export function usePlayback(): {
  playback?: Playback;
  /** A game is starting: its first view has not arrived. */
  starting: boolean;
  /** The game in progress, if any, is still being asked for, as the page loads. */
  resuming: boolean;
  /** The game resumed from its save as the page loaded, if one was. */
  resumed?: SessionId;
  /** Starts a game against `agent`, replacing any in progress. */
  start(agent: string, seed?: number): Promise<void>;
} {
  const [state, dispatch] = useReducer(reduce, INITIAL);
  const resumed = useGameUpdates({
    arrive: (update, past) =>
      dispatch({ kind: "arrive", update, past, now: Date.now() }),
    resume: (update, awaiting) =>
      dispatch({ kind: "resume", update, awaiting, now: Date.now() }),
  });
  // The next queued view takes over once the one shown has had its time.
  const { queue, until, shown } = state;
  // Whether views wait to be shown, for a capture that waits for the table to settle
  // (`by specs --shots`).
  useEffect(() => {
    (window as { berylBehind?: boolean }).berylBehind = queue.length > 0;
  }, [queue]);
  // Each view as it takes the screen, with the events it animates, on the page's clock: a
  // motion audit (`by specs --motion`, ui/scripts/motion.mjs) matches what moved on screen to
  // what happened in the game. Stamped before the view paints: a passive effect runs after
  // it, late on a busy machine, so the first frame showing the view would come before its
  // time.
  useLayoutEffect(() => {
    if (!shown) return;
    const w = window as {
      berylShown?: { at: number; update: GameUpdate; live: boolean }[];
    };
    const log = (w.berylShown ??= []);
    const at = performance.now();
    log.push({ at, update: shown, live: state.live });
    if (log.length > SHOWN_LOG) log.splice(0, log.length - SHOWN_LOG);
    // What it shows sounds as it shows; a view from the past is silent, as it is still.
    if (state.live) soundBeat(shown.view, at);
  }, [shown]);
  useShownToFrame(shown !== undefined);
  // A frozen page (a bug report, freeze.ts) shows no next view until it thaws.
  const frozen = useSyncExternalStore(subscribeFrozen, isFrozen);
  useEffect(() => {
    if (queue.length === 0 || frozen) return;
    const timer = setTimeout(
      () => dispatch({ kind: "advance", now: Date.now() }),
      Math.max(0, until - Date.now()),
    );
    return () => clearTimeout(timer);
  }, [queue, until, shown, frozen]);

  const start = async (agent: string, seed?: number) => {
    dispatch({ kind: "starting", on: true });
    try {
      await startGame(agent, seed);
    } catch (e) {
      dispatch({ kind: "starting", on: false });
      throw e;
    }
  };
  return {
    playback: shown && {
      update: shown,
      live: state.live,
      spotlight: state.spotlight,
      triggerFlights: state.triggerFlights,
      behind: queue.length > 0,
      skip: () => dispatch({ kind: "skip", now: Date.now() }),
      gather: (kind) => dispatch({ kind: "gather", for: kind }),
      release: () => dispatch({ kind: "release", now: Date.now() }),
      latest: state.gathered?.updates.at(-1),
      log: state.log,
    },
    starting: state.starting,
    resuming: state.resuming,
    resumed,
    start,
  };
}
