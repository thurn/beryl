// The game as the player watches it. The session streams a view after every step change and
// every decision of the opponent's (`thinking` and `playing` views), often faster than anyone
// could follow; here they queue and show one at a time, each held long enough to see what its
// events did, so the opponent's turn plays out rather than jumps. A view the player must
// answer ends each run of them. Views from before the screen started listening (a fixture's
// recorded past) show at once, and the player may skip the rest of a run (`Playback.skip`), as
// a click skips Arena's animations.
import { useEffect, useReducer } from "react";
import {
  currentGame,
  onGameUpdate,
  startGame,
  type GameEvent,
  type GameUpdate,
  type PlayerView,
} from "../ipc";
import { learnNames, logLines, type LogEntry, type Names } from "./log";

/** How long a view holds the screen before the next one, in milliseconds: a step passing
 * briefly, an action long enough to follow, a counter longest. */
export const HOLD = { step: 300, action: 900, counter: 1600 };

/** The log keeps this many entries. */
const LOG_LENGTH = 200;

/** How long `view` stays before a queued view replaces it. A view the player answers, or the
 * game's end, waits for them; a view with nothing in it (the opponent starting to think) goes
 * at once when another is waiting. */
export function holdFor(view: PlayerView): number {
  const { kind } = view.status;
  if (kind !== "thinking" && kind !== "playing") return 0;
  return eventsHold(view);
}

/** How long `view`'s events take to play out, in milliseconds. */
export function eventsHold(view: PlayerView): number {
  return Math.max(0, ...view.events.map(eventHold));
}

function eventHold(e: GameEvent): number {
  switch (e.kind) {
    case "stepBegins":
    case "draw":
    case "tap":
    case "untap":
      return HOLD.step;
    case "zoneChange":
      return e.reason === "counter" ? HOLD.counter : HOLD.action;
    default:
      return HOLD.action;
  }
}

export interface Playback {
  /** The view on screen, carrying the events since the one shown before it. */
  update: GameUpdate;
  /** It was shown as the game played on, so what happened in it is announced; a view from
   * the past just shows. */
  live: boolean;
  /** More views wait to be shown after it. */
  behind: boolean;
  /** Shows every view waiting at once, the last with all their events. */
  skip(): void;
  /** What has happened in the game up to it, oldest first. */
  log: LogEntry[];
}

interface State {
  shown?: GameUpdate;
  live: boolean;
  /** When the view shown may give way to the next, on `Date.now()`'s clock. */
  until: number;
  queue: GameUpdate[];
  log: LogEntry[];
  names: Names;
  /** A game was asked to start and its first view has not arrived. */
  starting: boolean;
  /** The game in progress has not been asked for yet (`currentGame`). */
  resuming: boolean;
}

type Action =
  | { kind: "arrive"; update: GameUpdate; past: boolean; now: number }
  | { kind: "advance"; now: number }
  | { kind: "skip"; now: number }
  | { kind: "starting"; on: boolean }
  | { kind: "resume"; update?: GameUpdate; now: number };

const INITIAL: State = {
  live: false,
  until: 0,
  queue: [],
  log: [],
  names: new Map(),
  starting: false,
  resuming: true,
};

/** `state` showing `updates` (the first ones views that held no time), shown at `now`. */
function show(
  state: State,
  updates: GameUpdate[],
  live: boolean,
  now: number,
): State {
  let { names, log } = state;
  const added: LogEntry[] = [];
  let key = log.at(-1)?.key ?? -1;
  for (const u of updates) {
    names = learnNames(names, u.view);
    for (const line of logLines(u.view, names)) {
      added.push({ ...line, key: ++key });
    }
  }
  const last = updates.at(-1)!;
  // The views passed over lend the one shown their events, so it animates all of them.
  const update =
    updates.length === 1
      ? last
      : {
          ...last,
          view: { ...last.view, events: updates.flatMap((u) => u.view.events) },
        };
  return {
    ...state,
    shown: update,
    live,
    until: now + (live ? holdFor(update.view) : 0),
    names,
    log: added.length ? [...log, ...added].slice(-LOG_LENGTH) : log,
  };
}

function reduce(state: State, action: Action): State {
  switch (action.kind) {
    case "starting":
      return { ...state, starting: action.on };
    case "resume": {
      // A view that arrived meanwhile is as new as the game's latest, or newer.
      const { update, now } = action;
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
      if (state.queue.length === 0 && (past || now >= state.until)) {
        return show(state, [update], !past, now);
      }
      return { ...state, queue: [...state.queue, update] };
    }
    case "advance": {
      if (state.queue.length === 0) return state;
      const queue = [...state.queue];
      const taken: GameUpdate[] = [];
      do {
        taken.push(queue.shift()!);
      } while (queue.length > 0 && holdFor(taken.at(-1)!.view) === 0);
      return show({ ...state, queue }, taken, true, action.now);
    }
    case "skip":
      if (state.queue.length === 0) return state;
      return show({ ...state, queue: [] }, state.queue, true, action.now);
  }
}

/** The game in progress as the player watches it, and how to start one. */
export function usePlayback(): {
  playback?: Playback;
  /** A game is starting: its first view has not arrived. */
  starting: boolean;
  /** The game in progress, if any, is still being asked for, as the page loads. */
  resuming: boolean;
  /** Starts a game against `agent`, replacing any in progress. */
  start(agent: string, seed?: number): Promise<void>;
} {
  const [state, dispatch] = useReducer(reduce, INITIAL);
  useEffect(() => {
    // Stopping a listener takes a moment, in which it may still hear views (StrictMode's
    // second mount hears them too): only the effect still mounted acts on them.
    let mounted = true;
    const act = (action: Action) => mounted && dispatch(action);
    const unlisten = onGameUpdate((update, past) =>
      act({ kind: "arrive", update, past, now: Date.now() }),
    );
    // Once listening, so no view falls between: a reloaded page takes up the game where it is.
    void unlisten
      .then(() => currentGame())
      .then(
        (update) =>
          act({ kind: "resume", update: update ?? undefined, now: Date.now() }),
        () => act({ kind: "resume", now: Date.now() }),
      );
    return () => {
      mounted = false;
      void unlisten.then((stop) => stop());
    };
  }, []);
  // The next queued view takes over once the one shown has had its time.
  const { queue, until, shown } = state;
  useEffect(() => {
    if (queue.length === 0) return;
    const timer = setTimeout(
      () => dispatch({ kind: "advance", now: Date.now() }),
      Math.max(0, until - Date.now()),
    );
    return () => clearTimeout(timer);
  }, [queue, until, shown]);

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
      behind: queue.length > 0,
      skip: () => dispatch({ kind: "skip", now: Date.now() }),
      log: state.log,
    },
    starting: state.starting,
    resuming: state.resuming,
    start,
  };
}
