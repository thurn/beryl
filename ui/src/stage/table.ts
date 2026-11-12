// The table: the part of a view a script accounts for, as the stage holds it between views
// (rust/crates/beryl_session/src/script/mod.rs, "The table and replay"). `apply` is that
// module's replay, one step at a time: each step carries all of what it changes, so the stage
// shows the table after every step without the view that follows.
import type {
  Act,
  ObjectView,
  PlayerView,
  Reset,
  Side,
  StackItem,
  Subject,
  ZoneName,
} from "../generated/protocol";

/** A view whose table the steps so far have changed, and the numbers its scripts gave the
 * abilities on its stack, bottom first: a view's stack does not name them. The rest of the
 * view is the chrome of the last view taken whole. */
export interface Table {
  view: PlayerView;
  abilities: readonly number[];
  /** The cards the opponent is handling, as they were when the look began: the same cards
   * stay lifted while it lasts, whatever order it leaves their pile in. */
  handled?: readonly number[];
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

/** The table of a view the stage starts over from. */
export const tableOf = (view: PlayerView, reset?: Reset): Table => ({
  view: tabled(view),
  abilities: reset?.abilities ?? [],
});

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
  const next = { ...table, view: { ...table.view, objects } };
  const zone = before?.zone ?? from;
  if (zone === object.zone && at === undefined) return next;
  return placed(
    taken(next, object.slot, zone, before?.owner ?? object.owner),
    object,
    at,
    item,
  );
}

/** The table after the step `act`. */
export function apply(table: Table, act: Act): Table {
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
        view: { ...view, stack: [...view.stack, act.item] },
        abilities: [...table.abilities, act.id],
      };
    case "flip":
    case "tap":
    case "showcase":
      return put(table, act.object);
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
