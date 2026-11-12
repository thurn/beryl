// The copy that draws a raised hand card over the rest of the hand (dom.ts `raise`; UI
// Conventions, "Hover inspection"). The raised node keeps its depth in the fan and its face,
// made invisible, so the pointer finds the cards in the fan's resting order: each card's
// visible strip is that card's, and a neighbor's edge moves the hover to the neighbor as
// Arena's does. Over the hand, this copy of the node, which the pointer passes through, shows
// the card raised.

/** One copy at a time, of the node `raise` names, kept as that node is: its pose each time
 * the stage draws it, and its face each time React draws it, as a change to the node's
 * element is heard. */
export class RaisedCopy {
  readonly #layer: HTMLElement;
  #node: HTMLElement | undefined;
  #copy: HTMLElement | undefined;
  #watch: MutationObserver | undefined;

  constructor(layer: HTMLElement) {
    this.#layer = layer;
  }

  /** Draws `node` raised over the hand, or none with no node. */
  show(node: HTMLElement | undefined) {
    if (node === this.#node) return;
    this.#watch?.disconnect();
    this.#copy?.remove();
    this.#node = node;
    this.#copy = undefined;
    this.#watch = undefined;
    if (!node) return;
    const copy = document.createElement("div");
    copy.setAttribute("aria-hidden", "true");
    this.#copy = copy;
    this.#layer.append(copy);
    this.#face();
    this.#posed();
    // React draws a face into the node, and the stage dresses and marks it, while it is raised.
    this.#watch = new MutationObserver((changes) => {
      if (changes.some((c) => c.target !== node || c.type !== "attributes")) {
        this.#face();
      }
      this.#posed();
    });
    this.#watch.observe(node, {
      subtree: true,
      childList: true,
      attributes: true,
      characterData: true,
    });
  }

  /** Follows the raised node's pose and dress. */
  #posed() {
    const node = this.#node;
    const copy = this.#copy;
    if (!node || !copy) return;
    copy.className = `${node.className} stage-raised-copy`;
    copy.style.cssText = node.style.cssText;
    // What the screen audits know it as: a copy of a card, not a second one.
    copy.dataset.ghostSlot = node.dataset.slot ?? node.dataset.stage;
    if (node.dataset.zone) copy.dataset.zone = node.dataset.zone;
  }

  #face() {
    const node = this.#node;
    const copy = this.#copy;
    if (!node || !copy) return;
    copy.replaceChildren(
      ...[...node.children].map((child) => child.cloneNode(true)),
    );
  }
}
