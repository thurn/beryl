// Typed wrappers for the Tauri commands in rust/crates/beryl_app/src/lib.rs.
import { invoke } from "@tauri-apps/api/core";

export interface AppInfo {
  version: string;
  smoke: boolean;
}

export const appInfo = () => invoke<AppInfo>("app_info");

export const uiReady = () => invoke<void>("ui_ready");
