// A view as the beats a player can follow. The session sends a view at each step and before
// each of the opponent's decisions, so one view often carries a whole sequence: the opponent
// casts a spell that resolves unanswered, activates a planeswalker that mills and draws, or
// counters the spell on the stack. Shown at once, all of it lands in a blink. Here a view's
// events are played forward from the view before, in order, and cut where the eye needs a
// beat: a spell or ability shown on the stack before it resolves, its effects before it
// leaves, a spell struck before it is carried off, cards from a library, hand or graveyard
// shown face up before they land on a pile. Each beat is a view of its own, held as long as
// its events need (`HOLD`); the last is the view itself. Every card and every event kind
// gets the same beats: none of this knows a card.
import type {
  Counter,
  GameEvent,
  GameUpdate,
  ObjectView,
  PlayerView,
  Side,
  StackItem,
  ZoneName,
} from "../generated/protocol";
import { choiceOf, choiceSlots } from "./choices";
import { pendingAttackers } from "./combat";
import { withResolving } from "./decisions";
import { liftsHandling, sameHandling, type Spotlight } from "./layout";
import { FLIGHT_HOLD } from "./flight";

/** How long a view holds the screen before the next one, in milliseconds: a step passing
 * briefly, a new turn while it is announced, an action long enough to follow, a spell or
 * ability the opponent puts on the stack long enough to read before it resolves unanswered, a
 * counter longest; and the game's last view a beat more before the result covers it, so the
 * play that ended it is seen. */
export const HOLD = {
  step: 300,
  turn: 1100,
  action: 900,
  trigger: 1300,
  permanentLanding: FLIGHT_HOLD,
  stack: 2000,
  counter: 1800,
  ending: 1600,
  /** A deal or collection can stagger its card flights by up to 300 ms. */
  deal: 1000,
  /** A single card crossing from the opponent's library to their hand. */
  draw: 700,
  shuffle: 420,
  /** Time for the face-down group to lift before an automated answer can take it away. */
  handling: 850,
  /** A spell or ability resolving against another spell, held on the stack with its arrow to
   * its target before the target is hit. */
  resolving: 900,
  /** The spell it hit, struck (CardLayer's shake) and then carried to its zone, in a view of
   * its own so that the eye follows it. */
  hit: 1500,
  /** A card drawn and put elsewhere by the same resolution (a loot's discard), shown arriving
   * in its owner's hand before it leaves. */
  drawn: 700,
  /** Time for a permanent's tap rotation to finish before its departure. */
  tapBeforeMove: 450,
  /** Time to see the mana sources turn and their mana reach the activation source. */
  waterbendMana: 520,
  /** Cards milled, discarded by the opponent or moved between piles, shown face up in a row
   * before they land: all of one move at once, so six cards take no longer than one. */
  showcase: 1100,
  /** What follows a showcase: the cards flying to their piles. */
  landing: 600,
};

/** A view cut out of another: the table part way through its events. */
export interface Beat {
  update: GameUpdate;
  /** How long it holds, when its events alone would not say. */
  hold?: number;
  /** Cards shown apart from where they lie: face up over the table, or just drawn. */
  spotlight?: Spotlight;
  /** Stack entries whose trigger flight starts in this beat. */
  triggerFlights?: ReadonlySet<number>;
}

/** A pending cost can spend previously floating mana without emitting a new mana event. */
function spentPool(before: PlayerView, after: PlayerView): boolean {
  return (["you", "opponent"] as const).some((side) =>
    Object.entries(before.players[side].mana).some(
      ([color, amount]) =>
        amount >
        after.players[side].mana[color as keyof typeof after.players.you.mana],
    ),
  );
}

/** `update` as the beats a player can follow from `before`, the view last shown or queued; with
 * none, or nothing worth a beat, it is one beat, itself. */
export function beats(
  before: GameUpdate | undefined,
  update: GameUpdate,
): Beat[] {
  let prior = before?.view.stack ?? [];
  return cutBeats(before, update).map((beat) => {
    const { view } = beat.update;
    const flights = new Set<number>();
    if (!view.undone) {
      for (const [index, item] of view.stack.entries()) {
        const old = prior[index];
        if (
          item.kind === "ability" &&
          item.triggered &&
          (!old || stackKey(index, old) !== stackKey(index, item))
        )
          flights.add(index);
      }
    }
    prior = view.stack;
    return flights.size ? { ...beat, triggerFlights: flights } : beat;
  });
}

function cutBeats(before: GameUpdate | undefined, update: GameUpdate): Beat[] {
  const { view } = update;
  const reveal = new Set<number>();
  if (before) {
    const faceDown = new Set(
      before.view.objects
        .filter((o) => o.zone === "battlefield" && o.facing === "down")
        .map((o) => o.slot),
    );
    for (const o of view.objects) {
      if (o.zone === "battlefield" && o.facing === "up" && faceDown.has(o.slot))
        reveal.add(o.slot);
    }
    for (const e of view.events) {
      if (e.kind === "zoneChange") reveal.delete(e.slot);
    }
  }
  const poolBeforeChoice =
    before && choiceOf(view) && spentPool(before.view, view);
  if (
    before &&
    !poolBeforeChoice &&
    reveal.size === 0 &&
    sameHandling(before.view, view) &&
    view.events.length === 0
  )
    return [{ update, hold: 0 }];
  if (
    !before ||
    view.undone ||
    (view.events.length === 0 && !poolBeforeChoice && reveal.size === 0)
  )
    return [{ update }];
  return new Walk(withResolving(before.view), update).run(reveal);
}

/** The resolving counterspell follows its target to the graveyard in the same beat. */
export function counterDeparture(hit: GameEvent, next?: GameEvent) {
  return hit.kind === "zoneChange" &&
    hit.from === "stack" &&
    hit.to === "graveyard" &&
    hit.reason === "counter" &&
    next?.kind === "zoneChange" &&
    next.from === "stack" &&
    next.to === "graveyard" &&
    next.reason === "resolve"
    ? next
    : undefined;
}

/** The source and its immediate returns travel together when the exile ends. */
export function linkedReturnFlights(
  before: PlayerView | undefined,
  view: PlayerView,
): ReadonlySet<number> {
  const flights = new Set<number>();
  if (!before || view.undone) return flights;
  const leaving = new Set(
    view.events.flatMap((e) =>
      e.kind === "zoneChange" &&
      e.from === "battlefield" &&
      e.to !== "battlefield"
        ? [e.slot]
        : [],
    ),
  );
  if (!leaving.size) return flights;
  for (const e of view.events) {
    if (
      e.kind !== "zoneChange" ||
      e.from !== "exile" ||
      e.to !== "battlefield" ||
      e.reason !== "return"
    )
      continue;
    const source = before.objects.find((o) => o.slot === e.slot)?.exiledBy;
    if (source != null && leaving.has(source)) {
      flights.add(source);
      flights.add(e.slot);
    }
  }
  return flights;
}

/** Whether a spell or ability is still on `stack`. */
function onStack(stack: StackItem[], item: StackItem): boolean {
  return stack.some((s) => sameItem(s, item));
}

export function sameItem(a: StackItem, b: StackItem): boolean {
  if (a.kind === "spell" && b.kind === "spell") return a.slot === b.slot;
  return a.kind === b.kind && JSON.stringify(a) === JSON.stringify(b);
}

/** Targets can change while an item is on the stack without starting another flight. */
export const stackKey = (index: number, item: StackItem) =>
  item.kind === "spell"
    ? `${index}:${item.slot}`
    : `${index}:${item.name}:${item.source}:${item.ability}:${item.granted}:${item.controller}:${item.triggered}`;

/** A cast's spell as the stack showed it. An event recorded before casts carried their spell
 * says only its card. */
function castItem(e: Extract<GameEvent, { kind: "cast" }>): StackItem {
  return e.item ?? { kind: "spell", slot: e.slot, targets: [], x: 0, paid: [] };
}

/** A spell or ability whose announcement the viewer sees in this event. */
export function announcedItem(e: GameEvent): StackItem | undefined {
  return e.kind === "cast"
    ? castItem(e)
    : e.kind === "ability"
      ? e.item
      : undefined;
}

/** Zones whose cards the viewer may not see: a library, and the opponent's hand. */
function hiddenTo(zone: ZoneName, owner: Side): boolean {
  return zone === "library" || (zone === "hand" && owner === "opponent");
}

/** Give newly visible cards a moment to be read on their way to a face-up pile. */
function hops(from: ZoneName, to: ZoneName, owner: Side): boolean {
  return (
    from !== to &&
    (to === "graveyard" || to === "exile") &&
    hiddenTo(from, owner)
  );
}

/** Whether a card moving from `from` to `to` goes between two face-up piles: a graveyard and
 * exile, either way. */
function crosses(from: ZoneName, to: ZoneName): boolean {
  const open: ZoneName[] = ["graveyard", "exile"];
  return from !== to && open.includes(from) && open.includes(to);
}

/** The opponent's spell or ability on the stack, held long enough to read; a trigger a little
 * less; the player's own, which they chose, an action's beat. */
function stackHold(item: StackItem): number {
  const controller = item.kind === "spell" ? undefined : item.controller;
  if (item.kind === "ability" && item.triggered) return HOLD.trigger;
  return controller === "you" ? HOLD.action : HOLD.stack;
}

/** Boundaries within a batched opponent Waterbend: benders turn, then mana sources
 * turn, then the paid ability lifts from its source. Mana source slots come from the
 * events so a creature or artifact used for mana is counted with the mana sources. */
function waterbendCuts(events: GameEvent[]) {
  const cuts = new Map<number, { hold: number; payingSource?: number }>();
  for (const [end, event] of events.entries()) {
    if (
      event.kind !== "ability" ||
      event.item.kind !== "ability" ||
      event.item.controller !== "opponent" ||
      !event.item.waterbend
    )
      continue;
    let start = end;
    while (start > 0) {
      const prior = events[start - 1]!;
      if (
        prior.kind !== "tap" &&
        prior.kind !== "mana" &&
        !(prior.kind === "zoneChange" && prior.reason === "sacrifice")
      )
        break;
      start--;
    }
    if (start === end) continue;
    const payment = events.slice(start, end);
    const sources = new Set(
      payment.flatMap((e) => (e.kind === "mana" ? [e.source] : [])),
    );
    const firstMana = payment.findIndex(
      (e) => e.kind === "mana" || (e.kind === "tap" && sources.has(e.slot)),
    );
    if (
      firstMana > 0 &&
      payment.slice(0, firstMana).some((e) => e.kind === "tap")
    )
      cuts.set(start + firstMana, { hold: HOLD.tapBeforeMove });
    cuts.set(end, {
      hold: sources.size ? HOLD.waterbendMana : HOLD.tapBeforeMove,
      payingSource: event.item.source ?? undefined,
    });
  }
  return cuts;
}

/** A view's events played forward from the view before it, cut into beats. */
class Walk {
  private readonly view: PlayerView;
  private readonly final: Map<number, ObjectView>;
  private cur: PlayerView;
  private readonly aiming: Set<number>;
  private readonly nextAiming: Set<number>;
  private pending: GameEvent[] = [];
  private readonly out: Beat[] = [];
  /** Slots drawn in this view so far, to be seen in a hand before they leave it. */
  private readonly drawn = new Set<number>();
  private readonly seenInChoice: Set<number>;
  private readonly directToGraveyard = new Set<number>();
  private readonly startsHandling: boolean;
  /** The beat cut last showed cards in a showcase: the next one lands them. */
  private landing = false;

  constructor(
    before: PlayerView,
    private readonly update: GameUpdate,
  ) {
    const choice = choiceOf(before);
    this.seenInChoice = new Set(choice ? choiceSlots(choice) : []);
    const handling = before.handling;
    if (handling?.zone === "library" && handling.count) {
      // The other seat also saw these cards lift. Their departure can continue
      // from that group instead of opening a separate showcase.
      for (const slot of before.players[handling.side].library.slice(
        -handling.count,
      ))
        this.directToGraveyard.add(slot);
    }
    this.view = update.view;
    this.aiming = new Set(pendingAttackers(before));
    this.nextAiming = new Set(pendingAttackers(this.view));
    this.startsHandling =
      liftsHandling(this.view) &&
      (!sameHandling(before, this.view) ||
        (this.view.handling?.zone === "library" &&
          this.view.handling.scry === true &&
          this.view.events.some(
            (e) => e.kind === "scry" && e.player === "opponent",
          )));
    this.final = new Map(this.view.objects.map((o) => [o.slot, o]));
    // A scripted or automatic choice may leave no visible picker. The card
    // put into the graveyard alongside a manifest still travels there directly.
    for (let i = 1; i < this.view.events.length; i++) {
      const first = this.view.events[i - 1]!;
      const next = this.view.events[i]!;
      if (
        first.kind === "zoneChange" &&
        first.from === "library" &&
        first.to === "battlefield" &&
        first.reason === "effect" &&
        this.final.get(first.slot)?.facing === "down" &&
        next.kind === "zoneChange" &&
        next.from === "library" &&
        next.to === "graveyard" &&
        next.reason === "effect" &&
        this.final.get(first.slot)?.owner === this.final.get(next.slot)?.owner
      ) {
        this.directToGraveyard.add(next.slot);
      }
    }
    this.cur = { ...before, status: { kind: "playing" }, events: [] };
  }

  run(reveal: ReadonlySet<number>): Beat[] {
    const events = this.view.events;
    const payments = waterbendCuts(events);
    const marksAbilityResolution = events.some(
      (e) => e.kind === "abilityFinished",
    );
    for (let i = 0; i < events.length; i++) {
      const e = events[i]!;
      const payment = payments.get(i);
      if (payment) {
        const { hold, payingSource } = payment;
        this.cut(
          hold,
          payingSource === undefined ? undefined : { payingSource },
        );
      }
      if (e.kind === "stepBegins") {
        // Untap has no priority of its own, so its events arrive with upkeep.
        const leavingUntap = this.cur.turn.step === "untap";
        if (leavingUntap) this.cut(HOLD.turn);
        if (e.step === "untap") this.cut();
        if (e.step === "untap" || leavingUntap) {
          this.cur = {
            ...this.cur,
            turn: { ...this.view.turn, active: e.active, step: e.step },
          };
        }
      }
      if (
        e.kind === "zoneChange" &&
        e.from !== e.to &&
        this.pending.some((pending) =>
          announcedItem(pending)?.targets.some(
            (target) => target.kind === "object" && target.slot === e.slot,
          ),
        )
      ) {
        // The spell may still be resolving at the view's end (a search after an exile).
        // Show the announcement before its target moves, even while the spell stays.
        this.cut();
      }
      if (
        e.kind === "zoneChange" &&
        e.from === "battlefield" &&
        this.pending.some((p) => p.kind === "tap" && p.slot === e.slot)
      ) {
        // The same physical card turns on the battlefield before it flies away.
        this.cut(HOLD.tapBeforeMove);
      }
      if (e.kind === "zoneChange" && e.from === "battlefield") {
        this.revealDepartures(events, i);
      }
      if (
        e.kind === "zoneChange" &&
        e.from === "battlefield" &&
        e.to !== "battlefield" &&
        e.to !== "nowhere" &&
        this.cur.objects.some((o) => o.slot === e.slot && o.token) &&
        !this.final.has(e.slot)
      ) {
        // Tokens reach the destination before ceasing to exist (CR 704.5d).
        // The final view omits them, so retain their physical card for the flight.
        this.cut();
        this.apply(e);
        this.pending.push(e);
        this.cut(HOLD.action);
        continue;
      }
      if (e.kind === "libraryShuffled") {
        this.cut();
        // A wheel can shuffle both libraries before dealing either of the new hands.
        const shuffle = new Set<Side>();
        let end = i;
        while (end < events.length) {
          const event = events[end]!;
          if (event.kind !== "libraryShuffled") break;
          shuffle.add(event.player);
          this.pending.push(event);
          end++;
        }
        const handling = this.cur.handling;
        if (handling && shuffle.has(handling.side) && liftsHandling(this.cur)) {
          // Put any lifted cards back first. The shuffle breaks the correspondence
          // to any known faces.
          const shuffles = this.pending;
          this.pending = [];
          this.cur = { ...this.cur, handling: undefined };
          this.cut(HOLD.handling, undefined, true);
          this.pending = shuffles;
        }
        const deals = new Map<Side, number[]>();
        for (let j = end; j < events.length; j++) {
          const next = events[j]!;
          if (next.kind !== "draw") break;
          const slots = deals.get(next.player) ?? [];
          slots.push(...(next.slots ?? []));
          deals.set(next.player, slots);
        }
        for (const side of shuffle) this.shuffle(side, deals.get(side) ?? []);
        this.cut(HOLD.shuffle, { shuffle });
        this.cut(HOLD.shuffle, undefined, true);
        i = end - 1;
        continue;
      }
      if (
        e.kind === "zoneChange" &&
        e.to === "library" &&
        e.from !== "library"
      ) {
        let end = i + 1;
        while (end < events.length) {
          const next = events[end]!;
          if (
            next.kind !== "zoneChange" ||
            next.to !== "library" ||
            next.from === "library"
          )
            break;
          end++;
        }
        const collecting = events[end]?.kind === "libraryShuffled";
        if (collecting || end > i + 1) {
          this.cut();
          for (const move of events.slice(i, end)) {
            this.apply(move, collecting);
            this.pending.push(move);
          }
          this.cut(HOLD.deal);
          i = end - 1;
          continue;
        }
      }
      if (e.kind === "zoneChange" && this.showcased(e)) {
        // Every card of the move at once: the events of one effect come together.
        const group = [e];
        while (i + 1 < events.length) {
          const next = events[i + 1]!;
          if (
            next.kind !== "zoneChange" ||
            next.from !== e.from ||
            next.to !== e.to ||
            !this.showcased(next)
          )
            break;
          group.push(next);
          i++;
        }
        this.cut();
        group.forEach((g) => this.apply(g));
        this.pending = group;
        // A full library travels straight to its destination. A row of that many faces
        // would be too small to read and conceal the stack they came from.
        if (group.length > 8) this.cut(HOLD.deal);
        else
          this.cut(HOLD.showcase, {
            shown: new Set(group.map((g) => g.slot)),
          });
        continue;
      }
      if (e.kind === "zoneChange" && crosses(e.from, e.to)) {
        // A card going from one face-up pile to another travels as it is, in a beat of its
        // own with the rest of its move, so that what it leads to (a token made for it)
        // follows rather than coming with it.
        this.cut();
        this.apply(e);
        this.pending.push(e);
        while (i + 1 < events.length) {
          const next = events[i + 1]!;
          if (next.kind !== "zoneChange" || !crosses(next.from, next.to)) break;
          this.apply(next);
          this.pending.push(next);
          i++;
        }
        this.cut(
          this.pending.filter(
            (e) => e.kind === "zoneChange" && e.to === "exile",
          ).length > 1
            ? 1000
            : HOLD.landing,
        );
        continue;
      }
      if (e.kind === "zoneChange" && this.struck(e)) {
        // The spell that hits it shown on the stack, then the hit in a beat of its own.
        if (this.pending.length > 0 || this.out.length === 0) {
          this.cut(HOLD.resolving, undefined, true);
        }
        this.apply(e);
        this.pending = [e];
        const leaving = counterDeparture(e, events[i + 1]);
        const resolving = this.cur.stack.at(-1);
        if (
          leaving &&
          resolving?.kind === "spell" &&
          resolving.slot === leaving.slot
        ) {
          this.apply(leaving);
          this.pending.push(leaving);
          i++;
        }
        this.cut(HOLD.hit);
        continue;
      }
      if (
        e.kind === "zoneChange" &&
        e.from === "hand" &&
        this.drawn.has(e.slot)
      ) {
        // A card drawn and put elsewhere by the same resolution arrives in the hand first.
        this.cut(HOLD.drawn);
        this.drawn.clear();
      }
      if (
        e.kind === "zoneChange" &&
        e.from === "stack" &&
        e.reason === "resolve" &&
        e.to !== "battlefield" &&
        this.pending.some(effect)
      ) {
        // What the spell did, while it is still on the stack, before it leaves.
        this.cut();
      }
      const trigger =
        e.kind === "ability" && e.item.kind === "ability" && e.item.triggered
          ? e.item
          : undefined;
      if (
        trigger &&
        this.pending.some(
          (pending) =>
            pending.kind === "zoneChange" &&
            pending.slot === trigger.source &&
            pending.to === "battlefield",
        )
      ) {
        // The permanent has to land before the trigger can visibly lift off it.
        this.cut();
      }
      if (e.kind === "abilityFinished") {
        this.finishAbility();
        continue;
      }
      if (trigger && this.pending.some(effect)) {
        // Older recordings have no resolution boundary. Effects between two
        // announcements can belong to a trigger that has already left the stack.
        const top = this.cur.stack.at(-1);
        if (
          !marksAbilityResolution &&
          top?.kind === "ability" &&
          !onStack(this.view.stack, top)
        )
          this.finishAbility();
        this.cut();
      }
      if (
        e.kind === "scry" &&
        e.player === "opponent" &&
        this.cur.handling?.zone === "library" &&
        this.cur.handling.side === "opponent" &&
        this.cur.handling.scry
      ) {
        this.cut();
        this.returnScry(e);
        continue;
      }
      if (e.kind === "draw" && e.cards > 1) this.cut();
      const drawing = this.drawn.size;
      this.apply(e);
      this.pending.push(e);
      if (e.kind === "resolutionPayment") {
        // Keep the paid-for item in view while mana reaches it, before resolution continues.
        this.put(e.item);
        this.cut(HOLD.action);
        continue;
      }
      if (e.kind === "draw") {
        // The opponent's hand lies at the table's edge: what they drew dips into view first.
        const drawn = new Set([...this.drawn].slice(drawing));
        const hold = e.cards > 1 ? HOLD.deal : HOLD.draw;
        if (e.player === "opponent" && drawn.size > 0)
          this.cut(hold, { drawn });
        else if (e.cards > 1) this.cut(hold);
        continue;
      }
      const item = announcedItem(e);
      if (item && !onStack(this.view.stack, item)) {
        // Gone by the view's end: shown on the stack before it resolves.
        this.cut(stackHold(item));
      }
    }
    if (reveal.size)
      this.showTurn(
        new Map([...reveal].map((slot) => [slot, this.final.get(slot)!])),
      );
    // A targeted ETB is on the stack before its targets are announced: its ability
    // event arrives in a later view. Keep that pending trigger off the landing beat too.
    const arrivingTrigger = this.view.stack.some(
      (item) =>
        item.kind === "ability" &&
        item.triggered &&
        !onStack(this.cur.stack, item) &&
        this.pending.some(
          (e) =>
            e.kind === "zoneChange" &&
            e.to === "battlefield" &&
            e.slot === item.source,
        ),
    );
    if (arrivingTrigger) this.cut();
    // Mana payment must be visible before an additional-cost chooser covers the table.
    // Its spell is still pending; confirming the chooser will not pay the mana again.
    if (
      choiceOf(this.view) &&
      (this.pending.some((e) => e.kind === "mana") ||
        spentPool(this.cur, this.view))
    ) {
      this.cur = {
        ...this.cur,
        players: {
          you: { ...this.cur.players.you, mana: this.view.players.you.mana },
          opponent: {
            ...this.cur.players.opponent,
            mana: this.view.players.opponent.mana,
          },
        },
      };
      this.cut(HOLD.action, undefined, true);
    }
    // A question asked over the table (a search, a scry) waits for what led to it to land: a
    // card exiled, a spell resolving, seen rather than moving under the overlay.
    if (
      (this.startsHandling && this.pending.length > 0) ||
      (this.pending.some(effect) && choiceOf(this.view))
    )
      this.cut();
    if (this.out.length === 0) return [{ update: this.update }];
    const last: GameUpdate = {
      ...this.update,
      view: { ...this.view, events: this.pending },
    };
    const deciding = this.view.status.kind === "deciding";
    return [
      ...this.out,
      {
        update: last,
        hold:
          this.landing && !deciding
            ? Math.max(HOLD.landing, this.startsHandling ? HOLD.handling : 0)
            : !this.startsHandling && this.view.handling
              ? eventsHold(last.view)
              : undefined,
      },
    ];
  }

  /** Land the top cards while the bottom packet approaches from below. */
  private returnScry(e: Extract<GameEvent, { kind: "scry" }>) {
    const before = this.cur.players.opponent.library;
    const after = this.view.players.opponent.library;
    const slots = new Set(before);
    const bottom = new Set(after.slice(0, e.bottom));
    // A later draw or shuffle can obscure the result in the final library.
    // Only use that order when the untouched middle still agrees.
    const ordered =
      before.length === after.length &&
      after.every((slot) => slots.has(slot)) &&
      before
        .filter((slot) => !bottom.has(slot))
        .slice(0, before.length - e.top - e.bottom)
        .every((slot, i) => slot === after[e.bottom + i]);
    this.cur = {
      ...this.cur,
      handling: undefined,
      players: {
        ...this.cur.players,
        opponent: {
          ...this.cur.players.opponent,
          library: ordered ? after : before,
        },
      },
    };
    this.pending.push(e);
    const tuck = ordered && bottom.size > 0;
    this.cut(HOLD.landing, tuck ? { scryBottom: bottom } : undefined);
    if (tuck && this.startsHandling) this.cut(HOLD.landing, undefined, true);
  }

  /** Reveal the cards leaving together (CR 708.9). Use the identity supplied
   * by this seat's view after the move, even when the destination is hidden. */
  private revealDepartures(events: GameEvent[], start: number) {
    const faces = new Map<number, Partial<ObjectView>>();
    for (let i = start; i < events.length; i++) {
      const move = events[i]!;
      if (move.kind !== "zoneChange" || move.from !== "battlefield") break;
      if (move.to === "battlefield" || move.to === "nowhere") continue;
      const before = this.cur.objects.find((o) => o.slot === move.slot);
      const after = this.final.get(move.slot);
      if (
        before?.facing === "down" &&
        after?.zone === move.to &&
        after.facing === "up" &&
        after.name !== null
      ) {
        faces.set(move.slot, { facing: "up", name: after.name, pt: null });
      }
    }
    if (faces.size) this.showTurn(faces);
  }

  private showTurn(faces: ReadonlyMap<number, Partial<ObjectView>>) {
    this.cut();
    const spotlight = { shown: new Set(faces.keys()) };
    // Lift the same face-down object first, then turn it while it is large.
    this.cut(HOLD.landing + 100, spotlight, true);
    for (const [slot, face] of faces) this.patch(slot, face);
    this.cut(HOLD.action, spotlight, true);
  }

  /** Whether the zone change `e` is shown in a showcase. */
  private showcased(e: Extract<GameEvent, { kind: "zoneChange" }>): boolean {
    // Cards just read in a choice travel directly to their destinations.
    if (
      this.seenInChoice.has(e.slot) ||
      (e.to === "graveyard" && this.directToGraveyard.has(e.slot))
    )
      return false;
    const owner =
      this.final.get(e.slot)?.owner ??
      this.cur.objects.find((o) => o.slot === e.slot)?.owner;
    return owner !== undefined && hops(e.from, e.to, owner);
  }

  /** Whether `e` takes a spell off the stack other than by resolving: countered, bounced,
   * exiled. */
  private struck(e: Extract<GameEvent, { kind: "zoneChange" }>): boolean {
    return (
      e.from === "stack" &&
      e.to !== "battlefield" &&
      e.reason !== "resolve" &&
      e.reason !== "cast" &&
      e.reason !== "play" &&
      this.cur.stack.some((s) => s.kind === "spell" && s.slot === e.slot)
    );
  }

  /** Ends a beat with the events pending, if any (or, `always`, with none: the table as it
   * stands), held `hold` or as long as its events need. */
  private cut(hold?: number, spotlight?: Spotlight, always = false) {
    if (this.pending.length === 0 && !always) return;
    const view: PlayerView = {
      ...this.cur,
      events: this.pending,
      pendingAttackers: [...this.aiming],
    };
    const landing = this.landing;
    this.landing =
      spotlight !== undefined && spotlight.payingSource === undefined;
    this.out.push({
      update: { ...this.update, view },
      hold: landing ? Math.max(hold ?? eventsHold(view), HOLD.landing) : hold,
      spotlight,
    });
    this.cur = { ...this.cur, events: [] };
    this.pending = [];
  }

  /** The object in `slot` once it has moved to `to`: as the view's end shows it where the
   * viewer may see it there, else as it was (a card going into a library, or into the
   * opponent's hand, shows no more than it did). */
  private moved(
    move: Extract<GameEvent, { kind: "zoneChange" }>,
  ): ObjectView | undefined {
    const { slot, from, to } = move;
    const was = this.cur.objects.find((o) => o.slot === slot);
    const now = this.final.get(slot);
    const owner = now?.owner ?? was?.owner;
    if (!owner) return undefined;
    const seen =
      now && (now.zone === to || !hiddenTo(to, owner)) ? now : (was ?? now);
    if (!seen) return undefined;
    if (from !== to && to === "battlefield" && now?.zone === to) {
      // The final object already carries counters added by later effects.
      // Keep any counters it entered with, and let later beats show their changes.
      const later = new Map<Counter, number>();
      for (const e of this.view.events.slice(
        this.view.events.indexOf(move) + 1,
      )) {
        if (!("slot" in e) || e.slot !== slot) continue;
        if (e.kind === "zoneChange") return { ...seen, zone: to };
        if (e.kind === "countersAdded" || e.kind === "countersRemoved") {
          const delta = e.kind === "countersAdded" ? e.amount : -e.amount;
          later.set(e.counter, (later.get(e.counter) ?? 0) + delta);
        }
      }
      if (later.size === 0) return { ...seen, zone: to };
      const counts = new Map(seen.counters.map((c) => [c.kind, c.count]));
      for (const [kind, delta] of later)
        counts.set(kind, (counts.get(kind) ?? 0) - delta);
      return {
        ...seen,
        zone: to,
        counters: [...counts].flatMap(([kind, count]) =>
          count > 0 || (count === 0 && kind === "loyalty")
            ? [{ kind, count }]
            : [],
        ),
      };
    }
    return { ...seen, zone: to };
  }

  /** Plays `e` on the table so far. Only what a beat must show is played; anything else the
   * view's end shows when it comes. */
  private apply(e: GameEvent, collecting = false) {
    const cur = this.cur;
    switch (e.kind) {
      case "zoneChange": {
        const moved = this.moved(e);
        if (!moved) return;
        const object = collecting ? { ...moved, name: null } : moved;
        const players = { ...cur.players };
        const side = object.owner;
        const player = { ...players[side] };
        player.library = player.library.filter((s) => s !== e.slot);
        player.graveyard = player.graveyard.filter((s) => s !== e.slot);
        if (e.to === "graveyard")
          player.graveyard = [...player.graveyard, e.slot];
        if (e.to === "library") {
          // Where the view's end has it, counting from the top (the list's end).
          const end = this.view.players[side].library;
          const fromTop = end.length - 1 - end.indexOf(e.slot);
          const at =
            !collecting && end.includes(e.slot)
              ? Math.max(0, player.library.length - fromTop)
              : player.library.length;
          player.library = [
            ...player.library.slice(0, at),
            e.slot,
            ...player.library.slice(at),
          ];
        }
        players[side] = player;
        const rest = cur.objects.filter((o) => o.slot !== e.slot);
        const battlefield = cur.battlefield.filter((s) => s !== e.slot);
        if (e.to === "battlefield") {
          battlefield.push(e.slot);
          // A creature entering attacking can have its own defender question.
          if (this.nextAiming.has(e.slot)) this.aiming.add(e.slot);
        } else {
          this.aiming.delete(e.slot);
        }
        this.cur = {
          ...cur,
          objects: [...rest, object].sort((a, b) => a.slot - b.slot),
          players,
          battlefield,
          stack:
            e.from === "stack"
              ? cur.stack.filter(
                  (s) => !(s.kind === "spell" && s.slot === e.slot),
                )
              : cur.stack,
        };
        // The card goes on the stack before its costs are paid (CR 601.2a). A beat for
        // those costs still shows the spell there, even before its cast event fills it in.
        if (e.to === "stack" && e.reason === "cast") {
          this.put({
            kind: "spell",
            slot: e.slot,
            targets: [],
            x: 0,
            paid: [],
          });
        }
        return;
      }
      case "attack": {
        // The declaration is already complete when its triggers go on the stack.
        // Replay its destinations on the same objects before cutting those beats.
        const declared = new Set(e.attackers);
        for (const slot of declared) this.aiming.delete(slot);
        this.cur = {
          ...cur,
          objects: cur.objects.map((object) => {
            if (!declared.has(object.slot) || object.zone !== "battlefield")
              return object;
            const combat = this.final.get(object.slot)?.combat;
            return combat?.kind === "attacking"
              ? { ...object, combat: { ...combat, blocked: false } }
              : object;
          }),
        };
        return;
      }
      case "cast":
        this.put(castItem(e));
        return;
      case "ability":
        this.put(e.item);
        return;
      case "draw": {
        // From the top of the library, its list's end.
        const library = cur.players[e.player].library;
        const taken =
          e.slots ??
          library.slice(Math.max(0, library.length - e.cards)).reverse();
        taken.forEach((s) => this.drawn.add(s));
        for (const slot of taken) {
          this.apply({
            kind: "zoneChange",
            slot,
            from: "library",
            to: "hand",
            reason: "draw",
          });
        }
        return;
      }
      case "tap":
      case "untap":
        this.patch(e.slot, { tapped: e.kind === "tap" });
        return;
      case "countersAdded":
      case "countersRemoved": {
        const o = cur.objects.find((x) => x.slot === e.slot);
        if (!o) return;
        const by = e.kind === "countersAdded" ? e.amount : -e.amount;
        const had = o.counters.find((c) => c.kind === e.counter)?.count ?? 0;
        const others = o.counters.filter((c) => c.kind !== e.counter);
        const count = Math.max(0, had + by);
        // A planeswalker's last loyalty paid shows as 0 until it dies
        // ([CR 704.5i](https://yawgatog.com/resources/magic-rules/#R7045i)).
        const kept = count > 0 || e.counter === "loyalty";
        this.patch(e.slot, {
          counters: kept ? [...others, { kind: e.counter, count }] : others,
        });
        return;
      }
      case "lifeGain":
      case "lifeLoss": {
        const by = e.kind === "lifeGain" ? e.amount : -e.amount;
        const player = cur.players[e.player];
        this.cur = {
          ...cur,
          players: {
            ...cur.players,
            [e.player]: { ...player, life: player.life + by },
          },
        };
        return;
      }
      case "gainControl":
        this.patch(e.slot, { controller: e.player });
        return;
      default:
        return;
    }
  }

  /** After a shuffle the old handles can no longer identify any face. For an immediate
   * deal, only the recorded draw order matters: place those anonymous backs at the top. */
  private shuffle(side: Side, deal: number[]) {
    const player = this.cur.players[side];
    const library = new Set(player.library);
    const top = deal.filter((slot) => library.delete(slot));
    const rest = this.view.players[side].library.filter((slot) =>
      library.delete(slot),
    );
    this.cur = {
      ...this.cur,
      players: {
        ...this.cur.players,
        [side]: { ...player, library: [...library, ...rest, ...top.reverse()] },
      },
      objects: this.cur.objects.map((o) =>
        o.zone === "library" && o.owner === side ? { ...o, name: null } : o,
      ),
    };
  }

  /** The effects of a resolving ability remain, but its stack surface is gone. */
  private finishAbility() {
    if (this.cur.stack.at(-1)?.kind === "ability")
      this.cur = { ...this.cur, stack: this.cur.stack.slice(0, -1) };
  }

  /** Puts `item` on top of the stack, in place of itself in the making: a spell or ability
   * goes on the stack as its casting begins, before its targets are chosen
   * ([CR 601.2a](https://yawgatog.com/resources/magic-rules/#R6012a)), and a view shown
   * meanwhile (the opponent choosing them) has it there already. */
  private put(item: StackItem) {
    const stack = this.cur.stack;
    const top = stack.at(-1);
    const making =
      top !== undefined &&
      (item.kind === "spell"
        ? top.kind === "spell" && top.slot === item.slot
        : top.kind === "ability" &&
          top.source === item.source &&
          top.ability === item.ability &&
          top.granted === item.granted &&
          top.controller === item.controller);
    this.cur = {
      ...this.cur,
      stack: [...(making ? stack.slice(0, -1) : stack), item],
    };
  }

  private patch(slot: number, change: Partial<ObjectView>) {
    this.cur = {
      ...this.cur,
      objects: this.cur.objects.map((o) =>
        o.slot === slot ? { ...o, ...change } : o,
      ),
    };
  }
}

/** Whether `e` is something a spell did, rather than its casting: what a beat before the
 * spell leaves the stack shows. */
function effect(e: GameEvent): boolean {
  switch (e.kind) {
    case "cast":
    case "ability":
    case "abilityFinished":
    case "tap":
    case "untap":
    case "mana":
    case "stepBegins":
      return false;
    case "zoneChange":
      return e.to !== "stack";
    default:
      return true;
  }
}

/** How long `view`'s events take to play out, in milliseconds. */
export function eventsHold(view: PlayerView): number {
  return Math.max(0, ...view.events.map(eventHold));
}

function eventHold(e: GameEvent): number {
  switch (e.kind) {
    case "abilityFinished":
      return 0;
    case "stepBegins":
      // A new turn holds while it is announced (TurnBanner), before anything is played in it.
      return e.step === "untap" ? HOLD.turn : HOLD.step;
    // The opponent's draw crosses the table; the player's own is in their hand at once.
    case "draw":
      return e.player === "opponent" ? HOLD.draw : HOLD.step;
    // Mana flows into what it pays for (ManaFlow), held no longer than a tap.
    case "tap":
    case "untap":
    case "mana":
      return HOLD.step;
    case "zoneChange":
      if (e.from === "stack" && e.to === "battlefield")
        return HOLD.permanentLanding;
      return e.reason === "counter" ? HOLD.counter : HOLD.action;
    // The player's own casts they just chose; the opponent's they have to read.
    case "cast":
      return e.controller === "opponent" ? HOLD.stack : HOLD.action;
    case "ability":
      if (e.item.kind !== "ability" || e.item.controller !== "opponent") {
        return HOLD.action;
      }
      return e.item.triggered ? HOLD.trigger : HOLD.stack;
    default:
      return HOLD.action;
  }
}
