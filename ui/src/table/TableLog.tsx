import { memo } from "react";
import type { LogLine } from "../generated/protocol";
import { LogPanel } from "../game/Narration";
import { rewindLabel, type TableLogControls } from "./tableLog";

export const TableLog = memo(function TableLog({
  rows,
  controls,
  folded,
  open,
  onToggle,
}: {
  rows: readonly LogLine[];
  controls: TableLogControls;
  folded: boolean;
  open: boolean;
  onToggle(): void;
}) {
  const { blocked, act, context } = controls;
  const blockers = new Set(blocked);
  return (
    <LogPanel
      folded={folded}
      open={open}
      onToggle={onToggle}
      manual
      blocked={blocked}
    >
      {rows.map((line) => (
        <li
          key={line.n}
          className={`game-log-entry${line.by === "You" ? "" : " game-log-opponent"}`}
          data-log-entry={line.n}
          data-blocks-undo={blockers.has(line.n) || undefined}
          onContextMenu={(event) => {
            event.preventDefault();
            event.stopPropagation();
            context(line, { x: event.clientX, y: event.clientY });
          }}
        >
          <span className={line.struck ? "table-log-struck" : undefined}>
            <span className="table-log-number">{line.n}</span>{" "}
            <strong>{line.by}</strong> {line.text}
          </span>
          <div
            className="table-log-actions"
            data-batch={line.undoable || line.blockedBy.length > 0 || undefined}
          >
            {(line.undoable || line.blockedBy.length > 0) && (
              <button
                type="button"
                onClick={() => act({ undo: { entry: line.n } })}
              >
                Undo
              </button>
            )}
            <button
              type="button"
              className="table-log-rewind"
              onClick={() => act({ rewind: { to: line.n } })}
            >
              {rewindLabel}
            </button>
          </div>
        </li>
      ))}
    </LogPanel>
  );
});
