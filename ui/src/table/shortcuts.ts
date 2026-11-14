import type { AppInfo } from "../generated/protocol.ts";
import type { Scope } from "./commands.ts";

type Platform = AppInfo["platform"];
type KeyEvent = Pick<
  KeyboardEvent,
  "code" | "metaKey" | "ctrlKey" | "altKey" | "shiftKey" | "isComposing"
>;

export interface ShortcutBindings {
  lookup(scope: Scope, chord: string): string | undefined;
}

const namedCodes = {
  Space: "Space",
  Tab: "Tab",
  Escape: "Escape",
  Backspace: "Backspace",
  Delete: "Delete",
  Return: "Enter",
  Home: "Home",
  End: "End",
  PageUp: "PageUp",
  PageDown: "PageDown",
  Insert: "Insert",
  ArrowUp: "ArrowUp",
  ArrowDown: "ArrowDown",
  ArrowLeft: "ArrowLeft",
  ArrowRight: "ArrowRight",
  "`": "Backquote",
  "−": "Minus",
  "=": "Equal",
  "[": "BracketLeft",
  "]": "BracketRight",
  "\\": "Backslash",
  ";": "Semicolon",
  "'": "Quote",
  ",": "Comma",
  ".": "Period",
  "/": "Slash",
};
const codesByName = new Map(Object.entries(namedCodes));
const namesByCode = new Map(
  Object.entries(namedCodes).map(([name, code]) => [code, name]),
);
const functionKey = /^F([1-9]|1\d|2[0-4])$/;
const shiftedPunctuation = new Map([
  ["+", "Equal"],
  ["<", "Comma"],
  [">", "Period"],
  ["?", "Slash"],
]);

// A modifier mask compares chords even when their strings list modifiers in different orders.
const control = 1;
const option = 2;
const shift = 4;
const meta = 8;

function codeFor(name: string, mac: boolean) {
  if (/^[a-z]$/i.test(name)) return `Key${name.toUpperCase()}`;
  if (/^\d$/.test(name)) return `Digit${name}`;
  if (functionKey.test(name)) return name;
  if (name === "-") return "Minus";
  // The Mac key labeled Delete is the physical Backspace key.
  if (mac && name === "Delete") return "Backspace";
  if (mac && name === "ForwardDelete") return "Delete";
  return codesByName.get(name);
}

function nameFor(code: string, mac: boolean) {
  if (/^(Key[A-Z]|Digit\d)$/.test(code)) return code.at(-1);
  if (functionKey.test(code)) return code;
  if (mac && code === "Backspace") return "Delete";
  if (mac && code === "Delete") return "ForwardDelete";
  return namesByCode.get(code);
}

function parts(binding: string, platform: Platform) {
  const match = /^((?:[A-Za-z]+\+)*)([^+]+|\+)$/.exec(binding);
  const [, prefix, name] = match ?? [];
  if (prefix === undefined || !name) return undefined;

  const mac = platform === "macos";
  const modifiers = new Map([
    ["Cmd", mac ? meta : control],
    ["Ctrl", control],
    ["Option", option],
    ["Alt", option],
    ["Shift", shift],
    ["Meta", meta],
  ]);
  let flags = 0;
  for (const modifier of prefix.split("+").slice(0, -1)) {
    const flag = modifiers.get(modifier);
    if (flag === undefined) return undefined;
    flags |= flag;
  }
  const shiftedCode = shiftedPunctuation.get(name);
  const code = shiftedCode ?? codeFor(name, mac);
  return code ? { code, flags, shiftedCode, name } : undefined;
}

/** The identity used to match a shortcut and detect conflicting bindings. */
export function bindingChord(binding: string, platform: Platform) {
  const parsed = parts(binding, platform);
  return (
    parsed &&
    `${parsed.flags | (parsed.shiftedCode ? shift : 0)}:${parsed.code}`
  );
}

function eventFlags(event: KeyEvent) {
  return (
    (event.ctrlKey ? control : 0) |
    (event.altKey ? option : 0) |
    (event.shiftKey ? shift : 0) |
    (event.metaKey ? meta : 0)
  );
}

export function resolve(
  event: KeyEvent,
  scope: Scope | null | undefined,
  bindings: ShortcutBindings,
): string | undefined {
  if ((scope !== "table" && scope !== "lobby") || event.isComposing) {
    return undefined;
  }
  return bindings.lookup(scope, `${eventFlags(event)}:${event.code}`);
}

/** Turn a physical key press into a binding for the command list to save. */
export function bindingFor(event: KeyEvent, platform: Platform) {
  if (event.isComposing) return undefined;
  const mac = platform === "macos";
  const name = nameFor(event.code, mac);
  if (!name) return undefined;
  return [
    event.metaKey && (mac ? "Cmd" : "Meta"),
    event.ctrlKey && "Ctrl",
    event.altKey && (mac ? "Option" : "Alt"),
    event.shiftKey && "Shift",
    name,
  ]
    .filter(Boolean)
    .join("+");
}

export function describe(
  binding: string | null | undefined,
  platform: Platform,
) {
  if (binding == null) return "";
  const parsed = parts(binding, platform);
  if (!parsed) return "";
  const mac = platform === "macos";
  const { flags, name } = parsed;
  const labels = [
    flags & control && (mac ? "⌃" : "Ctrl"),
    flags & option && (mac ? "⌥" : "Alt"),
    flags & shift && (mac ? "⇧" : "Shift"),
    flags & meta && (mac ? "⌘" : "Meta"),
    name,
  ];
  return labels.filter(Boolean).join(mac ? "" : "+");
}
