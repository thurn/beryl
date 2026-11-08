// The game as the player watches it. The session streams a view after every step change and
// every decision of the opponent's (`thinking` and `playing` views), often faster than anyone
// could follow; here they queue and show one at a time, each held long enough to see what its
// events did, so the opponent's turn plays out rather than jumps. A view the player must
// answer ends each run of them. Views from before the screen started listening (a fixture's
// recorded past) show at once, and the player may skip the rest of a run (`Playback.skip`), as
// a click skips Arena's animations.
import { useEffect, useLayoutEffect, useReducer } from "react";
import {
  currentGame,
  onGameUpdate,
  startGame,
  type GameUpdate,
  type PlayerView,
} from "../ipc";
import { beats, eventsHold } from "./beats";
import type { Spotlight } from "./layout";
import { learnNames, logLines, type LogEntry, type Names } from "./log";

export { eventsHold, HOLD } from "./beats";

/** The log keeps this many entries. */
const LOG_LENGTH = 200;

/** `window.berylShown` keeps this many views. */
const SHOWN_LOG = 500;

/** How long `view` stays before a queued view replaces it. A view the player answers, or the
 * game's end, waits for them; a view with nothing in it (the opponent starting to think) goes
 * at once when another is waiting. */
export function holdFor(view: PlayerView): number {
  const { kind } = view.status;
  if (kind !== "thinking" && kind !== "playing") return 0;
  return eventsHold(view);
}

/** Holds of the views `beats` made, which their events would not give, and the cards each
 * shows in a showcase. */
const BEAT_HOLDS = new WeakMap<GameUpdate, number>();
const SPOTLIGHTS = new WeakMap<GameUpdate, Spotlight>();

/** How long `update` stays: its beat's hold, or `holdFor` its view. */
function holdOf(update: GameUpdate): number {
  return BEAT_HOLDS.get(update) ?? holdFor(update.view);
}

/** `update` as the views a player can follow (beats.ts), after `before`, the view last shown
 * or queued. */
function split(
  before: GameUpdate | undefined,
  update: GameUpdate,
): GameUpdate[] {
  return beats(before, update).map((beat) => {
    if (beat.hold !== undefined) BEAT_HOLDS.set(beat.update, beat.hold);
    if (beat.spotlight) SPOTLIGHTS.set(beat.update, beat.spotlight);
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
  spotlight?: Spotlight;
  /** When the view shown may give way to the next, on `Date.now()`'s clock. */
  until: number;
  queue: GameUpdate[];
  log: LogEntry[];
  /** The key of the last log entry before the player's latest turn began: an undo goes back
   * to there. */
  turnStart?: number;
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
  let { names, log, turnStart } = state;
  // Keys only grow, past entries an undo took back too.
  let key = Math.max(log.at(-1)?.key ?? -1, turnStart ?? -1);
  for (const u of updates) {
    // What happened since the turn began never did.
    if (u.view.undone && turnStart !== undefined) {
      const kept = turnStart;
      log = log.filter((e) => e.key <= kept);
    }
    names = learnNames(names, u.view);
    const lines = logLines(u.view, names);
    if (lines.length)
      log = [...log, ...lines.map((l) => ({ ...l, key: ++key }))];
    if (startsTurn(u.view)) turnStart = key;
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
    spotlight: live ? SPOTLIGHTS.get(last) : undefined,
    until: now + (live ? holdOf(last) : 0),
    names,
    turnStart,
    log: log === state.log ? log : log.slice(-LOG_LENGTH),
  };
}

/** Whether `view` is where the player's turn began, which an undo goes back to: their first
 * decision of a turn of theirs, with nothing to take back yet. */
function startsTurn(view: PlayerView): boolean {
  return (
    view.status.kind === "deciding" &&
    !view.status.decision.undo &&
    view.turn.active === "you" &&
    view.turn.number > 0
  );
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
      const parts = past
        ? [update]
        : split(state.queue.at(-1) ?? shown, update);
      if (state.queue.length === 0 && (past || now >= state.until)) {
        return show(
          { ...state, queue: parts.slice(1) },
          [parts[0]!],
          !past,
          now,
        );
      }
      return { ...state, queue: [...state.queue, ...parts] };
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
    const w = window as { berylShown?: { at: number; update: GameUpdate }[] };
    const log = (w.berylShown ??= []);
    log.push({ at: performance.now(), update: shown });
    if (log.length > SHOWN_LOG) log.splice(0, log.length - SHOWN_LOG);
  }, [shown]);
  // A spec page framing the game shows it once it has a view (screens/Specs.tsx).
  const showing = shown !== undefined;
  useEffect(() => {
    if (showing && window.parent !== window) {
      window.parent.postMessage(
        { kind: "beryl-shown" },
        window.location.origin,
      );
    }
  }, [showing]);
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
      spotlight: state.spotlight,
      behind: queue.length > 0,
      skip: () => dispatch({ kind: "skip", now: Date.now() }),
      log: state.log,
    },
    starting: state.starting,
    resuming: state.resuming,
    start,
  };
}
