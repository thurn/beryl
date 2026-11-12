// The card records the page holds: what the card store (data/card-store.tsv, served by the
// `card_records` command) says of each name the page has asked about. The page bundles no card
// data. It asks for the names a game update shows before anything draws the update, so the
// lookups in cards.ts stay synchronous. Plain TypeScript with no page under it, which Node runs
// as it is (scripts/card-records.test.mjs).
import type { CardEntry, CardRecords, ObjectView } from "./generated/protocol";

/** Answers `names` with the records the store has for them: at once where they are in hand (a
 * fixture's), else when the server answers. */
export type Ask = (names: string[]) => CardRecords | Promise<CardRecords>;

/** Every string in `value`, whatever it says: a card's name among them wherever a view, its
 * events, its prompt or its script carries one, with no list of those places to fall behind
 * the protocol. A recorded fixture's records are found the same way
 * (beryl_session's `card_records::fixture_message`). */
export function strings(value: unknown, found = new Set<string>()) {
  if (typeof value === "string") {
    found.add(value);
  } else if (Array.isArray(value)) {
    for (const item of value) strings(item, found);
  } else if (typeof value === "object" && value !== null) {
    for (const item of Object.values(value)) strings(item, found);
  }
  return found;
}

type Named = Pick<ObjectView, "token" | "name">;

/** Whether `o` is a token with no name: a copy of a face-down permanent, a 2/2 with no name
 * of its own. */
export const isNamelessToken = (o: Named) => o.token && o.name === "";

export class CardRecordCache {
  /** By the name asked for: the card it names and which of its faces, or the token where no
   * card has the name. */
  private readonly named = new Map<string, { card: CardEntry; face: number }>();
  private readonly tokens = new Map<string, CardEntry>();
  /** The names the store has answered, with a record or with none. */
  private readonly answered = new Set<string>();
  /** The names asked and not yet answered, each with its request. */
  private readonly asking = new Map<string, Promise<void>>();
  /** What `whenCached` holds back, in order. */
  private held = 0;
  private last: Promise<void> = Promise.resolve();
  private back: string | undefined;
  private nameless: string | undefined;

  /** The card and face a card or face name names, or the token of that name where no card
   * has it; none for a name with no record, or not asked about yet. */
  find(name: string) {
    return this.named.get(name);
  }

  /** The token named `name`, which a card may be too (a card and the offspring token that
   * copies it). */
  token(name: string) {
    return this.tokens.get(name);
  }

  /** The image of Magic's card back, once any answer has arrived. */
  cardBack() {
    return this.back;
  }

  /** The art `o` is drawn with if it is a token with no name, once any answer has arrived:
   * no card's record draws one. Undefined for anything else, which its name finds. */
  namelessArt(o: Named) {
    return isNamelessToken(o) ? this.nameless : undefined;
  }

  /** Keeps the store's answer for `names`. */
  keep(names: Iterable<string>, records: CardRecords) {
    this.back = records.card_back || this.back;
    this.nameless = records.nameless_token || this.nameless;
    for (const name of names) {
      this.answered.add(name);
      // The card last, so it wins over a token of the same name.
      for (const [card, token] of [
        [records.tokens[name], true],
        [records.cards[name], false],
      ] as const) {
        if (!card) continue;
        const face = Math.max(
          0,
          card.faces.findIndex((f) => f.name === name),
        );
        this.named.set(name, { card, face });
        if (token) this.tokens.set(name, card);
      }
    }
  }

  /** Asks for those of `names` not yet asked about, in one request. Returns nothing when
   * every one is answered already, else a promise that settles once they all are, the ones
   * an earlier request is still fetching among them. A request that fails leaves its names
   * unanswered, to be asked again: until then they draw as names with no record do. */
  load(names: Iterable<string>, ask: Ask): Promise<void> | undefined {
    const waits = new Set<Promise<void>>();
    const missing: string[] = [];
    for (const name of names) {
      if (this.answered.has(name)) continue;
      const asked = this.asking.get(name);
      if (asked) waits.add(asked);
      else missing.push(name);
    }
    if (missing.length > 0) {
      const answer = ask(missing);
      if (answer instanceof Promise) {
        const kept = answer
          .then(
            (records) => this.keep(missing, records),
            () => undefined,
          )
          .finally(() => missing.forEach((name) => this.asking.delete(name)));
        missing.forEach((name) => this.asking.set(name, kept));
        waits.add(kept);
      } else {
        this.keep(missing, answer);
      }
    }
    return waits.size > 0
      ? Promise.all(waits).then(() => undefined)
      : undefined;
  }

  /** Runs `run` once the record of every name `shown` carries is cached: now, in the caller's
   * own tick, when none is missing and nothing earlier is still held back, else when its
   * request is answered. Runs are in the order they were asked for. */
  whenCached(shown: unknown, ask: Ask, run: () => void) {
    const loading = this.load(strings(shown), ask);
    if (!loading && this.held === 0) {
      run();
      return;
    }
    this.held += 1;
    this.last = this.last
      .then(() => loading)
      .then(() => {
        this.held -= 1;
        try {
          run();
        } catch (error) {
          // The runs behind this one still happen; the error is the page's, as it would be.
          queueMicrotask(() => {
            throw error;
          });
        }
      });
  }
}

/** The page's records. */
export const cardRecords = new CardRecordCache();
