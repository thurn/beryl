// The spec server (`beryl_cli specs-serve`), as the spec pages ask it (screens/Testing.tsx).
import type { CardRecords } from "../ipc";

/** Where Vite proxies the spec server (vite.config.ts). */
export const API = "/spec-api";

/** A spec server answer: `{ok}` or `{err}`, as `serve`'s commands answer. */
export async function post<T>(path: string, body: unknown): Promise<T> {
  const r = await fetch(`${API}${path}`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });
  const answer = (await r.json()) as { ok?: T; err?: { message?: string } };
  if ("err" in answer) throw new Error(answer.err?.message ?? "refused");
  return answer.ok as T;
}

/** The card store's records for `names`, which the spec server answers as the app does. */
export const specCards = (names: string[]) =>
  post<CardRecords>("/invoke/card_records", { request: { names } });
