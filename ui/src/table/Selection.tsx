import { useSyncExternalStore, type CSSProperties } from "react";
import { createPortal } from "react-dom";
import { Button } from "../components/Controls";
import type { DomSurface } from "../stage/dom";
import type { Name } from "../stage/layout";
import type { ScriptStage } from "../stage/stage";
import type { TableClient } from "./client";
import "./selection.css";

const empty = new Set<Name>();
const emptySelection = () => empty;
const noSubscription = () => () => {};

export function useTableSelection(client?: TableClient) {
  return useSyncExternalStore(
    client?.subscribeSelection ?? noSubscription,
    client?.selectionSnapshot ?? emptySelection,
  );
}

export function useTableSelecting(client?: TableClient) {
  return useSyncExternalStore(
    client?.subscribeSelection ?? noSubscription,
    client?.selectingSnapshot ?? (() => false),
  );
}

export const SELECTION_BAR_HEIGHT = 52;

export function TouchSelection({
  client,
  actions,
}: {
  client: TableClient;
  actions: (element: HTMLElement) => void;
}) {
  const { size } = useTableSelection(client);
  return (
    <div className="table-selection-bar">
      <output aria-label="Selected cards">
        {size} {size === 1 ? "card" : "cards"}
      </output>
      <Button
        disabled={!size}
        onClick={(event) => actions(event.currentTarget)}
      >
        Actions
      </Button>
      <Button onClick={() => client.setSelecting(false)}>Done</Button>
    </div>
  );
}

export function SelectionMark({ frame }: { frame?: number }) {
  return (
    <div
      className="stage-mark stage-mark-selected manual-selection-mark"
      data-local-mark=""
      style={
        frame === undefined
          ? undefined
          : ({ "--frame": `${frame}px` } as CSSProperties)
      }
      aria-hidden
    />
  );
}

export function Selection({
  client,
  stage,
  surface,
}: {
  client: TableClient;
  stage: ScriptStage;
  surface: DomSurface;
}) {
  const selected = useTableSelection(client);
  const selecting = useTableSelecting(client);
  const sweep = useSyncExternalStore(
    client.subscribeSelection,
    client.sweepSnapshot,
  );
  const resting = useSyncExternalStore(
    stage.subscribe,
    () => stage.shown.resting,
  );
  const marks = [];
  let count = 0;
  let power = 0;
  let toughness = 0;
  let anchor: { x: number; y: number } | undefined;
  for (const name of selected) {
    const entry = resting.get(name);
    const node = surface.node(name);
    if (!entry || !node) continue;
    marks.push(createPortal(<SelectionMark />, node, name));
    if (entry.object?.zone !== "battlefield") continue;
    const { pose, object } = entry;
    count++;
    power += object.pt?.power ?? 0;
    toughness += object.pt?.toughness ?? 0;
    if (!anchor || pose.y < anchor.y)
      anchor = { x: pose.x + pose.width / 2, y: pose.y };
  }

  return (
    <>
      {marks}
      {sweep && (
        <div
          className="selection-rectangle"
          aria-hidden
          style={{
            left: sweep.left,
            top: sweep.top,
            width: sweep.right - sweep.left,
            height: sweep.bottom - sweep.top,
          }}
        />
      )}
      {anchor && !selecting && (
        <output
          className="selection-summary"
          aria-label="Selection"
          style={{
            left: `clamp(68px, ${anchor.x}px, calc(100% - 68px))`,
            top: anchor.y,
          }}
        >
          <span>
            {count} {count === 1 ? "card" : "cards"}
          </span>
          <span>
            {power}/{toughness}
          </span>
        </output>
      )}
    </>
  );
}
