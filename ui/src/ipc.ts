// Typed wrappers for the Tauri commands in rust/crates/beryl_app, over a transport: Tauri
// inside the app, or a fixture that replays a recorded stream of messages so screens run in a
// plain browser (`just ui-shots`). Every command, its arguments, result and error, and every
// event are generated from rust/crates/beryl_session (`Commands`, `Events`).
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type {
  BugReport,
  CandidateIndex,
  Commands,
  DecisionId,
  Events,
  Fixture,
  GameUpdate,
  SessionId,
  StopsSetting,
} from "./generated/protocol";
import { brief, forwardTo, note, noteUpdate } from "./diagnostics";
import { whenThawed } from "./freeze";

export type * from "./generated/protocol";

export type Unlisten = () => void;

type Command = keyof Commands;
type Args<C extends Command> = Commands[C]["args"];
type Result<C extends Command> = Commands[C]["result"];

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

const tauriTransport: Transport = {
  invoke: <C extends Command>(command: C, args: Args<C>) =>
    invoke<Result<C>>(command, args).catch((error: Commands[C]["error"]) => {
      throw new CommandError(command, error);
    }),
  listen: <E extends keyof Events>(
    event: E,
    handler: (payload: Events[E], past: boolean) => void,
  ) => listen<Events[E]>(event, (e) => handler(e.payload, false)),
};

/** A command the fixture transport received, for tests (`window.berylFixture`). */
export interface FixtureCall {
  command: Command;
  args: unknown;
}

/** Replays a fixture: each command gets its recorded responses in order (the last one again
 * once they run out). Listeners get the recorded events in order: all of them at once, or,
 * given `at`, the first `at` (or all but the last `-at`, if negative), and then after each `choose` the views up to the next one the
 * player answers or the end, as if the app played on from the recorded answer. A `cancel`
 * goes back to the priority the cast began at, shown again with nothing happening in it, and
 * the next `choose` plays the recording on from there; an `undo` goes back to the decision before
 * the latest, shown again as undone; a `pass_on` plays on as a `choose` does. The events a listener gets as it
 * starts listening are past; those released by `choose` and `cancel` happen live.
 * Every command is logged in `calls`. A fixture is the app's own recording, so its values are taken as their
 * commands' types. */
export function fixtureTransport(
  fixture: Fixture,
  at?: number,
): Transport & {
  calls: FixtureCall[];
  view(): Events["game-update"]["view"] | undefined;
} {
  const responses = new Map<Command, unknown[]>();
  const events: { event: keyof Events; payload: unknown }[] = [];
  for (const m of fixture.messages) {
    if (m.kind === "response") {
      responses.set(m.command, [...(responses.get(m.command) ?? []), m.value]);
    } else {
      events.push(m);
    }
  }
  // A negative `at` counts back from the end, so a state named by its place before the
  // recording's last view outlives a re-recording that plays out differently.
  let released =
    at === undefined ? events.length : at < 0 ? events.length + at : at;
  type Listener = {
    event: keyof Events;
    handler: (payload: never, past: boolean) => void;
  };
  const listeners = new Set<Listener>();
  // Events from..to to `only` (as past events), or to every listener still listening.
  const deliver = (from: number, to: number, only?: Listener) =>
    queueMicrotask(() => {
      for (const { event, payload } of events.slice(from, to)) {
        for (const l of only ? [only] : [...listeners]) {
          if (l.event === event && listeners.has(l)) {
            l.handler(payload as never, only !== undefined);
          }
        }
      }
    });
  const calls: FixtureCall[] = [];
  const viewAt = (i: number) =>
    (events[i]?.payload as Events["game-update"] | undefined)?.view;
  /** Shows the recorded view `at` again, with nothing happening in it, `undone` if so; the
   * next `choose` plays the recording on from there. */
  const showAgain = (at: number, undone: boolean) => {
    const update = events[at]!.payload as Events["game-update"];
    released = at + 1;
    queueMicrotask(() => {
      for (const l of [...listeners]) {
        if (l.event === "game-update" && listeners.has(l)) {
          l.handler(
            {
              ...update,
              view: { ...update.view, events: [], undone },
            } as never,
            false,
          );
        }
      }
    });
    return Promise.resolve(null);
  };
  /** Takes back the latest answer: the decision before the latest view's, again. */
  const undo = () => {
    const latest = viewAt(released - 1);
    if (latest?.status.kind !== "deciding" || !latest.status.decision.undo) {
      return Promise.reject(
        new CommandError("undo", { kind: "nothingToUndo" }),
      );
    }
    let at = released - 1;
    while (at > 0) {
      at -= 1;
      if (viewAt(at)?.status.kind === "deciding") break;
    }
    return showAgain(at, true);
  };
  /** Takes back the cast the latest view is part of: its priority again, as it was. */
  const cancel = () => {
    const latest = viewAt(released - 1);
    if (
      latest?.status.kind !== "deciding" ||
      !latest.status.decision.cancellable
    ) {
      return Promise.reject(
        new CommandError("cancel", { kind: "notCancellable" }),
      );
    }
    let at = released - 1;
    while (at > 0) {
      at -= 1;
      const v = viewAt(at);
      if (
        v?.status.kind === "deciding" &&
        v.status.decision.kind === "priority"
      )
        break;
    }
    return showAgain(at, false);
  };
  return {
    calls,
    /** The latest view released, which the screen should show once it settles: what
     * `just ui-shots` checks the screen against. */
    view() {
      const latest = events
        .slice(0, released)
        .reverse()
        .find((e) => e.event === "game-update");
      return (latest?.payload as Events["game-update"] | undefined)?.view;
    },
    invoke<C extends Command>(command: C, args: Args<C>): Promise<Result<C>> {
      calls.push({ command, args });
      if (command === "cancel") return cancel() as Promise<Result<C>>;
      if (command === "undo") return undo() as Promise<Result<C>>;
      // A recording holds one line of play: passing on plays it on as its next answer did.
      const queue = responses.get(command === "pass_on" ? "choose" : command);
      if (!queue || queue.length === 0) {
        return Promise.reject(new Error(`fixture: no response for ${command}`));
      }
      if (
        (command === "choose" || command === "pass_on") &&
        released < events.length
      ) {
        const from = released;
        let last;
        do {
          last = events[released];
          released += 1;
        } while (released < events.length && last && playsOn(last));
        deliver(from, released);
      }
      return Promise.resolve(
        (queue.length > 1 ? queue.shift() : queue[0]) as Result<C>,
      );
    },
    async listen<E extends keyof Events>(
      event: E,
      handler: (payload: Events[E], past: boolean) => void,
    ) {
      const listener = {
        event,
        handler: handler as (payload: never, past: boolean) => void,
      };
      listeners.add(listener);
      deliver(0, released, listener);
      return () => void listeners.delete(listener);
    },
  };
}

/** Whether a recorded event is a view of the game playing on without the player, which the
 * next view follows unasked. */
const playsOn = (e: { event: keyof Events; payload: unknown }) => {
  const kind = (e.payload as Events["game-update"]).view.status.kind;
  return kind === "thinking" || kind === "playing";
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

/** A message a spec page (screens/Specs.tsx) hands the frame playing one of its moments: an
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
): Transport & { log: LiveEntry[] } {
  const log: LiveEntry[] = [];
  const events = relayed ? undefined : new EventSource(`${base}/events`);
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
      const response = await fetch(`${base}/invoke/${command}`, {
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
const liveBase = () =>
  new URLSearchParams(window.location.search).get("live") ?? undefined;

/** What a plain browser gets without `?fixture=`: enough for the shell to render. */
const browserFixture: Fixture = {
  messages: [
    {
      kind: "response",
      command: "app_info",
      value: { version: "browser", smoke: false },
    },
    { kind: "response", command: "ui_ready", value: null },
    { kind: "response", command: "list_agents", value: [] },
    { kind: "response", command: "current_game", value: null },
    { kind: "response", command: "resume_game", value: null },
    { kind: "response", command: "concede", value: null },
  ],
};

/** A value for the log: a report cut to what says what it was. */
const logged = (command: Command, value: unknown): unknown =>
  command === "report_bug"
    ? { description: (value as { request?: BugReport }).request?.description }
    : brief(value);

/** `inner`, every message logged for bug reports (diagnostics.ts), and its events held while
 * the page is frozen (freeze.ts). */
function instrumented(inner: Transport): Transport {
  return {
    async invoke<C extends Command>(command: C, args: Args<C>) {
      // The log's own forwarding is not logged: it would never end.
      if (command === "ui_log") return inner.invoke(command, args);
      note("debug", "invoke", command, logged(command, args));
      try {
        const result = await inner.invoke(command, args);
        note("debug", "result", command, logged(command, result));
        return result;
      } catch (e) {
        // A command the game refused is journaled by the app; anything else is the UI's to say.
        if (e instanceof CommandError) {
          note("info", "result", `${command} refused`, e.error);
        } else {
          note("error", "result", `${command} failed: ${String(e)}`);
        }
        throw e;
      }
    },
    listen<E extends keyof Events>(
      event: E,
      handler: (payload: Events[E], past: boolean) => void,
    ) {
      return inner.listen(event, (payload, past) => {
        if (event === "game-update") noteUpdate(payload as GameUpdate, past);
        whenThawed(() => handler(payload, past));
      });
    },
  };
}

/** Whether this page can write bug reports: the app, or a live browser page of its own
 * (`?live=`), not one framed by a spec page or replaying a fixture. */
export const canReportBugs = () =>
  window.parent === window &&
  ("__TAURI_INTERNALS__" in window || liveBase() !== undefined);

/** Whether the app can snapshot its window, for a report's frozen frame. */
export const canCaptureWindow = () => "__TAURI_INTERNALS__" in window;

async function chooseTransport(): Promise<Transport> {
  const wrapped = instrumented(await rawTransport());
  if (canReportBugs()) {
    forwardTo((entries) => wrapped.invoke("ui_log", { request: { entries } }));
  }
  return wrapped;
}

async function rawTransport(): Promise<Transport> {
  if ("__TAURI_INTERNALS__" in window) {
    return tauriTransport;
  }
  const live = liveBase();
  if (live) {
    const relayed =
      window.parent !== window &&
      new URLSearchParams(window.location.search).has("relay");
    const transport = liveTransport(live, relayed);
    (window as { berylLive?: unknown }).berylLive = transport;
    return transport;
  }
  // `?fixture=<name>` replays /fixtures/<name>.json, which `beryl_cli record-fixture` writes
  // and Vite serves from ui/public/fixtures (`by ui-test`'s pages fetch their own), with the
  // plain browser's responses to the commands it did not record; `&at=<n>` starts from its nth
  // view and plays on as `choose` is called.
  const params = new URLSearchParams(window.location.search);
  const name = params.get("fixture");
  if (!name) {
    return fixtureTransport(browserFixture);
  }
  const response = await fetch(`/fixtures/${encodeURIComponent(name)}.json`);
  if (!response.ok) {
    throw new Error(`fixture ${name}: HTTP ${response.status}`);
  }
  const fixture = (await response.json()) as Fixture;
  const recorded = new Set(
    fixture.messages.flatMap((m) => (m.kind === "response" ? [m.command] : [])),
  );
  const defaults = browserFixture.messages.filter(
    (m) => m.kind === "response" && !recorded.has(m.command),
  );
  const at = params.get("at");
  const replay = fixtureTransport(
    { messages: [...defaults, ...fixture.messages] },
    at === null ? undefined : Number(at),
  );
  (window as { berylFixture?: unknown }).berylFixture = replay;
  return replay;
}

let chosen: Promise<Transport> | undefined;

export function transport(): Promise<Transport> {
  chosen ??= chooseTransport();
  return chosen;
}

const call = async <C extends Command>(command: C, args: Args<C>) =>
  (await transport()).invoke(command, args);

export const appInfo = () => call("app_info", {});

export const uiReady = () => call("ui_ready", {});

// Games: one at a time, against an agent. Every view of the human's arrives as a
// `game-update` event. The game commands reject with a `CommandError` carrying a
// `SessionError`.

export const listAgents = () => call("list_agents", {});

/** Starts a game against `agent`, replacing any in progress; resolves to its session. */
export const startGame = (agent: string, seed?: number) =>
  call("start_game", { request: { agent, seed: seed ?? null } });

/** Answers the human's decision `decision` with its candidate `index`. */
export const choose = (decision: DecisionId, index: CandidateIndex) =>
  call("choose", { request: { decision, index } });

export const setStops = (session: SessionId, stops: StopsSetting) =>
  call("set_stops", { request: { session, stops } });

/** Takes back the spell or ability being cast, whose open decision `decision` is part of
 * (`DecisionView.cancellable`); the view of the priority it began at arrives again. */
export const cancel = (decision: DecisionId) =>
  call("cancel", { request: { decision } });

/** Takes back the human's latest answer from the open decision `decision`
 * (`DecisionView.undo`); the view of the decision it answered arrives again. */
export const undo = (decision: DecisionId) =>
  call("undo", { request: { decision } });

/** Passes the human's priority from the open decision `decision` on to their next stop
 * (`DecisionView.passOn`); the views as the game plays on arrive. */
export const passOn = (decision: DecisionId) =>
  call("pass_on", { request: { decision } });

/** The human concedes game `session`; the view of its end arrives at the next decision. */
export const concede = (session: SessionId) =>
  call("concede", { request: { session } });

/** The latest view of the game in progress, if any: where a reloaded page takes it up. */
export const currentGame = () => call("current_game", {});

/** Resumes the newest unfinished game saved, unless one is in progress; resolves to the
 * session in progress, if any. Its views arrive as a started game's do. */
export const resumeGame = () => call("resume_game", {});

/** Snapshots the window as drawn now; resolves to its number (only in the app:
 * `canCaptureWindow`). */
export const captureWindow = () => call("capture_window", {});

/** The app's URL for `path` on its `beryl-art` scheme. WebView2 and Android's WebView serve
 * custom schemes at http://<scheme>.localhost instead. */
const artSchemeUrl = (path: string) =>
  /Windows|Android/.test(navigator.userAgent)
    ? `http://beryl-art.localhost${path}`
    : `beryl-art://localhost${path}`;

/** Where the app serves snapshot `id` (rust/crates/beryl_app/src/snapshot.rs). */
export const snapshotUrl = (id: number) => artSchemeUrl(`/snapshot/${id}`);

/** Writes a bug report; resolves to its folder. */
export const reportBug = (request: BugReport) =>
  call("report_bug", { request });

export const onGameUpdate = async (
  handler: (update: Events["game-update"], past: boolean) => void,
) => (await transport()).listen("game-update", handler);

/** Whether card images can load: inside the app, whose `beryl-art` scheme serves them, or in
 * a live browser (`?live=`) or one asked to (`?art`), which load them from Scryfall. A fixture
 * browser (`just ui-shots`) draws placeholders and never touches the network, unless its
 * route asks for art, as when judging how real cards look. */
export const artAvailable = () =>
  "__TAURI_INTERNALS__" in window ||
  liveBase() !== undefined ||
  new URLSearchParams(window.location.search).has("art");

/** Whether the page plays sound (audio/sound.ts): the app, a live browser (`?live=`) or one
 * asked to (`?audio`), and never a page under automation (Playwright: `just ui-shots`,
 * `ui-test`, the specs, an agent driving a live game), which only logs its cues. */
export const audioAvailable = () =>
  !navigator.webdriver &&
  ("__TAURI_INTERNALS__" in window ||
    liveBase() !== undefined ||
    new URLSearchParams(window.location.search).has("audio"));

// Card images come from the app's `beryl-art` URI scheme (rust/crates/beryl_app/src/art.rs),
// which serves Scryfall's image paths from a disk cache and downloads misses: a card's from its
// image host, the card back's (under `/backs`) from its back host.
const SCRYFALL_HOSTS: Record<string, string> = {
  "https://cards.scryfall.io": "",
  "https://backs.scryfall.io": "/backs",
};

/** The app URL for an image URL from data/cards.json: the URL itself in a live browser. */
export const cardImageUrl = (scryfallUrl: string) => {
  const url = new URL(scryfallUrl);
  const prefix = SCRYFALL_HOSTS[url.origin];
  if (prefix === undefined) {
    throw new Error(`not a Scryfall image: ${scryfallUrl}`);
  }
  if (!("__TAURI_INTERNALS__" in window)) {
    return scryfallUrl;
  }
  return artSchemeUrl(`${prefix}${url.pathname}`);
};
