// The stage's nodes on the page: one element an object or an ability on the stack, in the
// card layer, for as long as the stage holds it. Only the stage writes an element's
// transform, opacity and depth, with what its model computed; React draws the face inside
// (Faces.tsx) and never touches those. The elements keep the names the old board gave its
// own (`card-el` and its looks, `stack-ability`), which the skin's styles and the screen
// audits (ui/scripts/capture.mjs) go by.
import type { Name, Point, Resting } from "./layout.ts";
import type { Pointer, Under } from "./pointer.ts";
import { transformOf, turnOf, type Pose } from "./pose.ts";
import type { Surface } from "./stage.ts";

/** The name a node's `data-stage` says. */
const named = (said: string): Name =>
  said.startsWith("a") ? (said as Name) : Number(said);

export class DomSurface implements Surface {
  readonly #layer: HTMLElement;
  readonly #nodes = new Map<Name, HTMLElement>();
  /** Where the browser's pointer was last, in the window. */
  #pointer: { x: number; y: number } | undefined;
  #repoint: (() => void) | undefined;

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

  /** Tells `pointer` what the browser's pointer does on the nodes, by their names and in stage
   * pixels, until the returned function is called: the one place the page's pointer events
   * become the stage's (pointer.ts). A touch has no hover: it presses, and is let go. */
  point(pointer: Pointer) {
    const layer = this.#layer;
    const nodeOf = (target: EventTarget | null) => {
      const node =
        target instanceof Element
          ? target.closest<HTMLElement>("[data-stage]")
          : null;
      return node && layer.contains(node) ? node : undefined;
    };
    const nameOf = (target: EventTarget | null) => {
      const said = nodeOf(target)?.dataset.stage;
      return said === undefined ? undefined : named(said);
    };
    // The layer is as large as the stage and scaled with it (game/Stage.tsx).
    const at = (e: PointerEvent): Point => {
      const box = layer.getBoundingClientRect();
      const scale = box.width / layer.offsetWidth || 1;
      return {
        x: (e.clientX - box.left) / scale,
        y: (e.clientY - box.top) / scale,
      };
    };
    const under = (
      e: PointerEvent,
      held: Element | undefined,
    ): Under | undefined => {
      for (const hit of document.elementsFromPoint(e.clientX, e.clientY)) {
        if (held?.contains(hit)) continue;
        const said = nodeOf(hit)?.dataset.stage;
        if (said !== undefined) return { name: named(said) };
        const life = hit.closest<HTMLElement>("[data-life]")?.dataset.life;
        if (life === "you" || life === "opponent") return { player: life };
      }
      return undefined;
    };
    const on = (
      kind: `pointer${"over" | "out" | "down" | "move" | "up" | "cancel"}`,
      heard: (e: PointerEvent) => void,
    ) => {
      layer.addEventListener(kind, heard);
      return () => layer.removeEventListener(kind, heard);
    };
    // A finger is nowhere once it is lifted: only a pointer that hovers is followed.
    const where = (e: PointerEvent) => {
      if (e.pointerType !== "touch")
        this.#pointer = { x: e.clientX, y: e.clientY };
    };
    window.addEventListener("pointermove", where, { passive: true });
    this.#repoint = () => {
      const here = this.#pointer;
      if (here && !pointer.held)
        pointer.over(nameOf(document.elementFromPoint(here.x, here.y)));
    };
    const stops = [
      () => window.removeEventListener("pointermove", where),
      on("pointerover", (e) => {
        if (e.pointerType !== "touch") pointer.over(nameOf(e.target));
      }),
      on("pointerout", (e) => {
        if (e.pointerType !== "touch") pointer.over(nameOf(e.relatedTarget));
      }),
      on("pointerdown", (e) => {
        const node = nodeOf(e.target);
        if (!node || e.button !== 0) return;
        // The press keeps its node however far the pointer goes.
        node.setPointerCapture(e.pointerId);
        pointer.press(named(node.dataset.stage!), at(e));
      }),
      on("pointermove", (e) => pointer.move(at(e))),
      on("pointerup", (e) =>
        pointer.release(at(e), under(e, nodeOf(e.target))),
      ),
      on("pointercancel", () => pointer.cancel()),
    ];
    return () => {
      this.#repoint = undefined;
      for (const stop of stops) stop();
    };
  }

  /** Tells the pointer afresh what it is on: a node that moved from under a still pointer, or
   * came under it, sends no event of its own. */
  repoint() {
    this.#repoint?.();
  }

  draw(name: Name, pose: Pose, frame: number) {
    const node = this.#nodes.get(name);
    const style = node?.style;
    if (!node || !style) return;
    style.transform = transformOf(pose, frame);
    style.opacity = String(pose.opacity);
    style.zIndex = String(Math.round(pose.depth));
    // The card inside is turned by the stage's rule for it (stage.css), only while it turns.
    const turn = turnOf(pose);
    style.setProperty("--turn", turn);
    node.toggleAttribute("data-turning", turn !== "");
  }
}
