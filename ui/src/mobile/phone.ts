// Whether the UI takes its portrait phone layout (mobile/): on a phone, as the app says
// (`AppInfo.mobile`, a Tauri mobile target), or forced with `?phone` for headless runs at phone
// viewports (`just ui-shots`, `just ui-test`).
import { useSyncExternalStore } from "react";

const forced = new URLSearchParams(window.location.search).has("phone");
let mobile = false;
const listeners = new Set<() => void>();

/** The phone layout, while it is on. */
export const isPhone = () => forced || mobile;

/** Takes the app's word on whether it runs on a phone. */
export function setMobile(value: boolean) {
  if (value === mobile) return;
  mobile = value;
  for (const listen of listeners) listen();
}

const subscribe = (listen: () => void) => {
  listeners.add(listen);
  return () => listeners.delete(listen);
};

/** Whether the phone layout is on, re-rendering when the app says so. */
export const usePhone = () => useSyncExternalStore(subscribe, isPhone);
