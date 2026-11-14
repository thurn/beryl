import type { AppInfo } from "../generated/protocol.ts";
import { key } from "../profile.ts";
import { commands, type Scope } from "./commands.ts";
import { bindingChord, type ShortcutBindings } from "./shortcuts.ts";

const saved = "beryl.table.bindings";
const byId = new Map(commands.map((command) => [command.id, command]));

/** Load one store per active app profile, using the platform from appInfo(). */
export class Bindings implements ShortcutBindings {
  readonly #storageKey = key(saved);
  readonly #overrides = new Map<string, string | null>();
  readonly #chords = {
    table: new Map<string, string>(),
    lobby: new Map<string, string>(),
  };

  readonly platform: AppInfo["platform"];

  constructor(platform: AppInfo["platform"]) {
    this.platform = platform;
    try {
      const stored: unknown = JSON.parse(
        localStorage.getItem(this.#storageKey) ?? "null",
      );
      if (stored && typeof stored === "object" && !Array.isArray(stored)) {
        for (const [id, binding] of Object.entries(stored)) {
          if (
            byId.has(id) &&
            (binding === null ||
              (typeof binding === "string" && bindingChord(binding, platform)))
          ) {
            this.#overrides.set(id, binding);
          }
        }
      }
    } catch {
      // An unavailable or unreadable store leaves the defaults in effect.
    }
    this.#rebuild();
  }

  /** The primary binding shown in the command list and menus. */
  get(id: string): string | undefined {
    if (this.#overrides.has(id)) return this.#overrides.get(id) ?? undefined;
    return byId.get(id)?.key?.[this.#platformKey];
  }

  lookup(scope: Scope, chord: string): string | undefined {
    return this.#chords[scope].get(chord);
  }

  /** Replace a binding and return the id of a command that lost its shortcut. */
  bind(id: string, binding: string | null): string | undefined {
    const command = byId.get(id);
    if (!command) return undefined;

    let displaced: string | undefined;
    if (binding !== null) {
      const chord = bindingChord(binding, this.platform);
      if (!chord) return undefined;
      const owner = this.lookup(command.scope, chord);
      if (owner && owner !== id) {
        this.#overrides.set(owner, null);
        displaced = owner;
      }
    }
    this.#overrides.set(id, binding);
    this.#rebuild();
    this.#save();
    return displaced;
  }

  reset() {
    this.#overrides.clear();
    this.#rebuild();
    this.#save();
  }

  get #platformKey() {
    return this.platform === "macos" ? "mac" : "other";
  }

  #rebuild() {
    for (const chords of Object.values(this.#chords)) chords.clear();
    for (const command of commands) {
      if (this.#overrides.has(command.id) || !command.key) continue;
      for (const binding of [command.key, ...(command.key_aliases ?? [])]) {
        const chord = bindingChord(binding[this.#platformKey], this.platform);
        if (chord) this.#chords[command.scope].set(chord, command.id);
      }
    }
    for (const [id, binding] of this.#overrides) {
      const command = byId.get(id);
      const chord = binding && bindingChord(binding, this.platform);
      if (command && chord) this.#chords[command.scope].set(chord, id);
    }
  }

  #save() {
    try {
      if (this.#overrides.size) {
        localStorage.setItem(
          this.#storageKey,
          JSON.stringify(Object.fromEntries(this.#overrides)),
        );
      } else {
        localStorage.removeItem(this.#storageKey);
      }
    } catch {
      // In-memory bindings still work when the browser cannot save them.
    }
  }
}
