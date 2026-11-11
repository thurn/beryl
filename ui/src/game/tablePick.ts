// Choices of cards made on the table rather than in an overlay, with the table in view: an
// untap's lands, and a choice of cards from the hand (a discard, a card exiled to pay a cost),
// for which the hand rises whole into view. Pure functions of the view.
import { faceEntry } from "../cards";
import type { ObjectView, PlayerView } from "../generated/protocol";
import {
  slotOf,
  twins,
  untapPlan,
  type Intent,
  type Interactions,
} from "./decisions";
import { untapChoice } from "./untapChoice";

/** What a pick of a card from the hand is for: the ask of its `chooseObject` decision. */
type Ask = NonNullable<Intent["ask"]>;

/** A choice of cards made on the table rather than in an overlay, with the table in view: the
 * cards that can be picked, glowing; how many may be; those picked to begin with; and, for
 * the cards picked, whether they answer it, the confirm button's label, and the picks that
 * send it. */
export interface TablePick {
  eligible: Set<number>;
  max: number;
  /** The cards are the hand's: it rises whole into view to pick them. */
  fromHand?: boolean;
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
  if (ask.kind === "untap") return untapPick(view, can, objects, ask.max);
  if (ask.kind === "discardBasicLand" && decision.kind === "yesNo") {
    return basicLandOrCards(view, objects, ask.count);
  }
  if (ask.kind === "discard") {
    return handPick(view, objects, "discard", 1, "Discard");
  }
  if (ask.kind === "cleanup") {
    // Down to the hand size, its picks counted on the button.
    return handPick(
      view,
      objects,
      "cleanup",
      Math.max(1, ask.count),
      "Discard",
      true,
    );
  }
  if (ask.kind === "exileCards" && ask.zone === "hand") {
    return handPick(view, objects, "exileCards", 1, "Exile");
  }
  return undefined;
}

/** The player's hand. */
const handOf = (view: PlayerView) =>
  view.objects
    .filter((o) => o.zone === "hand" && o.owner === "you")
    .map((o) => o.slot);

/** Picks the card in `slot`, or one the engine offers in its place: an interchangeable one
 * (`twins`), of which it offers one (architecture §7.2). */
function pickOf(
  ask: Ask,
  slot: number,
  objects: Map<number, ObjectView>,
): Intent {
  return {
    kind: "chooseObject",
    ask,
    wants: (o) => o.kind === "object" && o.slot === slot,
    alike: (o) => {
      const [a, b] = [objects.get(slot), objects.get(slotOf(o) ?? -1)];
      return a !== undefined && b !== undefined && twins(a, b);
    },
  };
}

/** `count` cards from the hand, picked as the engine asks for them, one at a time: a discard
 * of the player's choice ([CR 701.9b](https://yawgatog.com/resources/magic-rules/#R7019b)),
 * the cleanup step's discard down to the maximum hand size
 * ([CR 514.1](https://yawgatog.com/resources/magic-rules/#R5141)), or a card exiled from the
 * hand to pay a cost, as evoke's "Exile a blue card from your hand"
 * ([CR 601.2h](https://yawgatog.com/resources/magic-rules/#R6012h)). The cards the engine
 * offers glow, and those interchangeable with one; a count over one is said on the button. */
function handPick(
  view: PlayerView,
  objects: Map<number, ObjectView>,
  ask: Ask,
  count: number,
  verb: string,
  counted = count > 1,
): TablePick | undefined {
  if (view.status.kind !== "deciding") return undefined;
  const offered = view.status.decision.candidates.flatMap((c) => {
    const o =
      c.option.kind === "object" ? objects.get(c.option.slot) : undefined;
    return o ? [o] : [];
  });
  const eligible = handOf(view).filter((slot) => {
    const o = objects.get(slot)!;
    return offered.some((c) => c.slot === slot || twins(c, o));
  });
  if (eligible.length === 0) return undefined;
  const need = Math.min(count, eligible.length);
  const ready = (picked: ReadonlySet<number>) => picked.size === need;
  return {
    eligible: new Set(eligible),
    max: need,
    fromHand: true,
    initial: [],
    ready,
    label: (picked) => (counted ? `${verb} ${picked.size}/${need}` : verb),
    plan: (picked) => [...picked].map((slot) => pickOf(ask, slot, objects)),
  };
}

/** "Untap up to N lands", whoever controls them: the engine offers only tapped ones
 * (CR 701.26b), so each of the player's own is worth untapping, and as many as may be are
 * picked to begin with, those whose mana their hand wants most ([`untapChoice`]); an
 * opponent's, whose mana would let them answer, only by a click. */
function untapPick(
  view: PlayerView,
  can: Interactions,
  objects: Map<number, ObjectView>,
  max: number,
): TablePick {
  const eligible = new Set(can.bySlot.keys());
  const initial = untapChoice(view, eligible, max, faceEntry);
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
  const hand = handOf(view);
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
  const discard = (slot: number) => pickOf("discard", slot, objects);
  return {
    eligible: new Set(hand),
    max: need,
    fromHand: true,
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
