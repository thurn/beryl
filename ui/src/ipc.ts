// Typed wrappers for the Tauri commands in rust/crates/beryl_app, over a transport: Tauri
// inside the app, or a fixture that replays a recorded stream of messages so screens run in a
// plain browser (`just ui-shots`). Every command, its arguments, result and error, and every
// event are generated from rust/crates/beryl_session (`Commands`, `Events`).
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type {
  CandidateIndex,
  Commands,
  DecisionId,
  Events,
  Fixture,
  SessionId,
  StopsSetting,
} from "./generated/protocol";

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
  listen<E extends keyof Events>(
    event: E,
    handler: (payload: Events[E]) => void,
  ): Promise<Unlisten>;
}

const tauriTransport: Transport = {
  invoke: <C extends Command>(command: C, args: Args<C>) =>
    invoke<Result<C>>(command, args).catch((error: Commands[C]["error"]) => {
      throw new CommandError(command, error);
    }),
  listen: <E extends keyof Events>(
    event: E,
    handler: (payload: Events[E]) => void,
  ) => listen<Events[E]>(event, (e) => handler(e.payload)),
};

/** A command the fixture transport received, for tests (`window.berylFixture`). */
export interface FixtureCall {
  command: Command;
  args: unknown;
}

/** Replays a fixture: each command gets its recorded responses in order (the last one again
 * once they run out). Listeners get the recorded events in order: all of them at once, or,
 * given `at`, the first `at`, and then after each `choose` the views up to the next one the
 * player answers or the end, as if the app played on from the recorded answer. Every command
 * is logged in `calls`. A fixture is the app's own recording, so its values are taken as their
 * commands' types. */
export function fixtureTransport(
  fixture: Fixture,
  at?: number,
): Transport & { calls: FixtureCall[] } {
  const responses = new Map<Command, unknown[]>();
  const events: { event: keyof Events; payload: unknown }[] = [];
  for (const m of fixture.messages) {
    if (m.kind === "response") {
      responses.set(m.command, [...(responses.get(m.command) ?? []), m.value]);
    } else {
      events.push(m);
    }
  }
  let released = at ?? events.length;
  type Listener = { event: keyof Events; handler: (payload: never) => void };
  const listeners = new Set<Listener>();
  // Events from..to to `only`, or to every listener still listening.
  const deliver = (from: number, to: number, only?: Listener) =>
    queueMicrotask(() => {
      for (const { event, payload } of events.slice(from, to)) {
        for (const l of only ? [only] : [...listeners]) {
          if (l.event === event && listeners.has(l)) {
            l.handler(payload as never);
          }
        }
      }
    });
  const calls: FixtureCall[] = [];
  return {
    calls,
    invoke<C extends Command>(command: C, args: Args<C>): Promise<Result<C>> {
      calls.push({ command, args });
      const queue = responses.get(command);
      if (!queue || queue.length === 0) {
        return Promise.reject(new Error(`fixture: no response for ${command}`));
      }
      if (command === "choose" && released < events.length) {
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
      handler: (payload: Events[E]) => void,
    ) {
      const listener = { event, handler: handler as (payload: never) => void };
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
  ],
};

async function chooseTransport(): Promise<Transport> {
  if ("__TAURI_INTERNALS__" in window) {
    return tauriTransport;
  }
  // `?fixture=<name>` replays /fixtures/<name>.json, which `beryl_cli record-fixture` writes
  // and Vite serves from ui/public/fixtures, with the plain browser's responses to the commands
  // it did not record; `&at=<n>` starts from its nth view and plays on as `choose` is called.
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

export const onGameUpdate = async (
  handler: (update: Events["game-update"]) => void,
) => (await transport()).listen("game-update", handler);

/** Whether card images can load: only inside the app, whose `beryl-art` scheme serves them.
 * A plain browser (fixtures, `just ui-shots`) draws placeholders and never touches the
 * network. */
export const artAvailable = () => "__TAURI_INTERNALS__" in window;

// Card images come from the app's `beryl-art` URI scheme (rust/crates/beryl_app/src/art.rs),
// which serves Scryfall's image paths from a disk cache and downloads misses.
const SCRYFALL_IMAGES = "https://cards.scryfall.io/";

/** The app URL for an image URL from data/cards.json. */
export const cardImageUrl = (scryfallUrl: string) => {
  const url = new URL(scryfallUrl);
  if (url.origin + "/" !== SCRYFALL_IMAGES) {
    throw new Error(`not a Scryfall image: ${scryfallUrl}`);
  }
  return `beryl-art://localhost${url.pathname}`;
};
