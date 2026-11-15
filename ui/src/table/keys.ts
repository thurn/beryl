import type { AppInfo } from "../generated/protocol.ts";

type Platform = AppInfo["platform"];
type KeyEvent = Pick<
  KeyboardEvent,
  "code" | "metaKey" | "ctrlKey" | "altKey" | "shiftKey" | "isComposing"
>;

// These chords belong to a manual table even if its player rebinds the commands
// that use them. The keyboard listener prevents their native web-view actions.
const claimed = new Set([
  "KeyW",
  "KeyM",
  "Comma",
  "KeyR",
  "KeyP",
  "Equal",
  "Minus",
  "KeyQ",
]);

export function claims(event: KeyEvent, platform: Platform = "macos"): boolean {
  const primary =
    platform === "macos"
      ? event.metaKey && !event.ctrlKey
      : event.ctrlKey && !event.metaKey;
  return (
    primary &&
    !event.altKey &&
    !event.shiftKey &&
    !event.isComposing &&
    claimed.has(event.code)
  );
}
