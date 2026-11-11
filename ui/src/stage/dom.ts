// The stage's nodes on the page: one element an object, in the card layer, for the object's
// life. Only the stage writes an element's transform, opacity and depth, with what its model
// computed; React draws the face inside (Faces.tsx) and never touches those. The elements keep
// the names the old card layer gave its own (`card-el` and its looks), which the skin's styles
// and the screen audits (ui/scripts/capture.mjs) go by.
import type { Resting } from "./layout.ts";
import { transformOf, type Pose } from "./pose.ts";
import type { Surface } from "./stage.ts";

export class DomSurface implements Surface {
  readonly #layer: HTMLElement;
  readonly #nodes = new Map<number, HTMLElement>();

  constructor(layer: HTMLElement) {
    this.#layer = layer;
  }

  /** The node of the object in `slot`, which its face is drawn into. */
  node(slot: number): HTMLElement | undefined {
    return this.#nodes.get(slot);
  }

  make(slot: number) {
    const node = document.createElement("div");
    node.dataset.slot = String(slot);
    this.#nodes.set(slot, node);
    this.#layer.append(node);
  }

  remove(slot: number) {
    this.#nodes.get(slot)?.remove();
    this.#nodes.delete(slot);
  }

  dress(slot: number, { object, dress }: Resting) {
    const node = this.#nodes.get(slot);
    if (!node) return;
    node.className = [
      "card-el",
      "stage-node",
      `card-el-${dress.look}`,
      `card-el-${object.controller}`,
      dress.rim && "card-el-rim",
    ]
      .filter(Boolean)
      .join(" ");
    node.dataset.zone = dress.zone;
    if (object.name && !dress.back) node.dataset.card = object.name;
    else delete node.dataset.card;
  }

  draw(slot: number, pose: Pose, frame: number) {
    const style = this.#nodes.get(slot)?.style;
    if (!style) return;
    style.transform = transformOf(pose, frame);
    style.opacity = String(pose.opacity);
    style.zIndex = String(Math.round(pose.depth));
  }
}
