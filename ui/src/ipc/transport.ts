// The typed transport interface and its native and live HTTP implementations.
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { Commands, Events } from "../generated/protocol";

export type Unlisten = () => void;

export type Command = keyof Commands;
export type Args<C extends Command> = Commands[C]["args"];
export type Result<C extends Command> = Commands[C]["result"];

/** A command's rejection: the typed error the Rust command returned. */
export class CommandError<C extends Command = Command> extends Error {
  constructor(
    readonly command: C,
    readonly error: Commands[C]["error"],
  ) {
    super(`${command}: ${JSON.stringify(error)}`);
  }
}

export interface Transport {
  invoke<C extends Command>(command: C, args: Args<C>): Promise<Result<C>>;
  /** Calls `handler` with each event; `past` says it happened before the listener started
   * listening (a fixture's recorded history), so the UI shows it at once rather than paced. */
  listen<E extends keyof Events>(
    event: E,
    handler: (payload: Events[E], past: boolean) => void,
  ): Promise<Unlisten>;
}

export const tauriTransport: Transport = {
  invoke: <C extends Command>(command: C, args: Args<C>) =>
    invoke<Result<C>>(command, args).catch((error: Commands[C]["error"]) => {
      throw new CommandError(command, error);
    }),
  listen: <E extends keyof Events>(
    event: E,
    handler: (payload: Events[E], past: boolean) => void,
  ) => listen<Events[E]>(event, (e) => handler(e.payload, false)),
};

/** One message a live transport saw, for a reviewer's probes (`window.berylLive`): when it
 * happened on the page's clock, and what it was. */
export type LiveEntry =
  | { at: number; kind: "invoke"; command: Command; args: unknown }
  | {
      at: number;
      kind: "result";
      command: Command;
      ok: boolean;
      value: unknown;
    }
  | { at: number; kind: "event"; event: keyof Events; payload: unknown };

/** A message a spec page (screens/Testing.tsx) hands the frame playing one of its moments: an
 * event from the page's one stream, which carries every frame's. */
export interface RelayedEvent {
  kind: "beryl-relay";
  event: keyof Events;
  payload: unknown;
}

/** The app's commands and events from `beryl_cli serve` at `base`, so a plain browser plays
 * live games with the app's real timing (`just ui-live`). Every message is logged in `log`.
 * A `relayed` page hears its events from the page framing it rather than a stream of its own
 * (`RelayedEvent`). */
export function liveTransport(
  base: string,
  relayed = false,
  session?: string,
): Transport & { log: LiveEntry[] } {
  const log: LiveEntry[] = [];
  const query = session ? `?${new URLSearchParams({ session })}` : "";
  const events = relayed
    ? undefined
    : new EventSource(`${base}/events${query}`);
  // The server subscribes the stream before it answers: once open, no view is missed. The
  // framing page's stream is open before it frames anything.
  const opened = new Promise<void>((resolve) => {
    if (!events || events.readyState === EventSource.OPEN) resolve();
    else events.addEventListener("open", () => resolve(), { once: true });
  });
  return {
    log,
    async invoke<C extends Command>(command: C, args: Args<C>) {
      log.push({ at: performance.now(), kind: "invoke", command, args });
      // A text body keeps the request simple: no CORS preflight.
      const response = await fetch(`${base}/invoke/${command}${query}`, {
        method: "POST",
        body: JSON.stringify(args),
      });
      const answer = (await response.json()) as
        { ok: Result<C> } | { err: Commands[C]["error"] };
      const ok = "ok" in answer;
      log.push({
        at: performance.now(),
        kind: "result",
        command,
        ok,
        value: ok ? answer.ok : answer.err,
      });
      if (!ok) {
        throw new CommandError(command, answer.err);
      }
      return answer.ok;
    },
    async listen<E extends keyof Events>(
      event: E,
      handler: (payload: Events[E], past: boolean) => void,
    ) {
      // Live events are never past: each happens as the page sees it.
      const hear = (payload: Events[E]) => {
        log.push({ at: performance.now(), kind: "event", event, payload });
        handler(payload, false);
      };
      if (!events) {
        const relay = (e: MessageEvent<RelayedEvent>) => {
          if (e.origin !== window.location.origin) return;
          if (e.data?.kind === "beryl-relay" && e.data.event === event) {
            hear(e.data.payload as Events[E]);
          }
        };
        window.addEventListener("message", relay);
        return () => window.removeEventListener("message", relay);
      }
      const stream = events;
      const listener = (e: MessageEvent<string>) =>
        hear(JSON.parse(e.data) as Events[E]);
      stream.addEventListener(event, listener);
      // A command sent once listening (`current_game`) sees no view slip past.
      await opened;
      return () => stream.removeEventListener(event, listener);
    },
  };
}

/** The `beryl_cli serve` base URL a page was opened with (`?live=<url>`), if any. */
export const liveBase = () =>
  new URLSearchParams(window.location.search).get("live") ?? undefined;

/** Whether this game is a spec frame that receives updates from its parent. */
export const isRelayedGame = () =>
  window.parent !== window &&
  new URLSearchParams(window.location.search).has("relay");
