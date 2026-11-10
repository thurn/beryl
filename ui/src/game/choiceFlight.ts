// A choice hands its visible cards back to the table when the answer moves them to a zone.
// Snapshot at confirmation, before the chooser unmounts; consume only on a zone change.
import type { PlayerView, ZoneName } from "../generated/protocol";

interface Flight {
  zone: ZoneName;
  x: number;
  y: number;
  width: number;
  at: number;
  element: HTMLElement;
}
const flights = new Map<number, Flight>();

export function rememberChoiceCards(view: PlayerView) {
  flights.clear();
  const stage = document.querySelector<HTMLElement>(".stage");
  if (!stage) return;
  const origin = stage.getBoundingClientRect();
  const scale = origin.width / stage.offsetWidth;
  for (const element of document.querySelectorAll<HTMLElement>(
    ".choice-card[data-slot]",
  )) {
    const slot = Number(element.dataset.slot);
    const object = view.objects.find((o) => o.slot === slot);
    if (!object) continue;
    const box = element.getBoundingClientRect();
    flights.set(slot, {
      zone: object.zone,
      x: (box.left - origin.left) / scale,
      y: (box.top - origin.top) / scale,
      width: box.width / scale,
      at: performance.now(),
      element,
    });
  }
}

export function takeChoiceFlight(slot: number, zone: ZoneName) {
  const flight = flights.get(slot);
  if (!flight || flight.zone === zone) return undefined;
  flights.delete(slot);
  if (performance.now() - flight.at > 15000) return undefined;
  // During the commit that changes the view the old chooser may remain for one render.
  // Its representation goes away before the table representation starts moving.
  flight.element.style.visibility = "hidden";
  return flight;
}
