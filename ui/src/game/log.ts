// The game log: what each view's events say happened, as short sentences naming cards in
// full ("Opponent cast <card> targeting <card>", "You activated <card> +1 targeting Opponent",
// "<card> was countered"). The routine (steps
// beginning, the draw step's draw, tapping for mana) is left out; each turn opens with a
// heading.
import type {
  Counter,
  GameEvent,
  LossReason,
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
  /** The public result of a library choice, also shown near the player. */
  libraryChoice?: string;
  scry?: true;
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

/** A game's log as its views are shown, and what keeps it. */
export interface Logged {
  /** What has happened in the game so far, oldest first. */
  log: LogEntry[];
  /** For each decision, by how many of the player's answers stand before it
   * (`DecisionView.undoDepth`), the key of the last log entry before it: an undo back to that
   * decision goes back to there. None for a decision shown before the screen saw it. */
  marks: (number | undefined)[];
  /** The last key given a log entry: keys only grow, past entries an undo took back too. */
  key: number;
  names: Names;
}

/** The log of a game no view of which has shown. */
export const UNLOGGED: Logged = {
  log: [],
  marks: [],
  key: -1,
  names: new Map(),
};

/** The log keeps this many entries. */
const LOG_LENGTH = 200;

/** `logged` once `views` have shown, in order. */
export function logged(logged: Logged, views: PlayerView[]): Logged {
  let { names, log, marks, key } = logged;
  for (const view of views) {
    const status = view.status;
    const decision = status.kind === "deciding" ? status.decision : undefined;
    // What happened since the decision asked again never did.
    const kept =
      decision && view.undone ? marks[decision.undoDepth] : undefined;
    if (kept !== undefined) log = log.filter((e) => e.key <= kept);
    names = learnNames(names, view);
    const lines = logLines(view, names);
    if (lines.length)
      log = [...log, ...lines.map((l) => ({ ...l, key: ++key }))];
    if (decision) {
      marks = marks.slice(0, decision.undoDepth);
      marks[decision.undoDepth] = key;
    }
  }
  return {
    names,
    marks,
    key,
    log: log === logged.log ? log : log.slice(-LOG_LENGTH),
  };
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

/** The power and toughness kinds the rules read, named apart from the rest. */
const COUNTERS: Partial<Record<Counter, string>> = {
  plusOne: "+1/+1",
  minusOne: "−1/−1",
};

const NUMBERS = ["Zero", "One", "Two", "Three", "Four", "Five", "Six", "Seven"];

/** A kind's name in the rules: "time", "first strike", "+1/+2" (`plusOnePlusTwo`). */
function counterName(kind: Counter): string {
  const named = COUNTERS[kind];
  if (named) return named;
  const pt = /^(plus|minus)([A-Z][a-z]+)(Plus|Minus)([A-Z][a-z]+)$/.exec(kind);
  if (pt) {
    const part = (sign: string, n: string) =>
      `${sign.toLowerCase() === "plus" ? "+" : "−"}${NUMBERS.indexOf(n)}`;
    return `${part(pt[1]!, pt[2]!)}/${part(pt[3]!, pt[4]!)}`;
  }
  return kind.replace(/[A-Z]/g, (c) => ` ${c.toLowerCase()}`);
}

/** How a player lost, after "lost the game". */
const LOSSES: Record<LossReason, (life: number) => string> = {
  life: (life) => ` at ${life} life`,
  library: () => " drawing from an empty library",
  effect: () => " to a card's effect",
  concede: () => " by conceding",
};

const counters = (n: number, kind: Counter) =>
  `${n === 1 ? "a" : n} ${counterName(kind)} counter${n === 1 ? "" : "s"}`;

/** A loyalty ability's cost as its card prints it: "+1", "0", "−3". */
const loyaltyCost = (n: number) =>
  n > 0 ? `+${n}` : n < 0 ? `\u2212${-n}` : "0";

/** The indices of `events`' loyalty counters paid to activate an ability, which the
 * activation's line says: the last such change to its source before it. */
function loyaltyPaid(events: GameEvent[]): Set<number> {
  const paid = new Set<number>();
  events.forEach((e, i) => {
    if (e.kind !== "ability" || !e.loyalty || e.item.kind !== "ability") return;
    const source = e.item.source;
    const kind = e.loyalty > 0 ? "countersAdded" : "countersRemoved";
    for (let j = i - 1; j >= 0; j--) {
      const c = events[j]!;
      if (
        c.kind === kind &&
        c.slot === source &&
        c.counter === "loyalty" &&
        c.amount === Math.abs(e.loyalty)
      ) {
        paid.add(j);
        return;
      }
    }
  });
  return paid;
}

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
            `Turn ${view.turn.ownNumber}: ${e.active === "you" ? "your" : "Opponent's"} turn`,
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
      case "scry": {
        const libraryChoice = `Scry · ${e.top} top · ${e.bottom} bottom`;
        out.push({
          kind: "action",
          side: e.player,
          text: `${who(e.player)} put ${e.top} on top and ${e.bottom} on bottom`,
          libraryChoice,
          scry: true,
        });
        return;
      }
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
      case "ability": {
        const item = e.item;
        if (item.kind !== "ability") return;
        const targets = item.targets.map(target);
        const aimed = targets.length ? ` targeting ${listed(targets)}` : "";
        if (item.triggered) {
          say(item.controller, `${item.name} triggered${aimed}`);
          return;
        }
        const cost = e.loyalty === null ? "" : ` ${loyaltyCost(e.loyalty)}`;
        say(
          item.controller,
          `${who(item.controller)} activated ${item.name}${cost}${aimed}`,
        );
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
        say(
          e.player,
          `${who(e.player)} ${e.won ? "won" : "lost"} ${name(e.source)}'s coin flip`,
        );
        return;
      case "lose": {
        const outcome =
          view.status.kind === "over" ? view.status.outcome : undefined;
        const how =
          outcome?.kind === "win"
            ? LOSSES[outcome.reason](view.players[e.player].life)
            : "";
        say(e.player, `${who(e.player)} lost the game${how}`);
        return;
      }
      case "win":
        say(e.player, `${who(e.player)} won the game`);
        return;
      case "libraryShuffled":
      case "tap":
      case "untap":
      case "mana":
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
      case "library": {
        const library = view.players[side].library;
        // Older recordings omit the choice. Infer it only when the ends differ.
        const inferred =
          e.from === "stack" && library.length > 1
            ? library.at(-1) === e.slot
              ? true
              : library[0] === e.slot
                ? false
                : undefined
            : undefined;
        const libraryTop = e.libraryTop ?? inferred;
        if (libraryTop === undefined) {
          say(side, `${card} was put into ${whose(side)} library`);
        } else {
          const end = libraryTop ? "top" : "bottom";
          out.push({
            kind: "action",
            side,
            text: `${who(side)} put ${card} on the ${end} of ${side === "you" ? "your" : "their"} library`,
            libraryChoice: libraryTop ? "Top of Library" : "Bottom of Library",
          });
        }
        return;
      }
      case "stack":
      case "nowhere":
        return;
    }
  };

  const paid = loyaltyPaid(view.events);
  view.events.forEach((e, i) => paid.has(i) || line(e));
  return out;
}
