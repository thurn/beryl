// The stage's nodes on the page: one element an object or an ability on the stack, in the
// card layer, for as long as the stage holds it. Only the stage writes an element's
// transform, opacity and depth, with what its model computed; React draws the face inside
// (Faces.tsx) and never touches those. The elements keep the names the old board gave its
// own (`card-el` and its looks, `stack-ability`), which the skin's styles and the screen
// audits (ui/scripts/capture.mjs) go by.
import type { Name, Resting } from "./layout.ts";
import { transformOf, type Pose } from "./pose.ts";
import type { Surface } from "./stage.ts";

export class DomSurface implements Surface {
  readonly #layer: HTMLElement;
  readonly #nodes = new Map<Name, HTMLElement>();

  constructor(layer: HTMLElement) {
    this.#layer = layer;
  }

  /** The node of what the stage names `name`, which its face is drawn into. */
  node(name: Name): HTMLElement | undefined {
    return this.#nodes.get(name);
  }

  make(name: Name) {
    const node = document.createElement("div");
    // What it is, as the stage names it (`stage.pose`): a slot, or `a` and an ability's number.
    node.dataset.stage = String(name);
    if (typeof name === "number") node.dataset.slot = String(name);
    this.#nodes.set(name, node);
    this.#layer.append(node);
  }

  remove(name: Name) {
    this.#nodes.get(name)?.remove();
    this.#nodes.delete(name);
  }

  dress(name: Name, { object, dress }: Resting) {
    const node = this.#nodes.get(name);
    if (!node) return;
    node.dataset.zone = dress.zone;
    if (!object) {
      node.className = "hud stack-ability stage-node";
      return;
    }
    node.className = [
      "card-el",
      "stage-node",
      `card-el-${dress.look}`,
      `card-el-${object.controller}`,
      dress.rim && "card-el-rim",
    ]
      .filter(Boolean)
      .join(" ");
    if (object.name && !dress.back) node.dataset.card = object.name;
    else delete node.dataset.card;
  }

  draw(name: Name, pose: Pose, frame: number) {
    const style = this.#nodes.get(name)?.style;
    if (!style) return;
    style.transform = transformOf(pose, frame);
    style.opacity = String(pose.opacity);
    style.zIndex = String(Math.round(pose.depth));
  }
}
