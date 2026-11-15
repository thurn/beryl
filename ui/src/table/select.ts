import { battlefieldRow } from "../game/battlefield";
import type { Box } from "../game/placement";
import { named } from "../stage/dom";
import type { Name } from "../stage/layout";
import { sweepBox } from "../stage/overlay";
import type { Bindings } from "./bindings";
import { sameZone, type TableClient } from "./client";
import { resolve } from "./shortcuts";

const touches = (a: Box, b: Box) =>
  a.left <= b.right &&
  a.right >= b.left &&
  a.top <= b.bottom &&
  a.bottom >= b.top;

export function registerSelection(client: TableClient, bindings: Bindings) {
  client.onCardClick(({ name, action }, { meta }) => {
    if (!meta) return false;
    if (action) {
      const selected = new Set(client.selection);
      if (!selected.delete(name)) selected.add(name);
      client.setSelection(selected);
    }
    return true;
  });
  client.onSweep(
    (from, to, done) => {
      const box = sweepBox(from, to);
      if (!done) {
        client.setSelectionBox(box);
        return;
      }
      client.setSelectionBox(undefined);
      const { surface, view } = client;
      if (!surface || !view) return;
      const selected = view.objects.flatMap(({ slot }) => {
        const node = surface.node(slot);
        return node &&
          !node.matches(
            ".card-el-hidden, .card-el-rim, .card-el-pile, [data-card-borrowed]",
          ) &&
          client.card(slot)?.action &&
          touches(box, surface.bounds(node))
          ? [slot]
          : [];
      });
      client.setSelection(selected);
    },
    () => client.setSelectionBox(undefined),
  );
  client.onKey((event) => {
    if (event.key === "Escape") {
      if (client.sweepSnapshot() || client.selection.size) {
        event.preventDefault();
        client.cancelGesture();
        client.setSelection([]);
      }
      return;
    }
    if (
      event.target instanceof Element &&
      event.target.closest("input, textarea, select, [contenteditable]")
    )
      return;
    const command = resolve(event, "table", bindings);
    if (
      command === "selection.all" ||
      command === "selection.row" ||
      command === "selection.column"
    ) {
      if (selectAt(client, command)) event.preventDefault();
    }
  });
}

export function selectAt(
  client: TableClient,
  command: "selection.all" | "selection.row" | "selection.column",
) {
  const pointed = client.surface?.pointed();
  const viewer = pointed?.closest<HTMLElement>("[data-selection-viewer]");
  if (viewer) {
    if (command === "selection.row") return false;
    const cards = [
      ...viewer.querySelectorAll<HTMLElement>("[data-viewer-slot]"),
    ];
    const anchor = pointed?.closest("[data-viewer-slot]");
    if (command === "selection.column" && !anchor) return false;
    const left = anchor?.getBoundingClientRect().left;
    const slots = cards.flatMap((card) => {
      if (
        command === "selection.column" &&
        card.getBoundingClientRect().left !== left
      )
        return [];
      const slot = Number(card.dataset.viewerSlot);
      return client.card(slot, true)?.action ? [slot] : [];
    });
    client.setSelection(slots);
    return true;
  }
  if (client.overlayOpen || command === "selection.column") return false;
  const said = pointed?.closest<HTMLElement>("[data-stage]")?.dataset.stage;
  const target = said !== undefined ? client.card(named(said)) : undefined;
  if (!target?.action || !client.view) return false;
  const battlefield = target.action.zone === "battlefield";
  const row =
    command === "selection.row" && battlefield && target.object
      ? battlefieldRow(target.object)
      : undefined;
  if (command === "selection.row" && !row) return false;

  const objects: Name[] = [];
  for (const { subject, zone } of client.view.table?.objectActions ?? []) {
    if (!sameZone(zone, target.action.zone)) continue;
    if (subject.kind === "ability") {
      objects.push(`a${subject.id}`);
    } else if (subject.kind === "object") {
      const object = client.card(subject.slot)?.object;
      if (battlefield && object?.controller !== target.object?.controller)
        continue;
      if (row && (!object || battlefieldRow(object) !== row)) continue;
      objects.push(subject.slot);
    }
  }
  client.setSelection(objects);
  return true;
}
