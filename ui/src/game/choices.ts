// The choose-or-order decisions the game asks over the table, in one overlay
// (docs/ui_mockups/scry.png): what it shows, what it asks, and the answers it sends. The engine
// asks one pick at a time (a scry is a card for the bottom, then another, then done, then the
// order of the rest), so the overlay takes the whole choice at once and plans the picks that
// say it, which the game screen sends as each decision arrives. Pure functions of the view.
import { cardEntry } from "../cards";
import type {
  Candidate,
  DecisionView,
  Kind,
  ObjectView,
  PlayerView,
  StackItem,
} from "../generated/protocol";
import { abilityText } from "./faces";
import { optionsMatch, type Intent } from "./decisions";

/** The card at the overlay's right: what asks the choice. A card by name alone when the view
 * has no object for it (a land about to enter). */
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

const ICEBERG = "Aang's Iceberg";
const JACE = "Jace, the Mind Sculptor";
const TRAINER = "Thundertrap Trainer";
const SPELLBINDER = "Elite Spellbinder";
const TIME_SPIRAL = "Time Spiral";
const SKYDIVER = "Thieving Skydiver";
const SPRINGS = "Soporific Springs";

/** How many cards from the top of the library a card's ability looks at. */
const LOOKS: Record<string, number> = { [ICEBERG]: 2, [TRAINER]: 4 };
/** "Untap up to six lands." */
const TIME_SPIRAL_LANDS = 6;
/** The maximum hand size ([CR 402.2](https://yawgatog.com/resources/magic-rules/#R4022)):
 * none of these cards changes it. */
const HAND_SIZE = 7;

/** What the open decision asks, as the overlay shows it; none for a decision made on the board
 * (priority, targets, combat, mana) or on the opening screen. */
export function choiceOf(view: PlayerView): Choice | undefined {
  if (view.status.kind !== "deciding") return undefined;
  const at = new Situation(view, view.status.decision);
  switch (at.decision.kind) {
    case "chooseUpTo":
      return at.inLibrary(at.offered)
        ? scry(at)
        : at.resolvingName === TIME_SPIRAL
          ? untapLands(at)
          : chooseUpTo(at);
    case "chooseObject":
      if (at.resolvingName === ICEBERG) return scry(at);
      if (at.resolvingName === JACE) return putBack(at);
      if (at.resolvingName === TRAINER || at.resolvingName === SPELLBINDER) {
        return look(at);
      }
      return chooseObject(at);
    case "yesNo":
      if (at.resolvingName === JACE) return fateseal(at);
      if (at.resolvingName === TRAINER || at.resolvingName === SPELLBINDER) {
        return look(at);
      }
      return payOrCounter(at) ?? yesNo(at);
    case "discard":
      return discard(at);
    case "legendRule":
      return legendRule(at);
    case "payLife":
      return payLife(at);
    case "castOffer":
      return castOffer(at);
    case "x":
      return chooseX(at);
    default:
      return undefined;
  }
}

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

  /** The card the decision is for: what is resolving, or the spell being cast. */
  get source(): Source | undefined {
    const r = this.resolving ?? this.view.stack[this.view.stack.length - 1];
    if (!r) return undefined;
    const slot = r.kind === "spell" ? r.slot : (r.source ?? undefined);
    const name = r.kind === "ability" ? r.name : this.name(r.slot);
    return name ? { slot, name } : undefined;
  }

  /** The source's rules text: an ability's own paragraph, or the spell's text. */
  get sourceText(): string {
    const r = this.resolving;
    const name = this.source?.name;
    const text = (name && cardEntry(name)?.faces[0]?.oracle_text) || "";
    return r?.kind === "ability" ? abilityText(text, r.triggered, r.nth) : text;
  }

  inLibrary(slots: number[]) {
    return (
      slots.length > 0 &&
      slots.every((s) => this.objects.get(s)?.zone === "library")
    );
  }

  /** A player's library, top first. */
  library(side: "you" | "opponent") {
    return [...this.view.players[side].library].reverse();
  }

  hand(side: "you" | "opponent") {
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

  /** The cards a look at the top of your library shows, top first, the ones the decision
   * offers among them. */
  looked(count: number) {
    return this.library("you")
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

/** Front-face types, from the card data: what a card in a hidden zone is. */
function typeLine(name?: string) {
  return (name && cardEntry(name)?.faces[0]?.type_line) || "";
}

/** Scry ([CR 701.22a](https://yawgatog.com/resources/magic-rules/#R70122a)): "look at the top
 * N cards of your library, then put any number of them on the bottom of your library in any
 * order and the rest on top of your library in any order." The engine takes the cards for the
 * bottom one at a time, each under the one before, then done, then the rest from the top down
 * (`EffectCtx::scry`). Asked for the order alone, the bottom is settled. */
function scry(at: Situation): Arrange {
  const ordering = at.decision.kind === "chooseObject";
  const count = LOOKS[at.resolvingName ?? ""] ?? at.offered.length;
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

/** Jace, the Mind Sculptor's 0: "Draw three cards, then put two cards from your hand on top
 * of your library in any order." Each card put back goes on the one before. */
function putBack(at: Situation): Arrange {
  const back = at.view.events.filter(
    (e) => e.kind === "zoneChange" && e.from === "hand" && e.to === "library",
  ).length;
  const hand = at.hand("you");
  const need = Math.min(2 - back, hand.length);
  return {
    kind: "arrange",
    key: at.key,
    title: "Put Back",
    instruction: `Drag ${need === 1 ? "a card" : "two cards"} from your hand onto your library, top card rightmost.`,
    source: at.source,
    labels: ["Hand", "Top"],
    left: hand,
    right: [],
    rightCount: need,
    plan: (_left, right) =>
      right.map((slot) => at.pickCard("chooseObject", slot)),
  };
}

/** Jace, the Mind Sculptor's +2: "Look at the top card of target player's library. You may
 * put that card on the bottom of that player's library." */
function fateseal(at: Situation): Ask {
  const target = at.resolving?.targets[0];
  const whose = target?.kind === "player" ? target.player : "you";
  const top = at.library(whose)[0];
  return {
    kind: "ask",
    key: at.key,
    title: "Top of Library",
    instruction: `You may put the top card of ${whose === "you" ? "your" : "your opponent's"} library on the bottom.`,
    source: at.source,
    cards: top === undefined ? [] : [top],
    answers: [
      { label: "Put on Bottom", plan: [yes("yesNo")] },
      { label: "Keep on Top", plan: [no("yesNo")] },
    ],
  };
}

/** Thundertrap Trainer's look at the top four and Elite Spellbinder's at the opponent's hand:
 * may take a card of a kind. Asked yes or no first, then which (`may_choose_object`). */
function look(at: Situation): Pick {
  const trainer = at.resolvingName === TRAINER;
  const cards = trainer ? at.looked(LOOKS[TRAINER]!) : at.hand("opponent");
  const asking = at.decision.kind === "yesNo";
  const fits = (slot: number) => {
    const types = typeLine(at.name(slot));
    return trainer
      ? !/\b(Creature|Land)\b/.test(types)
      : !/\bLand\b/.test(types);
  };
  const pickable = asking ? cards.filter(fits) : at.offeredAmong(cards);
  const [title, instruction, verb] = trainer
    ? [
        "Top Four",
        "You may take a noncreature, nonland card. The rest go on the bottom.",
        "Put in Hand",
      ]
    : [
        "Opponent's Hand",
        "You may exile a nonland card. Its owner may cast it for {2} more.",
        "Exile",
      ];
  return {
    kind: "pick",
    key: at.key,
    title,
    instruction,
    source: at.source,
    cards,
    pickable,
    min: asking ? 0 : 1,
    max: 1,
    confirm: (n) => (n > 0 ? verb : trainer ? "Take Nothing" : "Exile Nothing"),
    plan: ([slot]) =>
      slot === undefined
        ? [no("yesNo")]
        : [
            ...(asking ? [yes("yesNo")] : []),
            at.pickCard("chooseObject", slot),
          ],
  };
}

/** Time Spiral's "untap up to six lands", one land at a time, then done. */
function untapLands(at: Situation): Pick {
  const cards = at.offeredAmong(at.view.battlefield);
  return {
    kind: "pick",
    key: at.key,
    title: "Untap Lands",
    instruction: "Choose up to six lands to untap.",
    source: at.source,
    cards,
    pickable: cards,
    min: 0,
    max: TIME_SPIRAL_LANDS,
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
 * one card at a time; in the cleanup step, down to the maximum hand size
 * ([CR 514.1](https://yawgatog.com/resources/magic-rules/#R5141)). */
function discard(at: Situation): Pick {
  const hand = at.hand("you");
  const count =
    at.view.turn.step === "cleanup" ? Math.max(1, hand.length - HAND_SIZE) : 1;
  return {
    kind: "pick",
    key: at.key,
    title: "Discard",
    instruction:
      count === 1
        ? "Choose a card to discard."
        : `Choose ${count} cards to discard down to seven.`,
    source: at.view.turn.step === "cleanup" ? undefined : at.source,
    cards: hand,
    pickable: at.offeredAmong(hand),
    min: count,
    max: count,
    confirm: () => "Discard",
    plan: (picked) => picked.map((slot) => at.pickCard("discard", slot)),
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

/** Soporific Springs: "As this land enters, you may pay 3 life. If you don't, it enters
 * tapped." */
function payLife(at: Situation): Ask {
  const text = cardEntry(SPRINGS)?.faces.find(
    (f) => f.name === SPRINGS,
  )?.oracle_text;
  const life = /pay (\d+) life/.exec(text ?? "")?.[1] ?? "3";
  return {
    kind: "ask",
    key: at.key,
    title: `Pay ${life} Life?`,
    instruction: `${SPRINGS} enters tapped unless you pay ${life} life.`,
    source: { name: SPRINGS },
    cards: [],
    answers: [
      { label: `Pay ${life} Life`, plan: [yes("payLife")] },
      { label: "Enter Tapped", plan: [no("payLife")] },
    ],
  };
}

/** Mana Tithe and No More Lies: "Counter target spell unless its controller pays {1}" (or
 * {3}), asked of that spell's controller
 * ([CR 118.12a](https://yawgatog.com/resources/magic-rules/#R11812a)). */
function payOrCounter(at: Situation): Ask | undefined {
  const r = at.resolving;
  const target = r?.targets[0];
  if (r?.kind !== "spell" || target?.kind !== "object") return undefined;
  const text = at.sourceText;
  const cost = /unless its controller pays ((?:\{[^}]+\})+)/.exec(text)?.[1];
  if (!cost) return undefined;
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

/** A card a resolved ability lets its controller cast now
 * ([CR 608.2g](https://yawgatog.com/resources/magic-rules/#R6082g)). Among these cards only
 * Malcolm, Alluring Scoundrel offers one: the card it discarded, without paying its mana
 * cost. The ability has finished resolving, so the decision names no source. */
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
 * Thieving Skydiver's kicker X among them. */
function chooseX(at: Situation): Ask {
  const name = at.source?.name;
  return {
    kind: "ask",
    key: at.key,
    title: "Choose X",
    instruction:
      name === SKYDIVER
        ? "Kicker {X}: X can't be 0. When it enters, gain control of an artifact with mana value X or less."
        : `Choose the value of X${name ? ` for ${name}` : ""}.`,
    source: at.source,
    cards: [],
    answers: at.answers((_, label) => label),
  };
}
