import type { MouseEvent } from "react";
import type { ObjectView, Side, Step } from "../ipc";
import type { PointerKeys } from "../stage/pointer";
import type { TableClient, TableTarget } from "./client";

type ClickEvent = MouseEvent<HTMLElement>;

const keys = (event: ClickEvent): PointerKeys => ({
  meta: /Mac/.test(navigator.platform) ? event.metaKey : event.ctrlKey,
  shift: event.shiftKey,
  alt: event.altKey,
});

export function pickPile(
  client: TableClient | undefined,
  pile: { side: Side; zone: string },
  event: ClickEvent,
  open: () => void,
) {
  const { side, zone } = pile;
  if (
    client &&
    (zone === "library" || zone === "graveyard" || zone === "exile")
  ) {
    const target: TableTarget = { kind: zone, side, open };
    if (event.type === "contextmenu") secondaryControl(client, target, event);
    else client.click(target, keys(event), event.currentTarget);
  } else open();
}

export function pickViewerCard(
  client: TableClient,
  object: ObjectView,
  event: ClickEvent,
) {
  const target = client.card(object.slot, true);
  if (target?.object !== object) return;
  if (event.type === "contextmenu") secondaryControl(client, target, event);
  else client.click(target, keys(event), event.currentTarget);
}

export function counterControls(client: TableClient | undefined, side: Side) {
  if (!client) return undefined;
  return (counter: string, event: ClickEvent) => {
    const target = { kind: "counter" as const, side, counter };
    if (event.type === "contextmenu") {
      event.preventDefault();
      client.secondary(
        target,
        { x: event.clientX, y: event.clientY },
        event.currentTarget,
      );
    } else client.click(target, keys(event), event.currentTarget);
  };
}

export function pickStep(
  client: TableClient,
  side: Side,
  step: Step,
  event: ClickEvent,
) {
  client.click({ kind: "step", side, step }, keys(event), event.currentTarget);
}

function secondaryControl(
  client: TableClient,
  target: TableTarget,
  event: ClickEvent,
) {
  event.preventDefault();
  event.stopPropagation();
  client.secondary(
    target,
    { x: event.clientX, y: event.clientY },
    event.currentTarget,
  );
}
