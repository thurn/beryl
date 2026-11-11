// The choose-or-order decisions the game asks over the table, in one overlay
// (docs/ui_mockups/scry.png): what it shows, what it asks, and the answers it sends. The engine
// asks one pick at a time (a scry is a card for the bottom, then another, then done, then the
// order of the rest), so the overlay takes the whole choice at once and plans the picks that
// say it, which the game screen sends as each decision arrives. Pure functions of the view.
import { cardEntry, faceEntry } from "../cards";
import type {
  Candidate,
  DecisionView,
  Kind,
  ObjectView,
  PlayerView,
  Side,
  StackItem,
} from "../generated/protocol";
import { counted, optionsMatch, type Intent } from "./decisions";

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

/** Cards from your hand put on your library, dragged from the fanned hand onto the deck and
 * into order there. */
export interface Stack extends Common {
  kind: "stack";
  hand: number[];
  /** How many cards go onto the library. */
  count: number;
  /** The cards on the library, the deepest first. */
  plan(onto: number[]): Intent[];
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
  /** A second button that picks nothing, if the choice may. */
  decline?: string;
  /** The cards picked to begin with. */
  picked?: number[];
  plan(picked: number[]): Intent[];
}

/** A question with an answer per button, the first the default, over the cards it is about;
 * asked of no card, it is the floating prompt's (`floats`). */
export interface Ask extends Common {
  kind: "ask";
  cards: number[];
  answers: { label: string; plan: Intent[] }[];
  /** None of the answers should be emphasized. */
  equalAnswers?: boolean;
}

export type Choice = Arrange | Stack | Pick | Ask;

/** The real cards placed in the tray, apart from its source. */
export function choiceSlots(choice: Choice): number[] {
  if (choice.kind === "stack") return choice.hand;
  if (choice.kind === "arrange") return [...choice.left, ...choice.right];
  return choice.cards;
}

/** Whether a choice is asked in the floating prompt over the table, as a title and a row of
 * buttons, rather than in the overlay over the dimmed table: a question with no cards. */
export const floats = (choice: Choice): choice is Ask =>
  choice.kind === "ask" && choice.cards.length === 0;

/** What the open decision asks, as the overlay shows it; none for a decision made on the board
 * (priority, targets, combat, mana) or on the opening screen. The prompt's shape says how it is
 * answered and its purpose which of that shape's arrangements it takes: a card that asks a
 * known purpose has its arm here already, and a purpose with no arm of its own is asked as its
 * shape's plainest choice, in the session's words. */
export function choiceOf(view: PlayerView): Choice | undefined {
  if (view.status.kind !== "deciding") return undefined;
  const at = new Situation(view, view.status.decision);
  const { shape, purpose } = at.prompt;
  switch (shape) {
    case "split":
      switch (purpose) {
        case "scry":
          return scry(at);
        case "put-back":
          return putBack(at);
        case "arrange":
          return takeAndBottom(at);
        default:
          return undefined;
      }
    case "yesNo":
      switch (purpose) {
        // Whether to move one card, and whether to pay a cost: small questions, asked on the
        // board.
        case "move":
        case "unless-pays":
          return undefined;
        case "pay-life":
          return payLife(at);
        default:
          return yesNo(at);
      }
    case "pick":
      switch (purpose) {
        case "look":
          return look(at);
        case "search":
          return search(at);
        case "exile-cost":
          return at.prompt.from?.zone === "graveyard"
            ? graveyardExile(at)
            : undefined;
        case "legend-rule":
          return small(at) ? undefined : legendRule(at);
        // Priority, targets and combat damage are decided on the board, as are untaps and
        // choices of cards from the hand (discards, a card exiled for a cost: the hand rises
        // to pick them, game/tablePick.ts); the opening screen asks for a mulligan's card for
        // the bottom.
        case "priority":
        case "target":
        case "combat-damage":
        case "untap":
        case "discard":
        case "cleanup":
        case "put-on-bottom":
          return undefined;
        default:
          return small(at) ? undefined : chooseObject(at);
      }
    case "number":
      return chooseX(at);
    case "mode":
      // The opening screen asks who plays first and for a mulligan.
      return purpose === "cast-offer" ? castOffer(at) : undefined;
    // A trigger order, a payment (the board lights what pays) and combat's declarations are
    // made on the board. No card asks for an entry of a list or for an amount divided yet.
    case "order":
    case "pay":
    case "declare":
    case "catalog":
    case "divide":
      return undefined;
  }
}

/** The most cards a choice picked on the board offers: its prompt's one row. */
export const SMALL_CHOICE = 5;

/** Whether a choice among cards is small enough to make on the board rather than over it:
 * a few cards, each in a zone every player sees (CR 400.2), which the board's prompt shows as
 * they lie or by their faces, as a few cards exiled with a permanent. */
function small(at: Situation): boolean {
  const offered = at.decision.candidates;
  return (
    offered.length <= SMALL_CHOICE &&
    offered.every((c) => {
      if (c.option.kind !== "object") return false;
      const zone = at.objects.get(c.option.slot)?.zone;
      return (
        zone === "battlefield" ||
        zone === "graveyard" ||
        zone === "exile" ||
        zone === "stack"
      );
    })
  );
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

  /** The prompt the decision's pick belongs to. */
  get prompt() {
    return this.decision.prompt;
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

  /** Whether the land being played, untapped, would let the viewer cast another card in
   * their hand that their untapped mana sources cannot pay for, by mana value. */
  untappedLandWanted(land?: number): boolean {
    const mana = this.view.battlefield.reduce((sum, s) => {
      const o = this.objects.get(s);
      return o?.controller === "you" && !o.tapped ? sum + manaMade(o) : sum;
    }, 0);
    return this.hand("you").some((s) => {
      const name = this.name(s);
      const face = name ? cardEntry(name)?.faces[0] : undefined;
      if (s === land || !face || /\bLand\b/.test(face.type_line)) return false;
      const value = manaValue(face.mana_cost ?? "");
      return value > mana && value <= mana + 1;
    });
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

  /** Picks the card in `slot`, or one interchangeable with it, at a decision of `kind`
   * for `purpose`, if it says. */
  pickCard(kind: Kind, slot: number, purpose?: string): Intent {
    const name = this.name(slot);
    return {
      kind,
      purpose,
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

  /** The cards the prompt lays out from the top of a library: the top `count` of the library
   * its cards lie in, top first. */
  get shown() {
    return this.looked(this.prompt.count, this.prompt.from?.player ?? "you");
  }
}

/** A mana cost's mana value ([CR 202.3](https://yawgatog.com/resources/magic-rules/#R2023)),
 * X as zero: "{2}{W}{W}" is 4. */
function manaValue(cost: string): number {
  return [...cost.matchAll(/\{([^}]+)\}/g)].reduce((sum, [, symbol]) => {
    const n = Number(symbol);
    return sum + (Number.isInteger(n) ? n : symbol === "X" ? 0 : 1);
  }, 0);
}

/** The mana a permanent makes when tapped: one for a land; for another permanent, the
 * symbols of a "{T}: Add" ability, or one for "Add one mana". */
function manaMade(o: ObjectView): number {
  if (o.types.includes("land")) return 1;
  const text = (o.name && faceEntry(o.name)?.oracle_text) || "";
  const add = /^\{T\}: Add ([^.]*)/m.exec(text)?.[1];
  if (add === undefined) return 0;
  return Math.max(1, (add.match(/\{[^}]+\}/g) ?? []).length);
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
function scry(at: Situation): Arrange {
  const ordering = at.prompt.part === "second";
  // Top first; a card the engine collapsed with an identical one still shows.
  const cards = at.offeredAmong(at.shown);
  const shown = cards.length > 0 ? cards : at.offered;
  return {
    kind: "arrange",
    key: at.key,
    title: at.prompt.title,
    source: at.source,
    labels: at.prompt.words.piles,
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
function putBack(at: Situation): Stack {
  const hand = at.hand("you");
  return {
    kind: "stack",
    key: at.key,
    title: at.prompt.title,
    source: at.source,
    hand,
    count: Math.min(at.prompt.most, hand.length),
    plan: (onto) => onto.map((slot) => at.pickCard("chooseObject", slot)),
  };
}

/** A look at the top of your library that puts some of the cards into your hand (the prompt's
 * `most` of them) and the rest on the bottom in any order. The engine takes the cards for the
 * hand one at a time, then those for the bottom, each under the one before, the last unasked.
 * Asked for the bottom alone (the prompt's second part), the hand is settled. */
function takeAndBottom(at: Situation): Arrange {
  const cards = at.offeredAmong(at.shown);
  const shown = cards.length > 0 ? cards : at.offered;
  const take = at.prompt.part === "first" ? at.prompt.most : 0;
  const need = Math.min(take, shown.length);
  const [hand, bottom] = at.prompt.words.piles;
  return {
    kind: "arrange",
    key: at.key,
    title: at.prompt.title,
    source: at.source,
    labels: [bottom, hand],
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

/** A look at the top of a library or at a hand that may move one of the cards it lights
 * (the source card at the right says which, and what happens to the rest): asked yes or no
 * first, then which (`may_choose_object`). */
function look(at: Situation): Pick {
  const { from, about, words } = at.prompt;
  const player = from?.player ?? "you";
  const cards = from?.zone === "library" ? at.shown : at.hand(player);
  const asking = at.prompt.part === "whether";
  const pickable = asking
    ? cards.filter((s) => about.includes(s))
    : at.offeredAmong(cards);
  // A card is nearly always worth taking: the one card that can be is picked to begin with,
  // and taking nothing is the second button.
  return {
    kind: "pick",
    key: at.key,
    title: at.prompt.title,
    source: at.source,
    cards,
    pickable,
    min: 1,
    max: 1,
    confirm: (n) => counted(words.confirm, n, 1),
    decline: asking ? words.decline : undefined,
    picked: pickable.length === 1 ? pickable : [],
    plan: ([slot]) =>
      slot === undefined
        ? [no("yesNo")]
        : [
            ...(asking ? [yes("yesNo")] : []),
            at.pickCard("chooseObject", slot),
          ],
  };
}

/** A library search ([CR 701.23a](https://yawgatog.com/resources/magic-rules/#R70123a)) for
 * up to `max` cards, which the engine asks for one at a time, then done, unless it has as many
 * as it may take or none is left to take. The source card at the right says what the search
 * finds. Each card of the library the search can find is shown, interchangeable ones too (the
 * engine offers one of them, §7.2), so that two of one name can be picked; picks are counted
 * on the confirm button. Finding nothing is the second button
 * ([CR 701.23b](https://yawgatog.com/resources/magic-rules/#R70123b): "that player isn't
 * required to find some or all of those cards"); a lone card found is picked to begin with. */
function search(at: Situation): Pick {
  const { most: max, words } = at.prompt;
  const copies = at.offeredAmong(at.library("you"));
  const cards = (copies.length >= at.offered.length ? copies : at.offered)
    .slice()
    .sort(
      (a, b) => (at.name(a) ?? "").localeCompare(at.name(b) ?? "") || a - b,
    );
  const most = Math.min(max, cards.length);
  return {
    kind: "pick",
    key: at.key,
    title: at.prompt.title,
    source: at.source,
    cards,
    pickable: cards,
    min: 1,
    max: most,
    confirm: (n) => counted(most > 1 ? words.counted : words.confirm, n, most),
    decline: words.decline,
    picked: cards.length === 1 ? cards : [],
    plan: (picked) => [
      ...picked.map((slot) => at.pickCard("chooseUpTo", slot)),
      // Not asked once the search has as many as it may take, or nothing is left to find.
      { ...done("chooseUpTo"), optional: true },
    ],
  };
}

/** A card among those the decision offers, laid out: the top cards of a library the prompt
 * counts, as the two of a manifest dread
 * ([CR 701.62a](https://yawgatog.com/resources/magic-rules/#R70162a): one goes onto the
 * battlefield face down, the other to the graveyard), or else the cards offered. */
function chooseObject(at: Situation): Pick {
  const { from, count, words, purpose } = at.prompt;
  const cards = from?.zone === "library" && count > 0 ? at.shown : at.offered;
  return {
    kind: "pick",
    key: at.key,
    title: at.prompt.title,
    source: at.source,
    cards,
    pickable: at.offeredAmong(cards),
    min: 1,
    max: 1,
    confirm: (n) => counted(words.confirm, n, 1),
    plan: ([slot]) =>
      slot === undefined ? [] : [at.pickCard("chooseObject", slot, purpose)],
  };
}

/** Cards from the graveyard to exile for a cost, as "exile six cards from
 * your graveyard" ([CR 601.2h](https://yawgatog.com/resources/magic-rules/#R6012h)). The
 * engine asks for them from the bottom of the graveyard up, each above the one before, of
 * cards with one name the lowest, and takes them unasked once one answer is left: the plan
 * follows it, asking for each card the engine will ask for. */
function graveyardExile(at: Situation): Pick {
  const count = at.prompt.most;
  const graveyard = at.view.players.you.graveyard.filter(
    (slot) => !at.view.objects.find((o) => o.slot === slot)?.token,
  );
  return {
    kind: "pick",
    key: at.key,
    title: at.prompt.title,
    source: at.source,
    cards: [...graveyard].reverse(),
    pickable: graveyard,
    min: count,
    max: count,
    confirm: (n) => counted(at.prompt.words.confirm, n, count),
    plan(picked) {
      const need = new Map<string, number>();
      for (const slot of picked) {
        const name = at.name(slot) ?? "";
        need.set(name, (need.get(name) ?? 0) + 1);
      }
      const left = [...graveyard];
      const picks: Intent[] = [];
      for (let from = 0, still = count; still > 0; still -= 1) {
        const window = left.slice(from, left.length - still + 1);
        const names = new Set(window.map((s) => at.name(s) ?? ""));
        const i = window.findIndex(
          (s) => (need.get(at.name(s) ?? "") ?? 0) > 0,
        );
        if (i < 0) break;
        const name = at.name(window[i]!) ?? "";
        // The lowest card of that name is the one the engine offers and exiles.
        const first = window.findIndex((s) => (at.name(s) ?? "") === name);
        if (names.size > 1)
          picks.push(at.pickCard("chooseObject", window[first]!, "exile-cost"));
        need.set(name, need.get(name)! - 1);
        left.splice(from + first, 1);
        from += first;
      }
      return picks;
    },
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
    title: at.prompt.title,
    cards,
    pickable: cards,
    min: 1,
    max: 1,
    confirm: (n) => counted(at.prompt.words.confirm, n, 1),
    plan: ([slot]) =>
      slot === undefined
        ? []
        : [at.pickCard("chooseObject", slot, "legend-rule")],
  };
}

/** "As this land enters, you may pay N life. If you don't, it enters tapped": asked of the
 * land by name. Paying is the default only when the land's mana would make a card in hand
 * castable this turn that the untapped lands cannot. */
function payLife(at: Situation): Ask {
  const { words, purpose } = at.prompt;
  const pay = { label: words.yes, plan: [{ ...yes("yesNo"), purpose }] };
  const tapped = { label: words.no, plan: [{ ...no("yesNo"), purpose }] };
  return {
    kind: "ask",
    key: at.key,
    title: at.prompt.title,
    cards: [],
    answers: at.untappedLandWanted(at.source?.slot)
      ? [pay, tapped]
      : [tapped, pay],
  };
}

/** A yes or no in the prompt's own words, over the cards it is about, if any: the top card of
 * a library that may go to the bottom, a revealed card that may go to the graveyard
 * ([CR 701.44a](https://yawgatog.com/resources/magic-rules/#R70144a): "that player ... may
 * put the revealed card into their graveyard"), a spell that goes on top of its owner's
 * library or the bottom, or whether to search at all (asked only of a player who knows cards
 * in their library: declining keeps them where they are, unshuffled). */
function yesNo(at: Situation): Ask {
  const { words, about, purpose } = at.prompt;
  return {
    kind: "ask",
    key: at.key,
    title: at.prompt.title,
    // Where each answer sends the card, neither is the one to press.
    equalAnswers: EQUAL_ANSWERS.has(purpose) || undefined,
    source: at.source,
    cards: about,
    answers: at.answers((o, label) =>
      o.kind === "yes" ? words.yes : o.kind === "no" ? words.no : label,
    ),
  };
}

/** The yes-or-no purposes whose answers are two places for one card. */
const EQUAL_ANSWERS = new Set(["fateseal", "top-or-bottom"]);

/** A card a resolved ability lets its controller cast now, without paying its mana cost
 * ([CR 608.2g](https://yawgatog.com/resources/magic-rules/#R6082g)). The ability has finished
 * resolving, so the decision names no source. */
function castOffer(at: Situation): Ask {
  const casts = at.decision.candidates.filter((c) => c.option.kind === "cast");
  const { words } = at.prompt;
  return {
    kind: "ask",
    key: at.key,
    title: at.prompt.title,
    cards: [],
    answers: at.answers((o, label) =>
      o.kind === "pass"
        ? words.decline
        : o.kind === "cast" && casts.length === 1
          ? words.confirm
          : label,
    ),
  };
}

/** The value of X as a spell is cast ([CR 601.2b](https://yawgatog.com/resources/magic-rules/#R6012b)),
 * a kicker's X among them, each by the total cost it makes
 * ([CR 601.2f](https://yawgatog.com/resources/magic-rules/#R6012f)). */
function chooseX(at: Situation): Ask {
  return {
    kind: "ask",
    key: at.key,
    title: at.prompt.title,
    source: at.source,
    cards: [],
    answers: at.decision.candidates.map((c) => {
      const cost = c.payments[0]?.cost;
      return {
        label: cost ? `${c.label} · ${cost}` : c.label,
        plan: [answer(at.decision.kind, c.option)],
      };
    }),
  };
}
