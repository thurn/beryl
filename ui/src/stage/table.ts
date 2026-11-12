// The table: the part of a view a script accounts for, as the stage holds it between views
// (rust/crates/beryl_session/src/script/mod.rs, "The table and replay"). `apply` is that
// module's replay, one step at a time: each step carries all of what it changes, so the stage
// shows the table after every step without the view that follows.
//
// A step is the session's but for one the stage plays of its own: the `lift` of the cards a
// prompt holds up for its player to handle (`Hold`), which the prompt on the stage plays as
// it plays a prompt's marks (prompt.ts), from the state the session sent it.
import type {
  Act as Sent,
  ObjectView,
  PlayerView,
  Reset,
  ScriptStep as SentStep,
  Side,
  Span,
  StackItem,
  Subject,
  ZoneName,
} from "../generated/protocol";

/** The cards a prompt holds up off the table for its player to handle, in the piles it lays
 * them out in, each in its order: one pile to pick from, or the two a split deals them into.
 * A card in a tray rests there, wherever the table has it, until the tray lets it go. */
export interface Tray {
  piles: readonly Heap[];
}

/** One pile of a tray. */
export interface Heap {
  cards: readonly number[];
  /** How many cards it is laid out for where that is not as many as the tray holds: what
   * it must come to hold. */
  room?: number;
}

/** The cards held up for both players to read, the Revealed Cards pile: each as the `showcase`
 * step that held it up had it, the first revealed first, one a slot. What one spell or
 * ability reveals lies there together, and what another reveals takes its place. The pile is
 * no part of a view: a table that starts over has none. */
export interface Revealed {
  cards: readonly ObjectView[];
  /** What revealed them; none for the rules of the game. */
  cause?: Subject;
}

/** The stage's own `lift`: the cards of `tray` come away from where they lie into it as the
 * span begins, lie as each later beginning has it, and go back to where the table has them
 * as it ends. `sorted` is a beginning that only moves cards within a tray already up. */
export interface Hold {
  verb: "lift";
  span: Span;
  tray: Tray;
  sorted?: boolean;
}

/** What a step does: an act the session sent, or the stage's own. */
export type Act = Sent | Hold;

/** One step of a script the stage plays. */
export interface Step {
  act: Act;
  /** The spell, ability, permanent or player responsible; none for the rules of the game. */
  cause?: Subject;
}

/** Whether `step` is one the session sent. */
export const sent = (step: Step): step is SentStep => !("tray" in step.act);

/** A script the stage plays: the session's, or one of the prompt's own steps. */
export interface Script {
  reset?: Reset;
  beats: Step[][];
}

/** A view whose table the steps so far have changed, and the numbers its scripts gave the
 * abilities on its stack, bottom first: a view's stack does not name them. The rest of the
 * view is the chrome of the last view taken whole. */
export interface Table {
  view: PlayerView;
  abilities: readonly number[];
  /** The cards the opponent is handling, as they were when the look began: the same cards
   * stay lifted while it lasts, whatever order it leaves their pile in. */
  handled?: readonly number[];
  /** The cards the open prompt holds up for its player. */
  tray?: Tray;
  /** The cards held up to be read. */
  revealed?: Revealed;
  /** Each card that has come to a hand since the stage started over, by its slot, and its
   * turn among them: a hand keeps no order in the rules, and on the table a card joins the
   * right end of its hand, as Arena's does (`vPdoSA5H5xU 17:47.3`). */
  order?: ReadonlyMap<number, number>;
}

/** `table`'s view with each hand in its order of arrival (`Table.order`): the cards that
 * were there as the stage started over in slot order, then those that came since, the
 * latest last. Nothing else of the view depends on where a record lies in its list. */
export function arranged({ view, order }: Table): PlayerView {
  if (!order?.size) return view;
  const turn = (o: ObjectView) => order.get(o.slot) ?? 0;
  const objects = [...view.objects].sort((a, b) => turn(a) - turn(b));
  return { ...view, objects };
}

/** `order` once the card in `slot` has gone from `from` to `to`: last of those in a hand,
 * or of none. */
function arrived(
  order: Table["order"],
  slot: number,
  from: ZoneName,
  to: ZoneName,
): Table["order"] {
  if ((from === "hand") === (to === "hand")) return order;
  const next = new Map(order);
  next.delete(slot);
  if (to === "hand") next.set(slot, 1 + Math.max(0, ...next.values()));
  return next;
}

/** The cards a look that begins at `view` handles: the top `count` of a library, top first,
 * or your hand, which the opponent looks at; of their own hand, none shows. */
function handledAt(
  view: PlayerView,
  handling: NonNullable<PlayerView["handling"]>,
): number[] {
  if (handling.zone === "library") {
    const library = view.players[handling.side].library;
    return handling.count ? library.slice(-handling.count).reverse() : [];
  }
  if (handling.side !== "you") return [];
  return view.objects
    .filter((o) => o.owner === "you" && o.zone === "hand")
    .map((o) => o.slot);
}

/** `view` with its stack as a script's table has it (script/table.rs `Table::of`), the same
 * for both seats: on top is the spell or ability whose resolution asks the viewer something,
 * which a view leaves out of its list, and a spell whose card has left the stack is not on
 * it. An ability stays on the stage, with the lines drawn to it, while its prompt is open. */
export function tabled(view: PlayerView): PlayerView {
  const { status } = view;
  const resolving =
    (status.kind === "deciding" && status.decision.resolving) || undefined;
  const same = (item: StackItem) =>
    JSON.stringify(item) === JSON.stringify(resolving);
  const on = (item: StackItem) =>
    item.kind === "ability" ||
    view.objects.some((o) => o.slot === item.slot && o.zone === "stack");
  const listed =
    resolving && !view.stack.some(same)
      ? [...view.stack, resolving]
      : view.stack;
  const stack = listed.filter(on);
  return stack.length === view.stack.length &&
    stack.every((item, i) => item === view.stack[i])
    ? view
    : { ...view, stack };
}

/** The table of a view the stage starts over from: the cards of `before`, the table until
 * then, that lay in a hand and still do keep their order there. */
export function tableOf(
  view: PlayerView,
  reset?: Reset,
  before?: Table,
): Table {
  const table = { view: tabled(view), abilities: reset?.abilities ?? [] };
  const held = (of: PlayerView) =>
    new Set(of.objects.filter((o) => o.zone === "hand").map((o) => o.slot));
  if (!before?.order?.size) return table;
  const [was, is] = [held(before.view), held(view)];
  const order = new Map(
    [...before.order].filter(([slot]) => was.has(slot) && is.has(slot)),
  );
  return order.size ? { ...table, order } : table;
}

/** Where the stack item `subject` names lies in the stack, bottom first. */
function stackIndex({ view, abilities }: Table, subject: Subject) {
  if (subject.kind === "object") {
    return view.stack.findIndex(
      (item) => item.kind === "spell" && item.slot === subject.slot,
    );
  }
  if (subject.kind === "player") return -1;
  let ability = 0;
  return view.stack.findIndex(
    (item) => item.kind === "ability" && abilities[ability++] === subject.id,
  );
}

const without = <T>(list: readonly T[], index: number) =>
  index < 0 ? [...list] : list.filter((_, i) => i !== index);

const inserted = <T>(list: readonly T[], item: T, at: number | undefined) => {
  const out = [...list];
  out.splice(at ?? out.length, 0, item);
  return out;
};

/** The table with the object in `slot` taken out of `zone`'s order. A hand and exile keep no
 * order; a card lies in its owner's library and graveyard (CR 400.3). */
function taken(table: Table, slot: number, zone: ZoneName, owner: Side): Table {
  const { view } = table;
  const pile = (key: "library" | "graveyard"): Table => ({
    ...table,
    view: {
      ...view,
      players: {
        ...view.players,
        [owner]: {
          ...view.players[owner],
          [key]: view.players[owner][key].filter((s) => s !== slot),
        },
      },
    },
  });
  switch (zone) {
    case "battlefield":
      return {
        ...table,
        view: {
          ...view,
          battlefield: view.battlefield.filter((s) => s !== slot),
        },
      };
    case "stack": {
      const stack = without(
        view.stack,
        stackIndex(table, { kind: "object", slot }),
      );
      return { ...table, view: { ...view, stack } };
    }
    case "library":
    case "graveyard":
      return pile(zone);
    default:
      return table;
  }
}

/** The table with `object` put into its zone's order at `at`, or at the end: the top of a
 * pile, the last permanent, the top of the stack as `item`. */
function placed(
  table: Table,
  object: ObjectView,
  at: number | undefined,
  item: StackItem | undefined,
): Table {
  const { view } = table;
  switch (object.zone) {
    case "battlefield":
      return {
        ...table,
        view: {
          ...view,
          battlefield: inserted(view.battlefield, object.slot, at),
        },
      };
    case "stack":
      return item
        ? { ...table, view: { ...view, stack: inserted(view.stack, item, at) } }
        : table;
    case "library":
    case "graveyard": {
      const player = view.players[object.owner];
      return {
        ...table,
        view: {
          ...view,
          players: {
            ...view.players,
            [object.owner]: {
              ...player,
              [object.zone]: inserted(player[object.zone], object.slot, at),
            },
          },
        },
      };
    }
    default:
      return table;
  }
}

/** The table with the record `object` in place of the one its slot held, kept in slot
 * order. When the record changes the object's zone, or gives it a place `at`, it leaves its
 * old zone's order (`from`, for an object the table did not hold) and enters the new one's. */
function put(
  table: Table,
  object: ObjectView,
  from: ZoneName = object.zone,
  at?: number,
  item?: StackItem,
): Table {
  const before = table.view.objects.find((o) => o.slot === object.slot);
  const objects = [
    ...table.view.objects.filter((o) => o.slot !== object.slot),
    object,
  ].sort((a, b) => a.slot - b.slot);
  const zone = before?.zone ?? from;
  const order = arrived(table.order, object.slot, zone, object.zone);
  const next = { ...table, view: { ...table.view, objects }, order };
  if (zone === object.zone && at === undefined) return next;
  return placed(
    taken(next, object.slot, zone, before?.owner ?? object.owner),
    object,
    at,
    item,
  );
}

/** `table` with the record `object` held up to be read by `cause`: with the others `cause`
 * revealed, in place of the face its slot had there; or alone, where another revealed what
 * was held up. */
function heldUp(table: Table, object: ObjectView, cause?: Subject): Table {
  const before = table.revealed;
  const same = JSON.stringify(before?.cause) === JSON.stringify(cause);
  const others = same ? (before?.cards ?? []) : [];
  const cards = [...others.filter((o) => o.slot !== object.slot), object];
  return { ...table, revealed: { cards, ...(cause && { cause }) } };
}

/** The table after the step `act`, which `cause` is responsible for. */
export function apply(table: Table, act: Act, cause?: Subject): Table {
  const { view } = table;
  const player = (
    side: Side,
    change: Partial<PlayerView["players"][Side]>,
  ) => ({
    ...table,
    view: {
      ...view,
      players: {
        ...view.players,
        [side]: { ...view.players[side], ...change },
      },
    },
  });
  switch (act.verb) {
    case "fly":
    case "appear":
      return put(table, act.object, act.from, act.at, act.item);
    case "vanish": {
      if (act.object) return put(table, act.object, act.from, act.at);
      if (act.subject.kind === "player") return table;
      if (act.subject.kind === "ability") {
        const index = stackIndex(table, act.subject);
        const id = act.subject.id;
        return {
          ...table,
          view: { ...view, stack: without(view.stack, index) },
          abilities: table.abilities.filter((a) => a !== id),
        };
      }
      // A token or a copy that is no more: its slot is free.
      const slot = act.subject.slot;
      const gone = view.objects.find((o) => o.slot === slot);
      const rest = taken(table, slot, act.from, gone?.owner ?? "you");
      return {
        ...rest,
        view: {
          ...rest.view,
          objects: rest.view.objects.filter((o) => o.slot !== slot),
        },
      };
    }
    case "emerge":
      return {
        ...table,
        view: { ...view, stack: [...view.stack, act.item] },
        abilities: [...table.abilities, act.id],
      };
    case "flip":
    case "tap":
      return put(table, act.object);
    case "showcase":
      return heldUp(put(table, act.object), act.object, cause);
    case "strike":
    case "streak":
      return act.object ? put(table, act.object) : table;
    case "count":
      switch (act.tally.kind) {
        case "life":
          return player(act.tally.player, { life: act.tally.life });
        case "pool":
          return player(act.tally.player, { mana: act.tally.mana });
        default:
          return put(table, act.tally.object);
      }
    case "set": {
      const fact = act.fact;
      switch (fact.kind) {
        case "turn":
          return {
            ...table,
            view: {
              ...view,
              turn: {
                ...view.turn,
                number: fact.number,
                active: fact.active,
                step: fact.step,
              },
            },
          };
        case "object":
          return put(table, fact.object);
        case "library":
          return player(fact.player, { library: fact.order });
        default: {
          const index = stackIndex(table, fact.subject);
          const stack = view.stack.map((item, i) =>
            i === index ? fact.item : item,
          );
          return { ...table, view: { ...view, stack } };
        }
      }
    }
    // The cards the opponent is handling, from the step that begins the look to the one that
    // ends it: the view's own word for it (`PlayerView.handling`), and which cards they are.
    case "lift": {
      if ("tray" in act) {
        const { tray: _, ...lowered } = table;
        return act.span === "begin" ? { ...table, tray: act.tray } : lowered;
      }
      const { handling: _, ...rest } = view;
      const { handled: __, ...other } = table;
      return act.span === "begin"
        ? {
            ...other,
            view: { ...rest, handling: act.handling },
            handled: handledAt(view, act.handling),
          }
        : { ...other, view: rest };
    }
    // Waits, words and the other spans change nothing on the table.
    case "dwell":
    case "announce":
    case "point":
    case "mark":
      return table;
  }
}
