import type { LogLine, TableAction } from "../generated/protocol";
import type { Point } from "../stage/layout";
import type { TableClient } from "./client";
import { commands } from "./commands";

export type HistoryAction = Extract<
  TableAction,
  { undo: unknown } | { rewind: unknown }
>;

export interface TableLogControls {
  blocked?: readonly number[];
  act(action: HistoryAction): void;
  context(line: LogLine, at: Point): void;
}

export const rewindLabel = commands.find(
  ({ id }) => id === "table.rewind",
)!.label;

export async function sendHistory(
  client: TableClient,
  action: HistoryAction,
  showBlockers: (entries?: readonly number[]) => void,
) {
  showBlockers();
  const reply = await client.send(action);
  if ("refused" in reply && reply.blocked_by?.length)
    showBlockers(reply.blocked_by);
}
