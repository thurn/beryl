// The choose-or-order decisions the game asks over the table, in one overlay
// (docs/ui_mockups/scry.png): what it shows, what it asks, and the answers it sends. The engine
// asks one pick at a time (a scry is a card for the bottom, then another, then done, then the
// order of the rest), so the overlay takes the whole choice at once and plans the picks that
// say it, which the game screen sends as each decision arrives. Pure functions of the view.
import { cardEntry } from "../cards";
import type {
  AskView,
  Candidate,
  DecisionView,
  Kind,
  ObjectView,
  PlayerView,
  Side,
  StackItem,
} from "../generated/protocol";
import { abilityText } from "./faces";
import { optionsMatch, type Intent } from "./decisions";

/** The card at the overlay's right: what asks the choice. A card by name alone when the view
 * has no object for it (an ability whose source has left). */
export interface Source {
  slot?: number;
  name: string;
}

interface Common {
  /** Keys the overlay to the decision it opened on, so its arrangement lasts until the plan it
   * made has been sent. */
  key: number;
  title: string;
  instruction: string;
  source?: Source;
}

/** Cards in two piles either side of a library, dragged between them and into order:
 * `left` from the outside in, `right` from the inside out, so the screen shows them as listed
 * with the library between. */
export interface Arrange extends Common {
  kind: "arrange";
  labels: [string, string];
  left: number[];
  right: number[];
  /** How many cards the right pile must hold, if a number. */
  rightCount?: number;
  /** Cards cannot go to the left pile. */
  leftClosed?: boolean;
  plan(left: number[], right: number[]): Intent[];
}

/** Cards in a row, some of which can be picked. */
export interface Pick extends Common {
  kind: "pick";
  cards: number[];
  pickable: number[];
  min: number;
  max: number;
  /** The confirm button's label for `n` picked. */
  confirm(n: number): string;
  plan(picked: number[]): Intent[];
}

/** A question with an answer per button, over the cards it is about. */
export interface Ask extends Common {
  kind: "ask";
  cards: number[];
  answers: { label: string; plan: Intent[] }[];
}

export type Choice = Arrange | Pick | Ask;

/** What the open decision asks, as the overlay shows it; none for a decision made on the board
 * (priority, targets, combat, mana) or on the opening screen. */
export function choiceOf(view: PlayerView): Choice | undefined {
  if (view.status.kind !== "deciding") return undefined;
  const at = new Situation(view, view.status.decision);
  const ask = at.decision.ask;
  switch (ask.kind) {
    case "plain":
      return plain(at);
    case "scry":
      return scry(at, ask);
    case "look":
      return look(at, ask);
    case "putBack":
      return putBack(at, ask);
    case "fateseal":
      return fateseal(at, ask);
    case "take":
      return takeAndBottom(at, ask.count, ask.take);
    case "bottom":
      return takeAndBottom(at, ask.count, 0);
    case "discardBasicLand":
      return discardBasicLand(at, ask);
    case "topOrBottom":
      return topOrBottom(at, ask);
    case "explore":
      return yesNo(at);
    case "untap":
      return untap(at, ask);
    case "unlessPays":
      return payOrCounter(at, ask) ?? yesNo(at);
    case "payLife":
      return payLife(at, ask);
    case "cleanup":
      return discard(at, ask.count);
    default:
      return assertNever(ask);
  }
}

function assertNever(ask: never): never {
  throw new Error(`an unknown ask: ${JSON.stringify(ask)}`);
}

/** A decision that asks no more than its kind says. */
function plain(at: Situation): Choice | undefined {
  switch (at.decision.kind) {
    case "chooseUpTo":
      return chooseUpTo(at);
    case "chooseObject":
      return chooseObject(at);
    case "yesNo":
      return yesNo(at);
    case "discard":
      return discard(at);
    case "exileCost":
      return exileCost(at);
    case "legendRule":
      return legendRule(at);
    case "castOffer":
      return castOffer(at);
    case "x":
      return chooseX(at);
    default:
      return undefined;
  }
}

/** One kind of ask's fields. */
type AskOf<K extends AskView["kind"]> = Extract<AskView, { kind: K }>;

/** Small numbers in words, as rules text writes them. */
const NUMBERS = [
  "zero",
  "one",
  "two",
  "three",
  "four",
  "five",
  "six",
  "seven",
  "eight",
  "nine",
  "ten",
];
const inWords = (n: number) => NUMBERS[n] ?? String(n);
const capitalized = (word: string) =>
  word.charAt(0).toUpperCase() + word.slice(1);
/** A player's, from the viewer's side. */
const whose = (side: Side) => (side === "you" ? "your" : "your opponent's");

/** The view around a decision, with the lookups the choices share. */
class Situation {
  readonly objects: Map<number, ObjectView>;
  /** The cards the decision offers, in its order. */
  readonly offered: number[];
  readonly resolving: StackItem | null;
  readonly resolvingName?: string;

  constructor(
    readonly view: PlayerView,
    readonly decision: DecisionView,
  ) {
    this.objects = new Map(view.objects.map((o) => [o.slot, o]));
    this.offered = decision.candidates.flatMap((c) =>
      c.option.kind === "object" ? [c.option.slot] : [],
    );
    this.resolving = decision.resolving;
    const r = this.resolving;
    this.resolvingName =
      r?.kind === "ability" ? r.name : r ? this.name(r.slot) : undefined;
  }

  get key() {
    return this.decision.id.seq;
  }

  name(slot: number) {
    return this.objects.get(slot)?.name ?? undefined;
  }

  /** The card the decision is for, as the engine names it: what is resolving, the spell or
   * ability being cast or activated, or the land asking for life. */
  get source(): Source | undefined {
    const r = this.resolving;
    const slot = this.decision.source ?? undefined;
    const name =
      r?.kind === "ability"
        ? r.name
        : slot === undefined
          ? undefined
          : this.name(slot);
    return name ? { slot, name } : undefined;
  }

  /** The source's rules text: an ability's own paragraph, or the spell's text. */
  get sourceText(): string {
    const r = this.resolving;
    const name = this.source?.name;
    const text = (name && cardEntry(name)?.faces[0]?.oracle_text) || "";
    return r?.kind === "ability" ? abilityText(text, r.triggered, r.nth) : text;
  }

  /** A player's library, top first. */
  library(side: Side) {
    return [...this.view.players[side].library].reverse();
  }

  hand(side: Side) {
    return this.view.objects
      .filter((o) => o.zone === "hand" && o.owner === side)
      .map((o) => o.slot);
  }

  /** The cards among `slots` the decision offers, or a card the engine would offer in place of
   * one: an interchangeable one shares its name (the engine offers one of them, §7.2). */
  offeredAmong(slots: number[]) {
    const names = new Set(this.offered.map((s) => this.name(s)));
    return slots.filter(
      (s) => this.offered.includes(s) || names.has(this.name(s)),
    );
  }

  /** The cards a look at the top `count` of a player's library shows, top first. */
  looked(count: number, side: Side = "you") {
    return this.library(side)
      .slice(0, count)
      .filter((s) => this.name(s) !== undefined);
  }

  /** Picks the card in `slot`, or one interchangeable with it. */
  pickCard(kind: Kind, slot: number): Intent {
    const name = this.name(slot);
    return {
      kind,
      wants: (o) => o.kind === "object" && o.slot === slot,
      alike: (o) =>
        o.kind === "object" && name !== undefined && this.name(o.slot) === name,
    };
  }

  /** Each candidate of the decision as an answer, with its own label or `label`'s. */
  answers(label: (c: Candidate, fallback: string) => string) {
    return this.decision.candidates.map((c) => ({
      label: label(c.option, c.label),
      plan: [answer(this.decision.kind, c.option)],
    }));
  }
}

/** The answer that names `option`. */
const answer = (kind: Kind, option: Candidate): Intent => ({
  kind,
  wants: (o) => optionsMatch(o, option),
});

const yes = (kind: Kind): Intent => ({ kind, wants: (o) => o.kind === "yes" });
const no = (kind: Kind): Intent => ({ kind, wants: (o) => o.kind === "no" });
const done = (kind: Kind): Intent => ({
  kind,
  wants: (o) => o.kind === "done",
});

/** Scry ([CR 701.22a](https://yawgatog.com/resources/magic-rules/#R70122a)): "look at the top
 * N cards of your library, then put any number of them on the bottom of your library in any
 * order and the rest on top of your library in any order." The engine takes the cards for the
 * bottom one at a time, each under the one before, then done, then the rest from the top down
 * (`EffectCtx::scry`). Asked for the order alone, the bottom is settled. */
function scry(at: Situation, { count }: AskOf<"scry">): Arrange {
  const ordering = at.decision.kind === "chooseObject";
  // Top first; a card the engine collapsed with an identical one still shows.
  const cards = at.offeredAmong(at.looked(count));
  const shown = cards.length > 0 ? cards : at.offered;
  return {
    kind: "arrange",
    key: at.key,
    title: "Scry",
    instruction: ordering
      ? "Drag cards to put them on top of your library in order."
      : "Drag cards to put them on the top or bottom of your library.",
    source: at.source,
    labels: ["Bottom", "Top"],
    left: [],
    right: [...shown].reverse(),
    leftClosed: ordering,
    plan(left, right) {
      // The bottom pile from the library out; then, from the top down, all but the last.
      const bottom = [...left]
        .reverse()
        .map((slot) => at.pickCard("chooseUpTo", slot));
      const top = [...right].reverse();
      const order = top
        .slice(0, -1)
        .map((slot) => at.pickCard("chooseObject", slot));
      if (ordering) return order;
      return top.length > 0
        ? [...bottom, done("chooseUpTo"), ...order]
        : bottom;
    },
  };
}

/** Cards from your hand put on top of your library in any order, each on the one before. */
function putBack(at: Situation, { count }: AskOf<"putBack">): Arrange {
  const hand = at.hand("you");
  const need = Math.min(count, hand.length);
  return {
    kind: "arrange",
    key: at.key,
    title: "Put Back",
    instruction: `Drag ${need === 1 ? "a card" : `${inWords(need)} cards`} from your hand onto your library, top card rightmost.`,
    source: at.source,
    labels: ["Hand", "Top"],
    left: hand,
    right: [],
    rightCount: need,
    plan: (_left, right) =>
      right.map((slot) => at.pickCard("chooseObject", slot)),
  };
}

/** A look at the top of your library that puts `take` of the cards into your hand and the rest
 * on the bottom in any order. The engine takes the cards for the hand one at a time, then those
 * for the bottom, each under the one before, the last unasked. Asked for the bottom alone
 * (`take` 0), the hand is settled. */
function takeAndBottom(at: Situation, count: number, take: number): Arrange {
  const cards = at.offeredAmong(at.looked(count));
  const shown = cards.length > 0 ? cards : at.offered;
  const need = Math.min(take, shown.length);
  return {
    kind: "arrange",
    key: at.key,
    title: `Top ${capitalized(inWords(count))}`,
    instruction:
      need > 0
        ? `Drag ${need === 1 ? "a card" : `${inWords(need)} cards`} to your hand. Order the rest for the bottom.`
        : "Drag cards to order them on the bottom of your library.",
    source: at.source,
    labels: ["Bottom", "Hand"],
    left: [...shown].reverse(),
    right: [],
    rightCount: need,
    plan(left, right) {
      const hand = right.map((slot) => at.pickCard("chooseObject", slot));
      // The bottom pile from the library out, all but the outermost.
      const bottom = [...left]
        .reverse()
        .slice(0, -1)
        .map((slot) => at.pickCard("chooseObject", slot));
      return [...hand, ...bottom];
    },
  };
}

/** "Discard two cards unless you discard a basic land card": which, then the discards. */
function discardBasicLand(
  at: Situation,
  { count }: AskOf<"discardBasicLand">,
): Ask {
  return {
    kind: "ask",
    key: at.key,
    title: "Discard",
    instruction: `Discard a basic land, or ${inWords(count)} cards.`,
    source: at.source,
    cards: [],
    answers: [
      { label: "Basic Land", plan: [yes("yesNo")] },
      {
        label: `${capitalized(inWords(count))} Cards`,
        plan: [no("yesNo")],
      },
    ],
  };
}

/** Whether the top card of a player's library goes to the bottom. */
function fateseal(at: Situation, { player }: AskOf<"fateseal">): Ask {
  const top = at.library(player)[0];
  return {
    kind: "ask",
    key: at.key,
    title: "Top of Library",
    instruction: `You may put the top card of ${whose(player)} library on the bottom.`,
    source: at.source,
    cards: top === undefined ? [] : [top],
    answers: [
      { label: "Put on Bottom", plan: [yes("yesNo")] },
      { label: "Keep on Top", plan: [no("yesNo")] },
    ],
  };
}

/** Whether a spell goes on top of its owner's library or the bottom. */
function topOrBottom(at: Situation, { card }: AskOf<"topOrBottom">): Ask {
  return {
    kind: "ask",
    key: at.key,
    title: "Top or Bottom?",
    instruction: "Put it into your library.",
    source: at.source,
    cards: [card],
    answers: [
      { label: "Top", plan: [yes("yesNo")] },
      { label: "Bottom", plan: [no("yesNo")] },
    ],
  };
}

/** A look at the top of a library or at a hand that may move one of the cards it lights
 * (the source card at the right says which, and what happens to the rest): asked yes or no
 * first, then which (`may_choose_object`). */
function look(at: Situation, ask: AskOf<"look">): Pick {
  const { player, zone, count, to, eligible } = ask;
  const library = zone === "library";
  const cards = library ? at.looked(count, player) : at.hand(player);
  const asking = at.decision.kind === "yesNo";
  const pickable = asking
    ? cards.filter((s) => eligible.includes(s))
    : at.offeredAmong(cards);
  const title = library
    ? `Top ${capitalized(inWords(count))}`
    : player === "you"
      ? "Your Hand"
      : "Opponent's Hand";
  const taking = to === "hand";
  const verb = taking ? "Put in Hand" : capitalized(to);
  return {
    kind: "pick",
    key: at.key,
    title,
    instruction: taking
      ? "You may put a highlighted card into your hand."
      : to === "exile"
        ? "You may exile a highlighted card."
        : "You may choose a highlighted card.",
    source: at.source,
    cards,
    pickable,
    min: asking ? 0 : 1,
    max: 1,
    confirm: (n) =>
      n > 0 ? verb : taking ? "Take Nothing" : `${capitalized(to)} Nothing`,
    plan: ([slot]) =>
      slot === undefined
        ? [no("yesNo")]
        : [
            ...(asking ? [yes("yesNo")] : []),
            at.pickCard("chooseObject", slot),
          ],
  };
}

/** Permanents to untap, up to a number, one at a time, then done. */
function untap(at: Situation, { max }: AskOf<"untap">): Pick {
  const cards = at.offeredAmong(at.view.battlefield);
  const lands = cards.every((s) => at.objects.get(s)?.types.includes("land"));
  const noun = lands ? "lands" : "permanents";
  return {
    kind: "pick",
    key: at.key,
    title: `Untap ${capitalized(noun)}`,
    instruction: `Choose up to ${inWords(max)} ${noun} to untap.`,
    source: at.source,
    cards,
    pickable: cards,
    min: 0,
    max,
    confirm: (n) => (n > 0 ? `Untap ${n}` : "Untap None"),
    plan: (picked) => [
      ...picked.map((slot) => at.pickCard("chooseUpTo", slot)),
      done("chooseUpTo"),
    ],
  };
}

/** Any other choice of up to a number of cards, one at a time, then done. */
function chooseUpTo(at: Situation): Pick {
  const cards = at.offered;
  return {
    kind: "pick",
    key: at.key,
    title: "Choose Cards",
    instruction: at.sourceText || "Choose any number of cards.",
    source: at.source,
    cards,
    pickable: cards,
    min: 0,
    max: cards.length,
    confirm: () => "Done",
    plan: (picked) => [
      ...picked.map((slot) => at.pickCard("chooseUpTo", slot)),
      done("chooseUpTo"),
    ],
  };
}

function chooseObject(at: Situation): Pick {
  return {
    kind: "pick",
    key: at.key,
    title: "Choose a Card",
    instruction: at.sourceText || "Choose a card.",
    source: at.source,
    cards: at.offered,
    pickable: at.offered,
    min: 1,
    max: 1,
    confirm: () => "Choose",
    plan: ([slot]) =>
      slot === undefined ? [] : [at.pickCard("chooseObject", slot)],
  };
}

/** A discard of the player's choice ([CR 701.9b](https://yawgatog.com/resources/magic-rules/#R7019b)),
 * one card at a time; in the cleanup step, `cleanup` of them down to the maximum hand size
 * ([CR 514.1](https://yawgatog.com/resources/magic-rules/#R5141)). */
function discard(at: Situation, cleanup?: number): Pick {
  const hand = at.hand("you");
  const count = Math.max(1, cleanup ?? 1);
  return {
    kind: "pick",
    key: at.key,
    title: "Discard",
    instruction:
      count === 1
        ? "Choose a card to discard."
        : `Choose ${count} cards to discard down to ${inWords(hand.length - count)}.`,
    source: cleanup === undefined ? at.source : undefined,
    cards: hand,
    pickable: at.offeredAmong(hand),
    min: count,
    max: count,
    confirm: () => "Discard",
    plan: (picked) => picked.map((slot) => at.pickCard("discard", slot)),
  };
}

/** A card from the hand to exile for a cost, as evoke's "Exile a blue card from your hand"
 * ([CR 601.2h](https://yawgatog.com/resources/magic-rules/#R6012h)). */
function exileCost(at: Situation): Pick {
  const hand = at.hand("you");
  return {
    kind: "pick",
    key: at.key,
    title: "Exile",
    instruction: "Choose a card to exile.",
    source: at.source,
    cards: hand,
    pickable: at.offeredAmong(hand),
    min: 1,
    max: 1,
    confirm: () => "Exile",
    plan: ([slot]) =>
      slot === undefined ? [] : [at.pickCard("exileCost", slot)],
  };
}

/** The legend rule ([CR 704.5j](https://yawgatog.com/resources/magic-rules/#R7045j)): the
 * controller of two legendary permanents with the same name chooses one; the rest go to the
 * graveyard. */
function legendRule(at: Situation): Pick {
  const cards = at.offeredAmong(
    at.view.battlefield.filter((s) => at.objects.get(s)?.controller === "you"),
  );
  return {
    kind: "pick",
    key: at.key,
    title: "Legend Rule",
    instruction: "Choose the legend to keep. The other goes to your graveyard.",
    cards,
    pickable: cards,
    min: 1,
    max: 1,
    confirm: () => "Keep",
    plan: ([slot]) =>
      slot === undefined ? [] : [at.pickCard("legendRule", slot)],
  };
}

/** "As this land enters, you may pay N life. If you don't, it enters tapped." */
function payLife(at: Situation, { life }: AskOf<"payLife">): Ask {
  const source = at.source;
  return {
    kind: "ask",
    key: at.key,
    title: `Pay ${life} Life?`,
    instruction: `${source?.name ?? "This land"} enters tapped unless you pay ${life} life.`,
    source,
    cards: [],
    answers: [
      { label: `Pay ${life} Life`, plan: [yes("payLife")] },
      { label: "Enter Tapped", plan: [no("payLife")] },
    ],
  };
}

/** "Counter target spell unless its controller pays {N}", asked of that spell's controller
 * ([CR 118.12a](https://yawgatog.com/resources/magic-rules/#R11812a)). */
function payOrCounter(
  at: Situation,
  { cost }: AskOf<"unlessPays">,
): Ask | undefined {
  const r = at.resolving;
  const target = r?.targets[0];
  if (r?.kind !== "spell" || target?.kind !== "object") return undefined;
  const spell = at.name(target.slot) ?? "your spell";
  return {
    kind: "ask",
    key: at.key,
    title: `Pay ${cost}?`,
    instruction: `${at.resolvingName} counters ${spell} unless you pay ${cost}.`,
    source: at.source,
    cards: [target.slot],
    answers: [
      { label: `Pay ${cost}`, plan: [yes("yesNo")] },
      { label: "Don't Pay", plan: [no("yesNo")] },
    ],
  };
}

function yesNo(at: Situation): Ask {
  return {
    kind: "ask",
    key: at.key,
    title: at.resolvingName ?? "Choose",
    instruction: at.sourceText || "Yes or no?",
    source: at.source,
    cards: [],
    answers: at.answers((_, label) => label),
  };
}

/** A card a resolved ability lets its controller cast now, without paying its mana cost
 * ([CR 608.2g](https://yawgatog.com/resources/magic-rules/#R6082g)). The ability has finished
 * resolving, so the decision names no source. */
function castOffer(at: Situation): Ask {
  const casts = at.decision.candidates.filter((c) => c.option.kind === "cast");
  const first = casts[0]?.option;
  const slot = first?.kind === "cast" ? first.slot : undefined;
  const name = slot === undefined ? undefined : at.name(slot);
  return {
    kind: "ask",
    key: at.key,
    title: name ? `Cast ${name}?` : "Cast It?",
    instruction: "You may cast it now without paying its mana cost.",
    cards: slot === undefined ? [] : [slot],
    answers: at.answers((o, label) =>
      o.kind === "pass"
        ? "Don't Cast"
        : o.kind === "cast" && casts.length === 1
          ? "Cast"
          : label,
    ),
  };
}

/** The value of X as a spell is cast ([CR 601.2b](https://yawgatog.com/resources/magic-rules/#R6012b)),
 * a kicker's X among them. */
function chooseX(at: Situation): Ask {
  const name = at.source?.name;
  return {
    kind: "ask",
    key: at.key,
    title: "Choose X",
    instruction: `Choose the value of X${name ? ` for ${name}` : ""}.`,
    source: at.source,
    cards: [],
    answers: at.answers((_, label) => label),
  };
}
