import { useSyncExternalStore } from "react";
import { createPortal } from "react-dom";
import type { LabelView, Side } from "../generated/protocol";
import type { DomSurface } from "../stage/dom";
import { shakeRefusal } from "../stage/Refusal";
import type { ScriptStage } from "../stage/stage";
import type { TableClient } from "./client";
import "./assists.css";

function Labels({
  labels,
  client,
}: {
  labels: readonly LabelView[];
  client: TableClient;
}) {
  if (!labels.length) return null;
  return (
    <div className="table-assist-labels">
      {labels.map(({ id, text }) => (
        <button
          key={id}
          type="button"
          data-assist-label=""
          onClick={(event) => {
            const source = event.currentTarget;
            void client
              .answerAssist({ kind: "applyLabel", id }, source)
              .catch(() => shakeRefusal(source));
          }}
        >
          {text}
        </button>
      ))}
    </div>
  );
}

export function CardLabels({
  labels,
  client,
  stage,
  surface,
}: {
  labels: readonly LabelView[];
  client: TableClient;
  stage: ScriptStage;
  surface: DomSurface;
}) {
  const resting = useSyncExternalStore(
    stage.subscribe,
    () => stage.shown.resting,
  );
  const byCard = new Map<number, LabelView[]>();
  for (const label of labels) {
    if (label.on.kind !== "object") continue;
    const { slot } = label.on;
    const card = resting.get(slot);
    if (card?.object?.zone !== "battlefield") continue;
    const existing = byCard.get(slot);
    if (existing) existing.push(label);
    else byCard.set(slot, [label]);
  }
  return [...byCard].map(([slot, labels]) => {
    const node = surface.node(slot);
    return (
      node &&
      createPortal(<Labels labels={labels} client={client} />, node, slot)
    );
  });
}

export function LifeLabels({
  labels,
  side,
  client,
}: {
  labels: readonly LabelView[];
  side: Side;
  client: TableClient;
}) {
  return (
    <Labels
      labels={labels.filter(
        ({ on }) => on.kind === "player" && on.player === side,
      )}
      client={client}
    />
  );
}
