// The copy that draws a raised hand card over the rest of the hand (dom.ts `raise`; UI
// Conventions, "Hover inspection"). The raised node keeps its depth in the fan and its face,
// made invisible, so the pointer finds the cards in the fan's resting order: each card's
// visible strip is that card's, and a neighbor's edge moves the hover to the neighbor as
// Arena's does. Over the hand, this copy of the node, which the pointer passes through, shows
// the card raised.
//
// The copy eases up out of the fan as the pointer comes onto the card, and back down into it
// as the pointer leaves, as Arena's hand does: up in a tenth of a second, down in about a
// sixth, each fast at first and slowing as it arrives. How far up it is (`--raise-k`, 0 at
// rest in the fan and 1 raised) runs on the browser's frames, as a held card's heft does
// (heft.ts), because it follows the pointer and not the stage's clock.

/** How long a card takes to rise all the way out of the fan, and to drop all the way back
 * into it, in ms. Arena's hand, filmed at 30 frames a second (jD7VqaT8VC8 45:26.77 and
 * 45:27.00, SswBO8aOOgc 21:01.95 and 21:02.75, IEXcjBn3BXk 18:06.62), rises in 70 to 130 ms
 * and drops in 130 to 200 ms, about half the way in the first frame of each. */
export const RISE_MS = 100;
export const DROP_MS = 160;

/** A move of a copy toward `to` (1 raised, 0 at rest), begun at time `since` from `from`. */
export interface Easing {
  from: number;
  to: number;
  since: number;
}

/** How far up a copy easing by `easing` is at time `now`, from 0 to 1: fast at first and
 * slowing as it arrives (a cubic ease out), over the share of a whole rise or drop it has
 * left to go. */
export function raisedAt({ from, to, since }: Easing, now: number): number {
  const length = Math.abs(to - from) * (to > from ? RISE_MS : DROP_MS);
  if (length <= 0) return to;
  const t = Math.min(1, Math.max(0, (now - since) / length));
  return from + (to - from) * (1 - (1 - t) ** 3);
}

/** One copy over the hand: of `node`, easing by `easing`. A copy that drops back into the
 * fan no longer follows its node's face, pose or raise, which its node forgets (dom.ts
 * `raise`); `transform` is the node's transform as it began to drop, and a node that moves
 * from there is no longer the card it drops onto. */
interface Copy {
  node: HTMLElement;
  element: HTMLElement;
  easing: Easing;
  watch: MutationObserver | undefined;
  transform: string;
}

/** The copy of the node `raise` names, kept as that node is (its pose each time the stage
 * draws it, and its face each time React draws it, as a change to the node's element is
 * heard), and the copies of nodes raised before it while they drop back into the fan. */
export class RaisedCopy {
  readonly #layer: HTMLElement;
  #raised: Copy | undefined;
  readonly #dropping: Copy[] = [];
  #frame = 0;

  constructor(layer: HTMLElement) {
    this.#layer = layer;
  }

  /** Draws `node` raised over the hand, or none with no node. The copy of the node raised
   * until now drops back into the fan, or, `atOnce`, is gone in this frame. */
  show(node: HTMLElement | undefined, atOnce = false) {
    const now = performance.now();
    const raised = this.#raised;
    if (raised?.node === node) return;
    if (raised) {
      this.#raised = undefined;
      raised.watch?.disconnect();
      raised.watch = undefined;
      if (atOnce) {
        raised.element.remove();
      } else {
        raised.easing = { from: this.#at(raised, now), to: 0, since: now };
        raised.transform = raised.node.style.transform;
        this.#dropping.push(raised);
      }
    }
    if (!node) {
      this.#tick();
      return;
    }
    // A card dropping back into the fan that the pointer comes back onto rises again from
    // where it is.
    const back = this.#dropping.findIndex((c) => c.node === node);
    const copy =
      back >= 0 ? this.#dropping.splice(back, 1)[0]! : this.#make(node, now);
    copy.easing = { from: this.#at(copy, now), to: 1, since: now };
    // Over any card still dropping.
    this.#layer.append(copy.element);
    this.#raised = copy;
    this.#face(copy);
    this.#posed(copy);
    // React draws a face into the node, and the stage dresses and marks it, while it is raised.
    copy.watch = new MutationObserver((changes) => {
      if (changes.some((c) => c.target !== node || c.type !== "attributes")) {
        this.#face(copy);
      }
      this.#posed(copy);
    });
    copy.watch.observe(node, {
      subtree: true,
      childList: true,
      attributes: true,
      characterData: true,
    });
    this.#tick();
  }

  #make(node: HTMLElement, now: number): Copy {
    const element = document.createElement("div");
    element.setAttribute("aria-hidden", "true");
    return {
      node,
      element,
      easing: { from: 0, to: 0, since: now },
      watch: undefined,
      transform: "",
    };
  }

  /** How far up `copy` is at time `now`: where its easing has it, or where it is going with
   * reduced motion. */
  #at(copy: Copy, now: number): number {
    return reducedMotion() ? copy.easing.to : raisedAt(copy.easing, now);
  }

  /** Draws every copy as far up as it is now, drops the copies that are back in the fan or
   * whose node has gone or moved, and asks for another frame while any copy is on its way. */
  #tick() {
    if (this.#frame) cancelAnimationFrame(this.#frame);
    this.#frame = 0;
    const now = performance.now();
    let moving = false;
    const raised = this.#raised;
    if (raised) {
      const k = this.#at(raised, now);
      raised.element.style.setProperty("--raise-k", String(k));
      moving ||= k < 1;
    }
    for (let i = this.#dropping.length - 1; i >= 0; i--) {
      const copy = this.#dropping[i]!;
      const k = this.#at(copy, now);
      const { node } = copy;
      if (
        k <= 0 ||
        !node.isConnected ||
        node.style.transform !== copy.transform
      ) {
        copy.element.remove();
        this.#dropping.splice(i, 1);
        continue;
      }
      copy.element.style.setProperty("--raise-k", String(k));
      moving = true;
    }
    if (moving) {
      this.#frame = requestAnimationFrame(() => this.#tick());
    }
  }

  /** Follows the raised node's pose and dress. */
  #posed(copy: Copy) {
    const { node, element } = copy;
    element.className = `${node.className} stage-raised-copy`;
    element.style.cssText = node.style.cssText;
    element.style.setProperty(
      "--raise-k",
      String(this.#at(copy, performance.now())),
    );
    // What the screen audits know it as: a copy of a card, not a second one.
    element.dataset.ghostSlot = node.dataset.slot ?? node.dataset.stage;
    if (node.dataset.zone) element.dataset.zone = node.dataset.zone;
  }

  #face({ node, element }: Copy) {
    element.replaceChildren(
      ...[...node.children].map((child) => child.cloneNode(true)),
    );
  }
}

/** Whether the player asked for reduced motion: then a card is raised and put back at once. */
const reducedMotion = () =>
  typeof matchMedia === "function" &&
  matchMedia("(prefers-reduced-motion: reduce)").matches;
