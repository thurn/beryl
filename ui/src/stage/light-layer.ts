/** The CSS fallback changes only when the canvas layers available to paint light change.
 * Rewriting this attribute on every stage frame makes Android redraw an otherwise still
 * table while the player is deciding. */
export function showLightLayers(
  layer: Pick<HTMLElement, "dataset">,
  painted: string,
) {
  if ((layer.dataset.light ?? "") === painted) return;
  if (painted) layer.dataset.light = painted;
  else delete layer.dataset.light;
}
