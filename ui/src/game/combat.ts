// The combat damage a player stands to take, as a view shows the attack. Pure functions of
// the view.
import type { PlayerView, Side } from "../generated/protocol";
import { aimsAttack } from "./decisions";

/** Older recordings named the attackers only in the attacking player's question. */
export function pendingAttackers(view: PlayerView): number[] {
  const decision =
    view.status.kind === "deciding" ? view.status.decision : undefined;
  return (
    view.pendingAttackers ??
    (decision && aimsAttack(decision) ? decision.prompt.about : [])
  );
}

/** The combat damage `side` stands to take from the creatures attacking them that `view`
 * shows unblocked, or undefined if none attacks them
 * ([CR 510.1b](https://yawgatog.com/resources/magic-rules/#R5101b)): "An unblocked creature
 * assigns its combat damage to the player, planeswalker, or battle it's attacking." The engine
 * has no trample or double strike, so a blocked creature deals the player none and each
 * deals its power once. An attacker among `blocked` is counted as blocked: one a blocker is
 * aimed at while the blocks are still being declared. */
export function incoming(
  view: PlayerView,
  side: Side,
  blocked?: ReadonlySet<number>,
): number | undefined {
  let total: number | undefined;
  const pending = pendingAttackers(view);
  for (const o of view.objects) {
    const c = o.combat;
    if (c?.kind !== "attacking" || c.targetDeparted || c.planeswalker !== null)
      continue;
    if (pending.includes(o.slot)) continue;
    if (o.controller === side) continue;
    const stopped = c.blocked || blocked?.has(o.slot);
    total = (total ?? 0) + (stopped ? 0 : Math.max(0, o.pt?.power ?? 0));
  }
  return total;
}
