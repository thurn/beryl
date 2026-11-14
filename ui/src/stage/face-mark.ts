// The mark on a player, drawn on the face of their life orb (components/Player.tsx `LifeOrb`).
import { BEZEL, coreOutline } from "../components/Player";
import type { Mark } from "../generated/protocol";

/** The marks drawn in a light of their own tone (stage.css `--tone`), which may lie on a player. */
export const LIT: ReadonlySet<Mark> = new Set([
  "eligible",
  "selected",
  "friendly-target",
  "hostile-target",
]);

const SVG = "http://www.w3.org/2000/svg";

/** The outline just outside the core's bezel, where the mark's rim lies. */
const RIM = coreOutline(BEZEL / 2 + 1);

/** The core's own outline, which the mark's light fills from its edge. */
const FACE = coreOutline(0);

/** A different id for each mark's gradient, since two orbs and several marks share a page. */
let made = 0;

/**
 * Draws a mark on the player whose life orb lies under `node` (a `[data-life]` element), in the
 * art's own units, so that it follows the outline of the orb's face exactly. Arena lights a
 * player who may be picked by tinting their life total's badge to its own outline, with no ring
 * standing off it. A mark drawn as an ellipse about the face was once taller than the face, and
 * on the opponent's orb, whose art is drawn flipped, sat above its middle.
 */
export function faceMark(node: HTMLElement): SVGSVGElement {
  const down = node.dataset.life === "opponent" ? -1 : 1;
  const id = `stage-mark-face-${made++}`;
  const svg = document.createElementNS(SVG, "svg");
  svg.setAttribute("class", "stage-mark-face");
  svg.setAttribute("viewBox", "-136 -57 272 116");
  svg.innerHTML = `
    <defs>
      <radialGradient id="${id}">
        <stop offset="0.55" class="stage-mark-face-clear" />
        <stop offset="1" class="stage-mark-face-edge" />
      </radialGradient>
    </defs>
    <g transform="scale(1 ${down})">
      <path class="stage-mark-face-light" d="${FACE}" fill="url(#${id})" />
      <path class="stage-mark-face-rim" d="${RIM}" />
    </g>`;
  return svg;
}
