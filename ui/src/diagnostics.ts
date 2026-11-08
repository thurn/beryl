// The UI's own log, which a bug report carries (components/BugReport.tsx): every command and
// its answer, every game view as it arrives, route changes, uncaught errors, rejected promises
// and console warnings and errors, each stamped on the wall clock and the page's clock. The
// latest game views are kept whole too, so a report can replay them as a UI fixture.
//
// Warnings and errors also go to the app's journal (`ui_log`, beryl_session::journal), so they
// outlive the page: a crash or a reload loses none.
import type { GameUpdate, Level, UiLogEntry } from "./generated/protocol";

export interface UiEntry {
  /** Wall clock, ISO. */
  time: string;
  /** The page's clock, in milliseconds (freeze.ts's: it stops while frozen). */
  at: number;
  level: Level;
  /** What kind of thing happened: invoke, result, event, error, console, route, note. */
  kind: string;
  message: string;
  data?: unknown;
}

const LOG_LIMIT = 2000;
const UPDATE_LIMIT = 60;
const log: UiEntry[] = [];
const updates: GameUpdate[] = [];
let forwarder: ((entries: UiLogEntry[]) => Promise<unknown>) | undefined;
let outbox: UiLogEntry[] = [];
let flushing: number | undefined;

/** Logs an entry; warnings and errors go on to the app's journal. */
export function note(
  level: Level,
  kind: string,
  message: string,
  data?: unknown,
) {
  log.push({
    time: new Date().toISOString(),
    at: Math.round(performance.now()),
    level,
    kind,
    message,
    data,
  });
  if (log.length > LOG_LIMIT) log.splice(0, log.length - LOG_LIMIT);
  if (level === "warn" || level === "error") {
    outbox.push({ level, message: `${kind}: ${message}`, data: data ?? null });
    flushing ??= window.setTimeout(flush, 250);
  }
}

function flush() {
  flushing = undefined;
  const entries = outbox;
  outbox = [];
  // A journal that cannot be reached says nothing: saying so would log again.
  if (forwarder && entries.length > 0) void forwarder(entries).catch(() => {});
}

/** Where warnings and errors go on to: the app's journal, once a transport can reach it. */
export function forwardTo(send: (entries: UiLogEntry[]) => Promise<unknown>) {
  forwarder = send;
  if (outbox.length > 0) flushing ??= window.setTimeout(flush, 0);
}

/** Logs a game view as it arrives, and keeps it whole among the latest. */
export function noteUpdate(update: GameUpdate, past: boolean) {
  updates.push(update);
  if (updates.length > UPDATE_LIMIT) updates.shift();
  note("debug", "event", "game-update", { past, ...summary(update) });
}

/** Where a view stands, in a line: enough to find it among the whole ones. */
export function summary(update: GameUpdate) {
  const { view } = update;
  const status = view.status;
  return {
    session: update.session,
    status: status.kind,
    decision:
      status.kind === "deciding"
        ? { seq: status.decision.id.seq, kind: status.decision.kind }
        : undefined,
    turn: view.turn.number,
    step: view.turn.step,
    stack: view.stack.length,
    events: view.events.map((e) => e.kind),
    undone: view.undone || undefined,
  };
}

/** What a report carries: the log, and the latest views whole, oldest first. */
export function diagnostics() {
  return { log: [...log], updates: [...updates] };
}

/** A value for the log: a game view summarized, anything else as is. */
export function brief(value: unknown): unknown {
  if (value && typeof value === "object" && "view" in value) {
    return summary(value as GameUpdate);
  }
  return value;
}

const describe = (value: unknown): string =>
  value instanceof Error
    ? value.message
    : typeof value === "string"
      ? value
      : (() => {
          try {
            return JSON.stringify(value);
          } catch {
            return String(value);
          }
        })();

let installed = false;

/** Logs uncaught errors, rejected promises, console warnings and errors, and routes. */
export function installDiagnostics() {
  if (installed) return;
  installed = true;
  window.addEventListener("error", (e) => {
    note("error", "error", e.message, {
      source: `${e.filename}:${e.lineno}:${e.colno}`,
      stack: (e.error as Error | undefined)?.stack,
    });
  });
  window.addEventListener("unhandledrejection", (e) => {
    const reason: unknown = e.reason;
    note("error", "rejection", describe(reason), {
      stack: reason instanceof Error ? reason.stack : undefined,
    });
  });
  for (const level of ["warn", "error"] as const) {
    const original = console[level].bind(console);
    console[level] = (...args: unknown[]) => {
      note(level, "console", args.map(describe).join(" "), {
        stack: new Error().stack,
      });
      original(...args);
    };
  }
  const route = () =>
    note("info", "route", window.location.pathname + window.location.hash);
  window.addEventListener("hashchange", route);
  route();
}
