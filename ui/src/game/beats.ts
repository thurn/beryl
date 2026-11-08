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
import type { Spotlight } from "./layout";

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
  /** The opponent drawing: a card crossing from their library to their hand. */
  draw: 700,
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

/** `update` as the beats a player can follow from `before`, the view last shown or queued; with
 * none, or nothing worth a beat, it is one beat, itself. */
export function beats(
  before: GameUpdate | undefined,
  update: GameUpdate,
): Beat[] {
  const { view } = update;
  if (!before || view.events.length === 0 || view.undone) return [{ update }];
  return new Walk(withResolving(before.view), update).run();
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

/** Zones whose cards the viewer may not see: a library, and the opponent's hand. */
function hiddenTo(zone: ZoneName, owner: Side): boolean {
  return zone === "library" || (zone === "hand" && owner === "opponent");
}

/** Whether moving a card from `from` to `to` takes it out of a zone the viewer cannot see into
 * a pile (milled, discarded by the opponent, exiled from a library), or out of sight from a
 * pile (shuffled from a graveyard into a library). Such a card is shown face up over the pile
 * it goes to (SHOWCASE), then shrinks into it. A card the viewer sees leave (a permanent, a
 * spell, their own hand card) or that moves between two face-up piles (exiled from a
 * graveyard, put into a graveyard from exile) travels as it is: there is nothing new to read. */
function hops(from: ZoneName, to: ZoneName, owner: Side): boolean {
  if (from === to) return false;
  if (to === "library") return from === "graveyard" || from === "exile";
  return (to === "graveyard" || to === "exile") && hiddenTo(from, owner);
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
  /** The beat cut last showed cards in a showcase: the next one lands them. */
  private landing = false;

  constructor(
    before: PlayerView,
    private readonly update: GameUpdate,
  ) {
    this.view = update.view;
    this.final = new Map(this.view.objects.map((o) => [o.slot, o]));
    this.cur = { ...before, status: { kind: "playing" }, events: [] };
  }

  run(): Beat[] {
    const events = this.view.events;
    for (let i = 0; i < events.length; i++) {
      const e = events[i]!;
      if (e.kind === "zoneChange" && this.showcased(e)) {
        // Every card of the move at once: the events of one effect come together.
        const group = [e];
        while (i + 1 < events.length) {
          const next = events[i + 1]!;
          if (next.kind !== "zoneChange" || !this.showcased(next)) break;
          group.push(next);
          i++;
        }
        this.cut();
        group.forEach((g) => this.apply(g));
        this.pending = group;
        this.cut(HOLD.showcase, {
          shown: new Set(group.map((g) => g.slot)),
        });
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
      const drawing = this.drawn.size;
      this.apply(e);
      this.pending.push(e);
      if (e.kind === "draw" && e.player === "opponent") {
        // The opponent's hand lies at the table's edge: what they drew dips into view first.
        const drawn = new Set([...this.drawn].slice(drawing));
        if (drawn.size > 0) this.cut(HOLD.draw, { drawn });
        continue;
      }
      const item =
        e.kind === "cast" ? castItem(e) : e.kind === "ability" ? e.item : null;
      if (item && !onStack(this.view.stack, item)) {
        // Gone by the view's end: shown on the stack before it resolves.
        this.cut(stackHold(item));
      }
    }
    // A question asked over the table (a search, a scry) waits for what led to it to land: a
    // card exiled, a spell resolving, seen rather than moving under the overlay.
    if (this.pending.some(effect) && choiceOf(this.view)) this.cut();
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
        hold: this.landing && !deciding ? HOLD.landing : undefined,
      },
    ];
  }

  /** Whether the zone change `e` is shown in a showcase. */
  private showcased(e: Extract<GameEvent, { kind: "zoneChange" }>): boolean {
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
  private apply(e: GameEvent) {
    const cur = this.cur;
    switch (e.kind) {
      case "zoneChange": {
        const object = this.moved(e.slot, e.to);
        if (!object) return;
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
          const at = end.includes(e.slot)
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
        const taken = library.slice(Math.max(0, library.length - e.cards));
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
