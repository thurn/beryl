import {
  useLayoutEffect,
  useSyncExternalStore,
  type CSSProperties,
} from "react";
import { DragFace } from "./DragFace";
import type { TableClient } from "./client";
import type { TableDrag } from "./drag";
import type { DropArea } from "./dropZones";
import { LibraryOrder } from "./LibraryOrder";
import type { TraySpace } from "../stage/tray";
import "./drag.css";

export function DropTargets({
  client,
  drag,
  areas,
  tray,
}: {
  client: TableClient;
  drag: TableDrag;
  areas: readonly DropArea[];
  tray: TraySpace;
}) {
  const { preview, order } = useSyncExternalStore(
    drag.subscribe,
    drag.snapshot,
  );
  useLayoutEffect(() => {
    drag.areas = areas;
  }, [areas, drag]);
  const carried = order?.cards ?? preview?.cards;
  const first = order || preview?.external ? 0 : 1;
  useLayoutEffect(() => {
    if (!carried) return;
    const nodes = carried.slice(first).flatMap(({ object }) => {
      const node = client.surface?.node(object.slot);
      const shown = document.querySelector<HTMLElement>(
        `[data-viewer-slot="${object.slot}"]`,
      );
      return [node, shown].filter(
        (element) => element !== undefined && element !== null,
      );
    });
    for (const node of nodes) node.dataset.tableCarried = "";
    return () => {
      for (const node of nodes) delete node.dataset.tableCarried;
    };
  }, [carried, first, client]);
  const target = preview?.target;
  return (
    <>
      {target && (
        <div
          className="table-drop-target"
          data-table-drop={target.kind}
          data-side={target.side}
          aria-hidden="true"
          style={{
            left: target.box.left,
            top: target.box.top,
            width: target.box.right - target.box.left,
            height: target.box.bottom - target.box.top,
          }}
        >
          {target.kind === "library" &&
            (["top", "bottom"] as const).map((position) => (
              <div
                key={position}
                className="table-library-half"
                data-position={position}
                data-over={target.position === position || undefined}
              >
                {position === "top" ? "Top" : "Bottom"}
              </div>
            ))}
        </div>
      )}
      {preview && (
        <div
          className="table-drag-stack"
          data-external={preview.external || undefined}
          aria-hidden="true"
          style={{ left: preview.at.x, top: preview.at.y }}
        >
          {preview.cards.slice(first).map(({ object }, index) => {
            const offset = (index + first) * 5;
            const home = preview.returning?.boxes.get(object.slot);
            const style = {
              left: offset,
              top: offset,
              zIndex: -index,
              ...(home && {
                "--return-x": `${home.left - preview.at.x - offset}px`,
                "--return-y": `${home.top - preview.at.y - offset}px`,
                "--return-scale": (home.right - home.left) / preview.width,
                "--return-duration": `${preview.returning!.duration}ms`,
              }),
            } as CSSProperties;
            return (
              <div
                key={object.slot}
                className="table-held-card"
                data-ghost-slot={object.slot}
                data-returning={!!home || undefined}
                style={style}
                onAnimationEnd={() => drag.finishReturn(preview)}
              >
                <DragFace object={object} width={preview.width} />
              </div>
            );
          })}
        </div>
      )}
      {order && (
        <LibraryOrder
          client={client}
          drag={drag}
          cards={order.cards}
          space={tray}
        />
      )}
    </>
  );
}
