import { setBugReportTableOpen } from "../components/bugReportKeys";
import { isNativeApp, tableKeys } from "../ipc";
import type { TableClient, TableTarget } from "./client";
import { claims } from "./keys";
import type { TableCommands } from "./TableMenu";
import { resolve } from "./shortcuts";

export function pointedTarget(client: TableClient): TableTarget | undefined {
  const pointed = client.surface?.pointed();
  const viewerCard = pointed?.closest<HTMLElement>("[data-viewer-slot]");
  if (viewerCard)
    return client.card(Number(viewerCard.dataset.viewerSlot), true);

  const pile = pointed?.closest<HTMLButtonElement>(
    "button[data-pile][data-side]",
  );
  const zone = pile?.dataset.pile;
  const side = pile?.dataset.side;
  if (
    (zone === "library" || zone === "graveyard" || zone === "exile") &&
    (side === "you" || side === "opponent")
  )
    return { kind: zone, side, open: () => pile?.click() };

  const counter = pointed?.closest<HTMLElement>("[data-counter]");
  const owner = counter?.closest<HTMLElement>("[data-side]")?.dataset.side;
  if (counter?.dataset.counter && (owner === "you" || owner === "opponent"))
    return { kind: "counter", side: owner, counter: counter.dataset.counter };

  const life = pointed?.closest<HTMLElement>("[data-life]")?.dataset.life;
  if (life === "you" || life === "opponent")
    return { kind: "life", side: life };

  return client.hovered();
}

export function currentTarget(client: TableClient) {
  const [first] = client.selection;
  return first === undefined
    ? pointedTarget(client)
    : (client.card(first) ?? client.card(first, true));
}

let nativeKeys = Promise.resolve();

export function listenTableKeys(commands: TableCommands) {
  const { client, bindings } = commands;
  const setNativeKeys = (on: boolean) => {
    if (isNativeApp())
      nativeKeys = nativeKeys
        .then(async () => {
          await tableKeys(on);
        })
        .catch(commands.reportError);
  };
  setBugReportTableOpen(true, true);
  setNativeKeys(true);
  const keydown = (event: KeyboardEvent) => {
    if (
      event.defaultPrevented ||
      event.isComposing ||
      document.documentElement.classList.contains("page-frozen")
    )
      return;
    let id = resolve(event, "table", bindings);
    if (!id && event.shiftKey) {
      const unshifted = resolve(
        {
          code: event.code,
          metaKey: event.metaKey,
          ctrlKey: event.ctrlKey,
          altKey: event.altKey,
          shiftKey: false,
          isComposing: event.isComposing,
        },
        "table",
        bindings,
      );
      if (unshifted?.startsWith("token.")) id = unshifted;
    }
    if (
      id !== "viewer.hide_revealed" &&
      event.target instanceof Element &&
      event.target.closest("input, textarea, select, [contenteditable], dialog")
    )
      return;
    if (
      event.code === "Escape" &&
      (client.overlayOpen ||
        client.arrows?.dragging ||
        client.holdingCard ||
        client.selection.size ||
        client.sweepSnapshot())
    )
      return;
    if (id || claims(event, bindings.platform)) event.preventDefault();
    if (id && !event.repeat && !commands.overlayOpen) {
      const target =
        id.startsWith("life.") ||
        id === "selection.row" ||
        id === "selection.column"
          ? pointedTarget(client)
          : currentTarget(client);
      void commands
        .shortcut(id, target, event.shiftKey)
        .catch(commands.reportError);
    }
  };
  window.addEventListener("keydown", keydown, true);
  window.addEventListener("keydown", client.key);
  return () => {
    window.removeEventListener("keydown", keydown, true);
    window.removeEventListener("keydown", client.key);
    setBugReportTableOpen(false);
    setNativeKeys(false);
  };
}
