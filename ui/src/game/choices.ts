// The choose-or-order decisions the game asks over the table, in one overlay
// (docs/ui_mockups/scry.png): what it shows, what it asks, and the answers it sends. The engine
// asks one pick at a time (a scry is a card for the bottom, then another, then done, then the
// order of the rest), so the overlay takes the whole choice at once and plans the picks that
// say it, which the game screen sends as each decision arrives. Pure functions of the view.
import { cardEntry, faceEntry, landFace } from "../cards";
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

/** Whether a choice is asked in the floating prompt over the table, as a title and a row of
 * buttons, rather than in the overlay over the dimmed table: a question with no cards. */
export const floats = (choice: Choice): choice is Ask =>
  choice.kind === "ask" && choice.cards.length === 0;

/** What the open decision asks, as the overlay shows it; none for a decision made on the board
 * (priority, targets, combat, mana) or on the opening screen. */
export function choiceOf(view: PlayerView): Choice | undefined {
  if (view.status.kind !== "deciding") return undefined;
  const at = new Situation(view, view.status.decision);
  const ask = at.decision.ask;
  switch (ask.kind) {
    case "attackTarget":
      return undefined;
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
    case "topOrBottom":
      return topOrBottom(at, ask);
    case "explore":
      return explore(at);
    // Whether to move one card: a small question, asked on the board.
    case "mayMove":
      return undefined;
    case "search":
      return search(at, ask);
    case "maySearch":
      return maySearch(at);
    // Untaps and choices of cards from the hand (discards, a card exiled for a cost: the hand
    // rises to pick them, game/tablePick.ts), crews, waterbends and payments (the board
    // lights what pays) are decided on the board.
    case "untap":
    case "discardBasicLand":
    case "cleanup":
    case "crew":
    case "waterbend":
    case "unlessPays":
      return undefined;
    case "payLife":
      return payLife(at, ask);
    case "manifestDread":
      return manifestDread(at);
    case "exileCards":
      return ask.zone === "graveyard"
        ? graveyardExile(at, ask.count)
        : undefined;
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
    case "chooseObject":
      return small(at) ? undefined : chooseObject(at);
    case "yesNo":
      return yesNo(at);
    case "legendRule":
      return small(at) ? undefined : legendRule(at);
    case "castOffer":
      return castOffer(at);
    case "x":
      return chooseX(at);
    default:
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
function scry(at: Situation, { count }: AskOf<"scry">): Arrange {
  const ordering = at.decision.kind === "chooseObject";
  // Top first; a card the engine collapsed with an identical one still shows.
  const cards = at.offeredAmong(at.looked(count));
  const shown = cards.length > 0 ? cards : at.offered;
  return {
    kind: "arrange",
    key: at.key,
    title: "Scry",
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
function putBack(at: Situation, { count }: AskOf<"putBack">): Stack {
  const hand = at.hand("you");
  return {
    kind: "stack",
    key: at.key,
    title: "Put Back",
    source: at.source,
    hand,
    count: Math.min(count, hand.length),
    plan: (onto) => onto.map((slot) => at.pickCard("chooseObject", slot)),
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

/** Whether the top card of a player's library goes to the bottom. */
function fateseal(at: Situation, { player }: AskOf<"fateseal">): Ask {
  const top = at.library(player)[0];
  return {
    kind: "ask",
    key: at.key,
    title: "Top of Library",
    equalAnswers: true,
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
    equalAnswers: true,
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
  // A card is nearly always worth taking: the one card that can be is picked to begin with,
  // and taking nothing is the second button.
  return {
    kind: "pick",
    key: at.key,
    title,
    source: at.source,
    cards,
    pickable,
    min: 1,
    max: 1,
    confirm: () => verb,
    decline: asking
      ? taking
        ? "Take Nothing"
        : `${capitalized(to)} Nothing`
      : undefined,
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

/** An explore ([CR 701.44a](https://yawgatog.com/resources/magic-rules/#R70144a)): "that
 * permanent's controller reveals the top card of their library ... Otherwise, that player
 * puts a +1/+1 counter on the exploring permanent and may put the revealed card into their
 * graveyard." The revealed card is shown, each answer named by where it goes. */
function explore(at: Situation): Ask {
  const top = at.library("you")[0];
  return {
    kind: "ask",
    key: at.key,
    title: "Revealed",
    source: at.source,
    cards: top === undefined ? [] : [top],
    answers: [
      { label: "Graveyard", plan: [yes("yesNo")] },
      { label: "Keep on Top", plan: [no("yesNo")] },
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
function search(at: Situation, { max }: AskOf<"search">): Pick {
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
    title: "Search",
    source: at.source,
    cards,
    pickable: cards,
    min: 1,
    max: most,
    confirm: (n) => (most > 1 ? `Find ${n}/${most}` : "Find"),
    decline: "Find Nothing",
    picked: cards.length === 1 ? cards : [],
    plan: (picked) => [
      ...picked.map((slot) => at.pickCard("chooseUpTo", slot)),
      // Not asked once the search has as many as it may take, or nothing is left to find.
      { ...done("chooseUpTo"), optional: true },
    ],
  };
}

/** Whether to search at all, asked only of a player who knows cards in their library (the
 * session searches for the others): declining keeps them where they are, unshuffled. */
function maySearch(at: Situation): Ask {
  return {
    kind: "ask",
    key: at.key,
    title: "Search?",
    source: at.source,
    cards: [],
    answers: [
      { label: "Search", plan: [yes("yesNo")] },
      { label: "Don't Search", plan: [no("yesNo")] },
    ],
  };
}

function chooseObject(at: Situation): Pick {
  return {
    kind: "pick",
    key: at.key,
    title: "Choose a Card",
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

/** Manifest dread ([CR 701.62a](https://yawgatog.com/resources/magic-rules/#R70162a)): one
 * of the top two cards goes onto the battlefield face down, the other to the graveyard. */
function manifestDread(at: Situation): Pick {
  const cards = at.looked(2);
  return {
    kind: "pick",
    key: at.key,
    title: "Manifest Dread",
    source: at.source,
    cards,
    pickable: at.offeredAmong(cards),
    min: 1,
    max: 1,
    confirm: () => "Manifest",
    plan: ([slot]) =>
      slot === undefined ? [] : [at.pickCard("chooseObject", slot)],
  };
}

/** Cards from the graveyard to exile for a cost, as "exile six cards from
 * your graveyard" ([CR 601.2h](https://yawgatog.com/resources/magic-rules/#R6012h)). The
 * engine asks for them from the bottom of the graveyard up, each above the one before, of
 * cards with one name the lowest, and takes them unasked once one answer is left: the plan
 * follows it, asking for each card the engine will ask for. */
function graveyardExile(at: Situation, count: number): Pick {
  const graveyard = at.view.players.you.graveyard;
  return {
    kind: "pick",
    key: at.key,
    title: "Exile",
    source: at.source,
    cards: [...graveyard].reverse(),
    pickable: graveyard,
    min: count,
    max: count,
    confirm: () => "Exile",
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
          picks.push(at.pickCard("exileCost", window[first]!));
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
    title: "Legend Rule",
    cards,
    pickable: cards,
    min: 1,
    max: 1,
    confirm: () => "Keep",
    plan: ([slot]) =>
      slot === undefined ? [] : [at.pickCard("legendRule", slot)],
  };
}

/** "As this land enters, you may pay N life. If you don't, it enters tapped": asked of the
 * land by name. Paying is the default only when the land's mana would make a card in hand
 * castable this turn that the untapped lands cannot. */
function payLife(at: Situation, { life }: AskOf<"payLife">): Ask {
  const name = at.source?.name;
  const land = name ? landFace(name) : undefined;
  const pay = { label: `Pay ${life} Life`, plan: [yes("payLife")] };
  const tapped = { label: "Tapped", plan: [no("payLife")] };
  return {
    kind: "ask",
    key: at.key,
    title: land?.name ?? name ?? `Pay ${life} Life?`,
    cards: [],
    answers: at.untappedLandWanted(at.source?.slot)
      ? [pay, tapped]
      : [tapped, pay],
  };
}

function yesNo(at: Situation): Ask {
  return {
    kind: "ask",
    key: at.key,
    title: at.resolvingName ?? "Choose",
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
    cards: [],
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
 * a kicker's X among them, each by the total cost it makes
 * ([CR 601.2f](https://yawgatog.com/resources/magic-rules/#R6012f)). */
function chooseX(at: Situation): Ask {
  return {
    kind: "ask",
    key: at.key,
    title: "Choose X",
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
