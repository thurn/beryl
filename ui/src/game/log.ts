// The game log: what each view's events say happened, as short sentences naming cards in
// full ("Opponent cast <card> targeting <card>", "<card> was countered"). The routine (steps beginning, the draw step's draw, tapping for mana) is left
// out; each turn opens with a heading.
import type {
  Counter,
  GameEvent,
  PlayerView,
  Side,
  TargetRef,
} from "../generated/protocol";

export interface LogEntry {
  /** Numbers the game's entries from 0. */
  key: number;
  /** A turn's heading, something countered, or anything else that happened. */
  kind: "turn" | "counter" | "action";
  /** The player it is about, for its marker. */
  side: Side;
  text: string;
}

export type LogLine = Omit<LogEntry, "key">;

/** The names of the cards seen so far, by slot: a card that has gone where the viewer can't
 * see it (a library, the opponent's hand) is still named by what it was. */
export type Names = ReadonlyMap<number, string>;

/** `names` with the cards `view` shows. */
export function learnNames(names: Names, view: PlayerView): Names {
  const out = new Map(names);
  for (const o of view.objects) {
    if (o.name) out.set(o.slot, o.name);
  }
  return out;
}

const who = (side: Side) => (side === "you" ? "You" : "Opponent");
/** A player as the object of a sentence. */
const whom = (side: Side) => (side === "you" ? "you" : "Opponent");
const whose = (side: Side) => (side === "you" ? "your" : "Opponent's");

/** "A", "A and B", "A, B and C". */
export function listed(items: string[]): string {
  if (items.length <= 1) return items[0] ?? "";
  return `${items.slice(0, -1).join(", ")} and ${items.at(-1)}`;
}

const COUNTERS: Record<Counter, string> = {
  plusOne: "+1/+1",
  minusOne: "−1/−1",
  loyalty: "loyalty",
  time: "time",
  story: "story",
  chorus: "chorus",
};

const counters = (n: number, kind: Counter) =>
  `${n === 1 ? "a" : n} ${COUNTERS[kind]} counter${n === 1 ? "" : "s"}`;

/** What `view`'s events say happened, oldest first. `names` names the cards it no longer
 * shows (`learnNames` of the views before, and of this one). */
export function logLines(view: PlayerView, names: Names): LogLine[] {
  const objects = new Map(view.objects.map((o) => [o.slot, o]));
  const name = (slot: number) => names.get(slot) ?? "a card";
  const named = (slot: number) => names.has(slot);
  const owner = (slot: number) => objects.get(slot)?.owner ?? "opponent";
  const controller = (slot: number) =>
    objects.get(slot)?.controller ?? owner(slot);
  const target = (t: TargetRef) =>
    t.kind === "player" ? whom(t.player) : name(t.slot);
  const out: LogLine[] = [];
  const say = (side: Side, text: string, kind: LogLine["kind"] = "action") =>
    out.push({ kind, side, text });

  const line = (e: GameEvent) => {
    switch (e.kind) {
      case "stepBegins":
        if (e.step === "untap") {
          say(
            e.active,
            `Turn ${view.turn.number}: ${e.active === "you" ? "your" : "Opponent's"} turn`,
            "turn",
          );
        }
        return;
      case "cast": {
        const spell = view.stack.find(
          (s) => s.kind === "spell" && s.slot === e.slot,
        );
        const targets = spell?.targets.map(target) ?? [];
        say(
          e.controller,
          `${who(e.controller)} cast ${name(e.slot)}` +
            (targets.length ? ` targeting ${listed(targets)}` : ""),
        );
        return;
      }
      case "zoneChange":
        return zoneChange(e);
      case "draw":
        // The draw step's draw goes without saying.
        if (view.turn.step === "draw" && e.player === view.turn.active) return;
        say(
          e.player,
          `${who(e.player)} drew ${e.cards === 1 ? "a card" : `${e.cards} cards`}`,
        );
        return;
      case "damage":
        say(
          controller(e.source),
          `${name(e.source)} dealt ${e.amount} damage to ${target(e.to)}`,
        );
        return;
      case "lifeGain":
        say(e.player, `${who(e.player)} gained ${e.amount} life`);
        return;
      case "lifeLoss":
        say(e.player, `${who(e.player)} lost ${e.amount} life`);
        return;
      case "countersAdded":
        say(
          controller(e.slot),
          `${name(e.slot)} got ${counters(e.amount, e.counter)}`,
        );
        return;
      case "countersRemoved":
        say(
          controller(e.slot),
          `${name(e.slot)} lost ${counters(e.amount, e.counter)}`,
        );
        return;
      case "tokensCreated": {
        const kinds = new Map<string, number>();
        for (const t of e.tokens) {
          kinds.set(name(t), (kinds.get(name(t)) ?? 0) + 1);
        }
        const made = [...kinds].map(([n, count]) =>
          count === 1 ? `a ${n}` : `${count} ${n}s`,
        );
        say(e.controller, `${who(e.controller)} created ${listed(made)}`);
        return;
      }
      case "attack":
        say(
          e.player,
          `${who(e.player)} attacked with ${listed(e.attackers.map(name))}`,
        );
        return;
      case "gainControl":
        say(e.player, `${who(e.player)} gained control of ${name(e.slot)}`);
        return;
      case "coinFlip":
        say(e.player, `${who(e.player)} ${e.won ? "won" : "lost"} a coin flip`);
        return;
      case "lose":
        say(e.player, `${who(e.player)} lost the game`);
        return;
      case "win":
        say(e.player, `${who(e.player)} won the game`);
        return;
      case "tap":
      case "untap":
        return;
    }
  };

  const zoneChange = (e: Extract<GameEvent, { kind: "zoneChange" }>) => {
    const card = name(e.slot);
    const side = owner(e.slot);
    switch (e.reason) {
      // Said by the cast, the draw and the tokens' creation.
      case "cast":
      case "draw":
      case "created":
        return;
      case "play":
        say(side, `${who(side)} played ${card}`);
        return;
      case "resolve":
        say(controller(e.slot), `${card} resolved`);
        return;
      case "counter":
        say(side, `${card} was countered`, "counter");
        return;
      case "destroy":
        say(side, `${card} was destroyed`);
        return;
      case "sacrifice":
        say(side, `${who(side)} sacrificed ${card}`);
        return;
      case "discard":
        say(side, `${who(side)} discarded ${card}`);
        return;
    }
    // Otherwise, by where it went. A card the viewer never saw goes unsaid.
    if (!named(e.slot) || e.from === e.to) return;
    switch (e.to) {
      case "hand":
        say(
          side,
          e.from === "battlefield"
            ? `${card} returned to ${whose(side)} hand`
            : `${card} was put into ${whose(side)} hand`,
        );
        return;
      case "battlefield":
        say(controller(e.slot), `${card} entered the battlefield`);
        return;
      case "graveyard":
        say(
          side,
          e.from === "battlefield"
            ? `${card} went to the graveyard`
            : `${card} was put into ${whose(side)} graveyard`,
        );
        return;
      case "exile":
        say(side, `${card} was exiled`);
        return;
      case "library":
        say(side, `${card} was put into ${whose(side)} library`);
        return;
      case "stack":
      case "nowhere":
        return;
    }
  };

  for (const e of view.events) line(e);
  return out;
}
