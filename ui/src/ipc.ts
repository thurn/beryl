// Typed wrappers for the Tauri commands in rust/crates/beryl_app/src/lib.rs, over a transport:
// Tauri inside the app, or a fixture that replays a recorded stream of messages so screens run
// in a plain browser (`just ui-shots`).
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export interface AppInfo {
  version: string;
  smoke: boolean;
}

export type Unlisten = () => void;

export interface Transport {
  invoke<T>(command: string, args?: Record<string, unknown>): Promise<T>;
  listen<T>(event: string, handler: (payload: T) => void): Promise<Unlisten>;
}

/** One recorded message: a command's response, or an event the app emitted. */
export type FixtureMessage =
  | { kind: "response"; command: string; value: unknown }
  | { kind: "event"; event: string; payload: unknown };

export interface Fixture {
  messages: FixtureMessage[];
}

const tauriTransport: Transport = {
  invoke: (command, args) => invoke(command, args),
  listen: (event, handler) => listen(event, (e) => handler(e.payload as never)),
};

/** Replays a fixture: each command gets its recorded responses in order (the last one again
 * once they run out), and each listener gets every recorded event of its name, in order. */
export function fixtureTransport(fixture: Fixture): Transport {
  const responses = new Map<string, unknown[]>();
  for (const m of fixture.messages) {
    if (m.kind === "response") {
      responses.set(m.command, [...(responses.get(m.command) ?? []), m.value]);
    }
  }
  return {
    invoke<T>(command: string): Promise<T> {
      const queue = responses.get(command);
      if (!queue || queue.length === 0) {
        return Promise.reject(new Error(`fixture: no response for ${command}`));
      }
      return Promise.resolve(
        (queue.length > 1 ? queue.shift() : queue[0]) as T,
      );
    },
    async listen<T>(event: string, handler: (payload: T) => void) {
      let live = true;
      const payloads = fixture.messages.flatMap((m) =>
        m.kind === "event" && m.event === event ? [m.payload as T] : [],
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
  ],
};

async function chooseTransport(): Promise<Transport> {
  if ("__TAURI_INTERNALS__" in window) {
    return tauriTransport;
  }
  // `?fixture=<name>` replays /fixtures/<name>.json, served by Vite from ui/public/fixtures.
  const name = new URLSearchParams(window.location.search).get("fixture");
  if (!name) {
    return fixtureTransport(browserFixture);
  }
  const response = await fetch(`/fixtures/${encodeURIComponent(name)}.json`);
  if (!response.ok) {
    throw new Error(`fixture ${name}: HTTP ${response.status}`);
  }
  return fixtureTransport((await response.json()) as Fixture);
}

let chosen: Promise<Transport> | undefined;

export function transport(): Promise<Transport> {
  chosen ??= chooseTransport();
  return chosen;
}

export const appInfo = async () =>
  (await transport()).invoke<AppInfo>("app_info");

export const uiReady = async () => (await transport()).invoke<void>("ui_ready");

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
