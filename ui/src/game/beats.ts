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
  GameEvent,
  GameUpdate,
  ObjectView,
  PlayerView,
  Side,
  StackItem,
  ZoneName,
} from "../generated/protocol";
import { choiceOf } from "./choices";
import { withResolving } from "./decisions";
import { sameHandling, type Spotlight } from "./layout";

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
  const { view } = update;
  const poolBeforeChoice =
    before && choiceOf(view) && spentPool(before.view, view);
  if (
    before &&
    !poolBeforeChoice &&
    sameHandling(before.view, view) &&
    view.events.length === 0
  )
    return [{ update, hold: 0 }];
  if (
    !before ||
    view.undone ||
    (view.events.length === 0 &&
      !poolBeforeChoice &&
      !packHandling(before.view, view))
  )
    return [{ update }];
  return new Walk(withResolving(before.view), update).run();
}

/** Return a private arrangement as one packet before the pile settles. */
function packHandling(before: PlayerView, after: PlayerView): boolean {
  const handling = before.handling;
  return (
    handling?.zone === "library" &&
    handling.count !== null &&
    handling.count > 1 &&
    !sameHandling(before, after)
  );
}

/** Whether a spell or ability is still on `stack`. */
function onStack(stack: StackItem[], item: StackItem): boolean {
  return stack.some((s) => sameItem(s, item));
}

export function sameItem(a: StackItem, b: StackItem): boolean {
  if (a.kind === "spell" && b.kind === "spell") return a.slot === b.slot;
  return a.kind === b.kind && JSON.stringify(a) === JSON.stringify(b);
}

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

/** A view's events played forward from the view before it, cut into beats. */
class Walk {
  private readonly view: PlayerView;
  private readonly final: Map<number, ObjectView>;
  private cur: PlayerView;
  private pending: GameEvent[] = [];
  private readonly out: Beat[] = [];
  /** Slots drawn in this view so far, to be seen in a hand before they leave it. */
  private readonly drawn = new Set<number>();
  private readonly seenInChoice: Set<number>;
  private readonly startsHandling: boolean;
  /** The beat cut last showed cards in a showcase: the next one lands them. */
  private landing = false;

  constructor(
    before: PlayerView,
    private readonly update: GameUpdate,
  ) {
    const choice = choiceOf(before);
    this.seenInChoice = new Set(choice?.kind === "pick" ? choice.cards : []);
    this.view = update.view;
    this.startsHandling =
      !!this.view.handling && !sameHandling(before, this.view);
    this.final = new Map(this.view.objects.map((o) => [o.slot, o]));
    this.cur = { ...before, status: { kind: "playing" }, events: [] };
  }

  run(): Beat[] {
    const events = this.view.events;
    for (let i = 0; i < events.length; i++) {
      const e = events[i]!;
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
        if (handling && shuffle.has(handling.side)) {
          // Put the search packet back before shuffling. Those backs remain the same
          // elements, but the shuffle breaks the correspondence to any known faces.
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
    if (arrivingTrigger) this.cut(HOLD.action);
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
    if (packHandling(this.cur, this.view))
      this.cut(HOLD.handling, { packHandling: true }, true);
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

  /** Whether the zone change `e` is shown in a showcase. */
  private showcased(e: Extract<GameEvent, { kind: "zoneChange" }>): boolean {
    // Cards just read in a choice travel directly to their destinations.
    if (this.seenInChoice.has(e.slot)) return false;
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
    const view: PlayerView = { ...this.cur, events: this.pending };
    const landing = this.landing;
    this.landing = spotlight !== undefined;
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
  private moved(slot: number, to: ZoneName): ObjectView | undefined {
    const was = this.cur.objects.find((o) => o.slot === slot);
    const now = this.final.get(slot);
    const owner = now?.owner ?? was?.owner;
    if (!owner) return undefined;
    const seen =
      now && (now.zone === to || !hiddenTo(to, owner)) ? now : (was ?? now);
    return seen && { ...seen, zone: to };
  }

  /** Plays `e` on the table so far. Only what a beat must show is played; anything else the
   * view's end shows when it comes. */
  private apply(e: GameEvent, collecting = false) {
    const cur = this.cur;
    switch (e.kind) {
      case "zoneChange": {
        const moved = this.moved(e.slot, e.to);
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
        if (e.to === "battlefield") battlefield.push(e.slot);
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
