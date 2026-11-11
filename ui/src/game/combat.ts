// A combat declaration made on the board: the attackers, or the blockers and the attackers
// they block, picked in any order and changed at will until the player confirms, shown on the
// table as if declared. The engine takes a declaration one pick at a time, with identical
// creatures as one candidate (architecture §7.2, §11), so confirming plans the picks that say
// it, which the game screen sends as each decision arrives. Pure functions of the view.
import type { ObjectView, PlayerView, Side } from "../generated/protocol";
import { aimsAttack, twins, type Intent } from "./decisions";

/** A declaration in the making: each attacker and what it attacks (a planeswalker's slot, or
 * null for the defending player); each blocker and the attacker it blocks; and the creature
 * last picked for blocking, or the group currently choosing an attack destination. */
export interface Draft {
  attacks: Map<number, number | null>;
  blocks: Map<number, number>;
  selected?: number;
  attackGroup: number[];
  aimed?: boolean;
}

export const emptyDraft = (): Draft => ({
  attacks: new Map(),
  blocks: new Map(),
  attackGroup: [],
});

/** `view` with `draft` declared: its attackers attacking, its blockers blocking and their
 * attackers blocked, so the table lays it out and draws its lines as it would the real one. */
export function declared(view: PlayerView, draft: Draft): PlayerView {
  if (draft.attacks.size === 0 && draft.blocks.size === 0) return view;
  const blocked = new Set(draft.blocks.values());
  const objects = view.objects.map((o): ObjectView => {
    const walker = draft.attacks.get(o.slot);
    if (walker !== undefined) {
      return {
        ...o,
        combat: { kind: "attacking", planeswalker: walker, blocked: false },
      };
    }
    const attacker = draft.blocks.get(o.slot);
    if (attacker !== undefined) {
      return { ...o, combat: { kind: "blocking", attacker } };
    }
    if (blocked.has(o.slot) && o.combat?.kind === "attacking") {
      return { ...o, combat: { ...o.combat, blocked: true } };
    }
    return o;
  });
  return { ...view, objects };
}

/** Older recordings named the attackers only in the attacking player's question. */
export function pendingAttackers(view: PlayerView): number[] {
  const decision =
    view.status.kind === "deciding" ? view.status.decision : undefined;
  return (
    view.pendingAttackers ??
    (decision && aimsAttack(decision) ? decision.prompt.about : [])
  );
}

/** Assigns the current group together. It stays available for changing the destination. */
export function aimAttacks(draft: Draft, target: number | null): Draft {
  const attacks = new Map(draft.attacks);
  for (const slot of draft.attackGroup) attacks.set(slot, target);
  return { ...draft, attacks, aimed: true };
}

/** The combat damage `side` stands to take from the creatures attacking them that `view`
 * shows unblocked, or undefined if none attacks them
 * ([CR 510.1b](https://yawgatog.com/resources/magic-rules/#R5101b)): "An unblocked creature
 * assigns its combat damage to the player, planeswalker, or battle it's attacking." The engine
 * has no trample or double strike, so a blocked creature deals the player none and each
 * deals its power once. */
export function incoming(view: PlayerView, side: Side): number | undefined {
  let total: number | undefined;
  const pending = pendingAttackers(view);
  for (const o of view.objects) {
    const c = o.combat;
    if (c?.kind !== "attacking" || c.targetDeparted || c.planeswalker !== null)
      continue;
    if (pending.includes(o.slot)) continue;
    if (o.controller === side) continue;
    total = (total ?? 0) + (c.blocked ? 0 : Math.max(0, o.pt?.power ?? 0));
  }
  return total;
}

/** The first of `slots` interchangeable with the object in `slot`. */
function twinAmong(
  objects: Map<number, ObjectView>,
  slot: number,
  slots: Iterable<number>,
): number | undefined {
  const o = objects.get(slot);
  if (!o) return undefined;
  for (const s of slots) {
    const other = objects.get(s);
    if (other && twins(o, other)) return s;
  }
  return undefined;
}

/** The picks that declare `attacks` ([CR 508.1a](https://yawgatog.com/resources/magic-rules/#R5081a)):
 * "The active player chooses which creatures that they control, if any, will attack." At each
 * pick of an attacker, the first creature the engine offers that the player chose, or one
 * interchangeable with it (it offers one of identical creatures); what that creature attacks,
 * when the engine asks ([CR 508.1b](https://yawgatog.com/resources/magic-rules/#R5081b): only
 * while the defending player controls a planeswalker); then done. */
export function attackPlan(
  attacks: Map<number, number | null>,
  objects: Map<number, ObjectView>,
): Intent[] {
  const left = new Map(attacks);
  // What the creature just picked attacks.
  let aim: number | null = null;
  const attack: Intent = {
    kind: "attacker",
    wants: (o) => o.kind === "attack" && left.has(o.slot),
    alike: (o) =>
      o.kind === "attack" &&
      twinAmong(objects, o.slot, left.keys()) !== undefined,
    picked(o) {
      if (o.kind !== "attack") return;
      const slot = left.has(o.slot)
        ? o.slot
        : twinAmong(objects, o.slot, left.keys())!;
      aim = left.get(slot) ?? null;
      left.delete(slot);
    },
  };
  const defender: Intent = {
    kind: "attackTarget",
    optional: true,
    wants: (o) => o.kind === "defender" && o.planeswalker === aim,
  };
  return [
    ...[...attacks.keys()].flatMap(() => [attack, defender]),
    { kind: "attacker", wants: (o) => o.kind === "done" },
  ];
}

/** The picks that declare `blocks` ([CR 509.1a](https://yawgatog.com/resources/magic-rules/#R5091a)):
 * "The defending player chooses which creatures they control, if any, will block ... For each
 * of the chosen creatures, the defending player chooses one creature for it to block". At
 * each pick of a blocker, the first creature the engine offers that the player chose, or one
 * interchangeable with it; then the attacker it blocks; then done.
 *
 * Of identical attackers the engine offers one, the lowest slot (§7.2), and picks it unasked
 * when the blocker may block no other; once blocked, an attacker is told apart from its twins
 * (CR 509.1h: it "becomes a blocked creature"). So the attacker the player clicked may not be
 * the one that gets blocked, and later blockers of it follow the one that did. */
export function blockPlan(
  blocks: Map<number, number>,
  objects: Map<number, ObjectView>,
): Intent[] {
  const left = new Map(blocks);
  // The attacker the engine took for each one the player clicked.
  const taken = new Map<number, number>();
  // The attacker the blocker just picked blocks, as the player clicked it.
  let target: number | undefined;
  const block: Intent = {
    kind: "blocker",
    wants: (o) => o.kind === "block" && left.has(o.slot),
    alike: (o) =>
      o.kind === "block" &&
      twinAmong(objects, o.slot, left.keys()) !== undefined,
    picked(o) {
      if (o.kind !== "block") return;
      const slot = left.has(o.slot)
        ? o.slot
        : twinAmong(objects, o.slot, left.keys())!;
      target = left.get(slot);
      left.delete(slot);
    },
  };
  const aimed = () =>
    target === undefined ? undefined : (taken.get(target) ?? target);
  const attacker: Intent = {
    kind: "blockTarget",
    optional: true,
    wants: (o) => o.kind === "object" && o.slot === aimed(),
    alike: (o) =>
      o.kind === "object" &&
      target !== undefined &&
      !taken.has(target) &&
      twinAmong(objects, o.slot, [target]) !== undefined,
    picked(o) {
      if (o.kind === "object" && target !== undefined)
        taken.set(target, o.slot);
    },
    skipped() {
      // Picked unasked: the lowest of the attacker and its twins not yet blocked.
      if (target === undefined || taken.has(target)) return;
      const blocked = new Set(taken.values());
      const lowest = [...objects.keys()]
        .filter(
          (s) =>
            !blocked.has(s) &&
            (s === target || twinAmong(objects, s, [target!]) !== undefined),
        )
        .sort((a, b) => a - b)[0];
      if (lowest !== undefined) taken.set(target, lowest);
    },
  };
  return [
    ...[...blocks.keys()].flatMap(() => [block, attacker]),
    { kind: "blocker", wants: (o) => o.kind === "done" },
  ];
}

/** The attackers a blocker may block, by the engine's preview: one of each set of identical
 * attackers, so each one's twins too. */
export function blockable(
  offered: number[],
  objects: Map<number, ObjectView>,
): Set<number> {
  const out = new Set(offered);
  for (const o of objects.values()) {
    if (o.combat?.kind !== "attacking") continue;
    if (twinAmong(objects, o.slot, offered) !== undefined) out.add(o.slot);
  }
  return out;
}
