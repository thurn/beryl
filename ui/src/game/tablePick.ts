// Choices of cards made on the table rather than in an overlay, with the table in view: an
// untap's lands, and Thirst for Discovery's discard from the hand. Pure functions of the view.
import { faceEntry } from "../cards";
import type { ObjectView, PlayerView } from "../generated/protocol";
import {
  slotOf,
  twins,
  untapPlan,
  type Intent,
  type Interactions,
} from "./decisions";

/** A choice of cards made on the table rather than in an overlay, with the table in view: the
 * cards that can be picked, glowing; how many may be; those picked to begin with; and, for
 * the cards picked, whether they answer it, the confirm button's label, and the picks that
 * send it. */
export interface TablePick {
  eligible: Set<number>;
  max: number;
  initial: number[];
  ready(picked: ReadonlySet<number>): boolean;
  label(picked: ReadonlySet<number>): string;
  plan(picked: ReadonlySet<number>): Intent[];
}

/** The open decision's choice of cards on the table, if it is one: an untap of lands, or the
 * hand's discard of a basic land or of cards. */
export function tablePick(
  view: PlayerView,
  can: Interactions,
): TablePick | undefined {
  if (view.status.kind !== "deciding") return undefined;
  const decision = view.status.decision;
  const objects = new Map(view.objects.map((o) => [o.slot, o]));
  const { ask } = decision;
  if (ask.kind === "untap") return untapPick(can, objects, ask.max);
  if (ask.kind === "discardBasicLand" && decision.kind === "yesNo") {
    return basicLandOrCards(view, objects, ask.count);
  }
  return undefined;
}

/** "Untap up to N lands": the engine offers only tapped ones (CR 701.26b), so every one is
 * worth untapping, the player's own first; as many as may be are picked to begin with. */
function untapPick(
  can: Interactions,
  objects: Map<number, ObjectView>,
  max: number,
): TablePick {
  const eligible = new Set(can.bySlot.keys());
  const own = (slot: number) =>
    objects.get(slot)?.controller === "you" ? 0 : 1;
  const initial = [...eligible]
    .sort((a, b) => own(a) - own(b) || a - b)
    .slice(0, max);
  return {
    eligible,
    max,
    initial,
    ready: () => true,
    label: (picked) =>
      picked.size > 0 ? `Untap ${picked.size}/${max}` : "Untap None",
    plan: (picked) => untapPlan(picked, objects),
  };
}

/** "Discard two cards unless you discard a basic land card"
 * ([CR 118.12a](https://yawgatog.com/resources/magic-rules/#R11812a)): asked yes or no, then
 * the discards, here picked from the hand at once: one basic land, or `count` cards (as many
 * as the hand holds). */
function basicLandOrCards(
  view: PlayerView,
  objects: Map<number, ObjectView>,
  count: number,
): TablePick {
  const hand = view.objects
    .filter((o) => o.zone === "hand" && o.owner === "you")
    .map((o) => o.slot);
  const need = Math.min(count, hand.length);
  const basic = (slot: number) => {
    const name = objects.get(slot)?.name;
    return /^Basic\b.*\bLand\b/.test(
      (name && faceEntry(name)?.type_line) || "",
    );
  };
  const landAlone = (picked: ReadonlySet<number>) =>
    picked.size === 1 && basic([...picked][0]!);
  const ready = (picked: ReadonlySet<number>) =>
    landAlone(picked) || picked.size === need;
  const discard = (slot: number): Intent => ({
    kind: "discard",
    wants: (o) => o.kind === "object" && o.slot === slot,
    alike: (o) => {
      const [a, b] = [objects.get(slot), objects.get(slotOf(o) ?? -1)];
      return a !== undefined && b !== undefined && twins(a, b);
    },
  });
  return {
    eligible: new Set(hand),
    max: need,
    initial: [],
    ready,
    label: (picked) =>
      ready(picked) ? "Discard" : `Discard ${picked.size}/${need}`,
    plan: (picked) => [
      {
        kind: "yesNo",
        wants: (o) => o.kind === (landAlone(picked) ? "yes" : "no"),
      },
      ...[...picked].map(discard),
    ],
  };
}
