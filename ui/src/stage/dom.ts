// The stage's nodes on the page: one element an object or an ability on the stack, in the
// card layer, for as long as the stage holds it. Only the stage writes an element's
// transform, opacity and depth, with what its model computed; React draws the face inside
// (Faces.tsx) and never touches those. An effect on a node (effects.ts) is an element of the
// stage's own inside it, under or over the face, and how far through it is a number the stage
// writes on the node each tick: its look is a rule of stage.css read from that number, never a
// CSS animation. An effect on a player is drawn the same way in the element of their life
// total, which the chrome around the table draws. A mark (spans.ts) is drawn as an effect
// is, an element inside what it is on with its look in stage.css, for as long as its span is
// open. An arrow or line is one path in a drawing of the layer's own, from the step that
// begins it to the step that ends it: over everything at rest and under what flies, or for a
// line of combat on the table, under the cards. The stage writes its shape in stage pixels
// each tick. Where the browser has WebGL the lines and the marks of combat are painted as
// light instead (light.ts, glow.ts), on two canvases of the layer's own, at the depths of
// the two drawings; the paths stay on the page as the record of what is drawn, which the
// checks read, and are not painted, and the layer says which of the two are painted
// (`data-light`) for the styles that hide them and the marks a card's face would wear. A streak (streaks.ts) is a path of the same drawing for as long as its step. How
// far through an announcement is (shown.ts) is a number on the layer's parent, which the
// chrome's banners are drawn in. The elements keep the names the old board gave its
// own (`card-el` and its looks, `stack-ability`), which the skin's styles and the screen
// audits (ui/scripts/capture.mjs) go by.
import type { Arrow, Mark } from "../generated/protocol";
import type { Drawn, On } from "./effects.ts";
import { Glow } from "./glow.ts";
import type { Name, Point, Resting } from "./layout.ts";
import { dark, type Lit } from "./light.ts";
import type { Shape } from "./lines.ts";
import type { StreakShape } from "./streaks.ts";
import type { Pointer, Under } from "./pointer.ts";
import { tippedOf, transformOf, turnOf, type Pose } from "./pose.ts";
import type { Surface } from "./stage.ts";

/** The name a node's `data-stage` says. */
const named = (said: string): Name =>
  /^-?\d/.test(said) ? Number(said) : (said as Name);

export class DomSurface implements Surface {
  readonly #layer: HTMLElement;
  readonly #nodes = new Map<Name, HTMLElement>();
  /** The drawings the lines are paths of, each made with its first: one over everything at
   * rest, for an arrow at a target and the line to a source, and one under the cards, on
   * the table, for the lines of combat. */
  readonly #drawings: { over?: SVGSVGElement; under?: SVGSVGElement } = {};
  readonly #lines = new Map<string, SVGPathElement>();
  /** What paints each layer of light: none yet, or `null` where the browser has none to
   * give. */
  readonly #glow: { under?: Glow | null; over?: Glow | null } = {};
  #lit: Lit | undefined;
  /** What a prompt pulls toward the pointer (Aim.tsx), as it is at a stage time: no part of
   * the stage's scene. */
  #aim: ((now: number) => Lit) | undefined;
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
    // What an effect on it is sized by (stage.css).
    node.style.setProperty("--frame", `${dress.frame}px`);
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

  /** The element of what the stage names `on`: its node, or a player's life total, which is
   * the chrome's (`data-life`), beside the card layer. */
  #on(on: On) {
    return on === "you" || on === "opponent"
      ? this.#layer.ownerDocument.querySelector<HTMLElement>(
          `[data-life="${on}"]`,
        )
      : this.#nodes.get(on);
  }

  line(key: string, arrow?: Arrow, shape?: Pick<Shape, "d"> | null) {
    let path = this.#lines.get(key);
    if (arrow === undefined) {
      path?.remove();
      this.#lines.delete(key);
      return;
    }
    if (!path) {
      const document = this.#layer.ownerDocument;
      const svg = "http://www.w3.org/2000/svg";
      const where = arrow === "attack" || arrow === "block" ? "under" : "over";
      let drawing = this.#drawings[where];
      if (!drawing?.isConnected) {
        drawing = document.createElementNS(svg, "svg");
        drawing.setAttribute("class", `stage-lines stage-lines-${where}`);
        drawing.setAttribute("aria-hidden", "true");
        this.#layer.append(drawing);
        this.#drawings[where] = drawing;
      }
      path = document.createElementNS(svg, "path");
      path.setAttribute("class", `stage-line stage-line-${arrow}`);
      path.dataset.line = key;
      this.#lines.set(key, path);
      drawing.append(path);
    }
    const d = shape?.d ?? "";
    if (path.getAttribute("d") !== d) path.setAttribute("d", d);
  }

  /** Paints all that is `lit`; with none, the stage no longer draws here, and the painters
   * go. */
  light(lit?: Lit) {
    this.#lit = lit;
    if (lit) return this.#paint();
    for (const where of ["under", "over"] as const) {
      this.#glow[where]?.gone();
      delete this.#glow[where];
    }
    delete this.#layer.dataset.light;
  }

  /** Draws what the pointer pulls as `trace` has it at each stage time, from now and at once:
   * it follows the pointer, not the tick. With none, it is gone. */
  aim(trace?: (now: number) => Lit) {
    this.#aim = trace;
    this.#paint();
  }

  #paint() {
    // With reduced motion asked for, light stands still: nothing runs along it or flies off.
    const still = this.#layer.ownerDocument.defaultView?.matchMedia?.(
      "(prefers-reduced-motion: reduce)",
    ).matches;
    // What the pointer pulls is of the same stage time as the scene it is drawn with.
    const aim = this.#aim?.(this.#lit?.over.now ?? 0);
    for (const where of ["under", "over"] as const) {
      const [held, pulled] = [this.#lit?.[where] ?? dark(0), aim?.[where]];
      const drawn = {
        now: held.now,
        ribbons: [...held.ribbons, ...(pulled?.ribbons ?? [])],
        glints: [...held.glints, ...(pulled?.glints ?? [])],
        dots: [...held.dots, ...(pulled?.dots ?? [])],
      };
      const scene = still ? { ...drawn, now: 0, dots: [] } : drawn;
      const { ribbons } = scene;
      const lit = ribbons.length + scene.glints.length + scene.dots.length > 0;
      // A painter is made with the first light there is to paint in its layer.
      if (this.#glow[where] === undefined && lit) {
        this.#glow[where] = Glow.on(this.#layer, where) ?? null;
      }
      // A context the browser took back paints nothing: the paths are drawn again.
      if (this.#glow[where]?.lost) {
        this.#glow[where].gone();
        this.#glow[where] = null;
      }
      this.#glow[where]?.paint(scene);
    }
    // Which layers are painted, for the styles that hide what the paint stands for.
    const painted = (["under", "over"] as const).filter(
      (where) => this.#glow[where],
    );
    if (painted.length === 0) delete this.#layer.dataset.light;
    else this.#layer.dataset.light = painted.join(" ");
  }

  streak(key: string, look?: string, shape?: StreakShape | null) {
    if (look === undefined) return this.line(key);
    const made = !this.#lines.has(key);
    // Over everything at rest, as an arrow is.
    this.line(key, "source", shape);
    const path = this.#lines.get(key)!;
    if (made) {
      path.setAttribute("class", `stage-streak stage-streak-${look}`);
      delete path.dataset.line;
      path.dataset.streak = key;
    }
    // Its halo is as wide as its head, whatever the layout's size (stage.css).
    if (shape) path.style.setProperty("--wide", `${shape.wide}px`);
  }

  notice(kind: string, progress?: number) {
    // The chrome draws its announcements beside the card layer: both read the number here.
    const style = this.#layer.parentElement?.style;
    if (progress === undefined) style?.removeProperty(`--notice-${kind}`);
    else style?.setProperty(`--notice-${kind}`, String(progress));
  }

  mark(on: On, mark: Mark, shown?: number) {
    const node = this.#on(on);
    if (!node) return;
    const drawn = node.querySelector<HTMLElement>(
      `:scope > .stage-mark-${mark}`,
    );
    if (shown === undefined) {
      drawn?.remove();
      return;
    }
    const made = drawn ?? document.createElement("div");
    if (!drawn) {
      made.className = `stage-mark stage-mark-${mark}`;
      made.setAttribute("aria-hidden", "true");
      node.append(made);
    }
    // How far it has come to its look, which is drawn from that number alone.
    const through = String(shown);
    if (made.style.getPropertyValue("--shown") !== through) {
      made.style.setProperty("--shown", through);
    }
  }

  effect(on: On, { name, layer }: Omit<Drawn, "progress">, progress?: number) {
    const node = this.#on(on);
    if (!node) return;
    const drawn = node.querySelector(`:scope > .stage-effect-${name}`);
    if (progress === undefined) {
      drawn?.remove();
      node.style.removeProperty(`--${name}`);
      return;
    }
    if (!drawn) {
      const made = document.createElement("div");
      made.className = `stage-effect stage-effect-${name} stage-effect-${layer}`;
      made.setAttribute("aria-hidden", "true");
      node.append(made);
    }
    // How far through it is, which its look is drawn from.
    node.style.setProperty(`--${name}`, String(progress));
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
    // And tipped as it turns over on its way, only while it does.
    const tipped = tippedOf(pose);
    style.setProperty("--tipped", tipped);
    node.toggleAttribute("data-turning-over", tipped !== "");
  }
}
