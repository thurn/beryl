// Typed wrappers for the Tauri commands in rust/crates/beryl_app/src/lib.rs.
import { invoke } from "@tauri-apps/api/core";

export interface AppInfo {
  version: string;
  smoke: boolean;
}

export const appInfo = () => invoke<AppInfo>("app_info");

export const uiReady = () => invoke<void>("ui_ready");

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
