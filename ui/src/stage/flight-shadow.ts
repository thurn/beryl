import type { Pose } from "./pose.ts";

const shadows = new WeakMap<HTMLElement, HTMLElement>();
const FRAME = 100;

export function removeWithFlightShadow(card?: HTMLElement) {
  if (!card) return;
  drawFlightShadow(card);
  card.remove();
}

/** Draw a flying card's shadow below the resting cards. Moving and fading the shape
 * only change its compositor layer, as for the shadow of a card the pointer holds. */
export function drawFlightShadow(card: HTMLElement, pose?: Pose["shadow"]) {
  let shadow = shadows.get(card);
  if (!pose || pose.opacity < 0.001) {
    if (shadow) {
      shadow.remove();
      shadows.delete(card);
    }
    return;
  }
  if (!shadow) {
    shadow = document.createElement("div");
    shadow.className = "stage-held-shadow stage-flight-shadow";
    shadow.setAttribute("aria-hidden", "true");
    shadow.style.setProperty("--frame", `${FRAME}px`);
    card.before(shadow);
    shadows.set(card, shadow);
  }
  const scale = pose.width / FRAME;
  shadow.style.transform = `translate(${pose.x}px, ${pose.y}px) scale(${scale}, ${scale * 0.7}) translate(-50%, -50%)`;
  shadow.style.opacity = String(pose.opacity);
}
