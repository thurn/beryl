import { useLayoutEffect, useRef, useState, type PointerEvent } from "react";
import { Button } from "../components/Controls";
import { DRAG_THRESHOLD } from "../stage/pointer";
import { Library } from "../stage/Tray.tsx";
import { landing, laidOut, type TraySpace } from "../stage/tray";
import type { TableClient } from "./client";
import type { TableDrag } from "./drag";
import type { DragCard } from "./dropActions";
import { DragFace } from "./DragFace";

export function LibraryOrder({
  client,
  drag,
  cards,
  space,
}: {
  client: TableClient;
  drag: TableDrag;
  cards: DragCard[];
  space: TraySpace;
}) {
  const [order, setOrder] = useState(cards);
  const [carrying, setCarrying] = useState<{
    slot: number;
    dy: number;
    dx: number;
  }>();
  const grip = useRef<{ x: number; y: number }>(undefined);
  const root = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => client.pushOpen(drag.cancelOrder), [client, drag]);
  const tray = {
    putBack: true as const,
    piles: [{ cards: order.map(({ object }) => object.slot) }, { cards: [] }],
  };
  const laid = laidOut(tray, space);
  const move = (from: number, to: number) => {
    const item = order[from];
    if (!item || from === to || to < 0 || to >= order.length) return;
    const next = order.filter((_, index) => index !== from);
    next.splice(to, 0, item);
    setOrder(next);
  };
  const displacement = (e: PointerEvent) => {
    const element = root.current!;
    const scale =
      element.getBoundingClientRect().width / element.offsetWidth || 1;
    return {
      dx: (e.clientX - grip.current!.x) / scale,
      dy: (e.clientY - grip.current!.y) / scale,
    };
  };
  return (
    <div
      ref={root}
      className="table-library-order"
      role="dialog"
      aria-modal="true"
      aria-label="Order Cards"
    >
      {laid.library && <Library box={laid.library} />}
      {order.map(({ object }, index) => {
        const place = laid.cards.get(object.slot)!;
        const held = carrying?.slot === object.slot ? carrying : undefined;
        return (
          <button
            key={object.slot}
            className="table-order-card"
            type="button"
            data-order-slot={object.slot}
            data-ghost-slot={object.slot}
            aria-label={`${object.name ?? "Card back"}, ${index + 1} from top`}
            style={{
              left: place.x + (held?.dx ?? 0),
              top: place.y + (held?.dy ?? 0),
              zIndex: held ? order.length + 1 : order.length - index,
            }}
            onPointerDown={(e) => {
              if (e.button !== 0) return;
              e.currentTarget.setPointerCapture(e.pointerId);
              grip.current = { x: e.clientX, y: e.clientY };
              setCarrying({ slot: object.slot, dx: 0, dy: 0 });
            }}
            onPointerMove={(e) => {
              if (grip.current && held)
                setCarrying({ slot: object.slot, ...displacement(e) });
            }}
            onPointerUp={(e) => {
              if (!grip.current || !held) return;
              const { dx, dy } = displacement(e);
              grip.current = undefined;
              setCarrying(undefined);
              if (Math.hypot(dx, dy) < DRAG_THRESHOLD) {
                move(index, 0);
                return;
              }
              const pose = { ...place, y: place.y + dy };
              const point = {
                x: place.x + place.width / 2 + dx,
                y: pose.y + place.width / space.card.aspect / 2,
              };
              const target = landing(tray, space, point, {
                slot: object.slot,
                pose,
              });
              if (target?.pile === 0)
                move(index, target.at > index ? target.at - 1 : target.at);
            }}
            onPointerCancel={() => {
              grip.current = undefined;
              setCarrying(undefined);
            }}
            onKeyDown={(e) => {
              if (e.key !== "ArrowUp" && e.key !== "ArrowDown") return;
              e.preventDefault();
              move(index, index + (e.key === "ArrowUp" ? -1 : 1));
            }}
          >
            <DragFace object={object} width={place.width} />
          </button>
        );
      })}
      <div className="table-order-controls">
        <h2>Order Cards</h2>
        <Button onClick={() => drag.confirmOrder(order)}>Done</Button>
        <Button variant="secondary" onClick={drag.cancelOrder}>
          Cancel
        </Button>
      </div>
    </div>
  );
}
