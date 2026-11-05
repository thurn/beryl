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

/** Replays a fixture: each command gets its recorded responses in order (the last one again
 * once they run out), and each listener gets every recorded event of its name, in order. A
 * fixture is the app's own recording, so its values are taken as their commands' types. */
export function fixtureTransport(fixture: Fixture): Transport {
  const responses = new Map<Command, unknown[]>();
  for (const m of fixture.messages) {
    if (m.kind === "response") {
      responses.set(m.command, [...(responses.get(m.command) ?? []), m.value]);
    }
  }
  return {
    invoke<C extends Command>(command: C): Promise<Result<C>> {
      const queue = responses.get(command);
      if (!queue || queue.length === 0) {
        return Promise.reject(new Error(`fixture: no response for ${command}`));
      }
      return Promise.resolve(
        (queue.length > 1 ? queue.shift() : queue[0]) as Result<C>,
      );
    },
    async listen<E extends keyof Events>(
      event: E,
      handler: (payload: Events[E]) => void,
    ) {
      let live = true;
      const payloads = fixture.messages.flatMap((m) =>
        m.kind === "event" && m.event === event ? [m.payload as Events[E]] : [],
      );
      queueMicrotask(() => payloads.forEach((p) => live && handler(p)));
      return () => {
        live = false;
      };
    },
  };
}

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
  // it did not record.
  const name = new URLSearchParams(window.location.search).get("fixture");
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
  return fixtureTransport({ messages: [...defaults, ...fixture.messages] });
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
  call("choose", { decision, index });

export const setStops = (session: SessionId, stops: StopsSetting) =>
  call("set_stops", { session, stops });

export const onGameUpdate = async (
  handler: (update: Events["game-update"]) => void,
) => (await transport()).listen("game-update", handler);

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
