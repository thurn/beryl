import { counterName } from "../game/log";
import type {
  ObjectView,
  PlayerView,
  TableAction,
  TableActionPhase,
  TablePhase,
  TableZone,
} from "../generated/protocol";
import { sameZone, type CardTarget } from "./client";
import { fill, uses, type ActionValues } from "./fill";
import {
  actionReference,
  checked,
  counterNames,
  seatOf,
  type MenuCommand,
  type MenuContext,
} from "./menuRows";

export function counterValue(object: ObjectView | undefined, name: string) {
  return (
    object?.counters.find(
      ({ kind }) => counterName(kind).replaceAll("−", "-") === name,
    )?.count ??
    object?.namedCounters?.find((counter) => counter.name === name)?.count ??
    0
  );
}

export function baseStats(object?: ObjectView) {
  if (!object?.pt) return { power: 0, toughness: 0 };
  const bonus = counterValue(object, "+1/+1") - counterValue(object, "-1/-1");
  return {
    power: object.pt.power - bonus,
    toughness: object.pt.toughness - bonus,
  };
}

const statChanges: Record<string, readonly [number, number]> = {
  "stats.power_up": [1, 0],
  "stats.power_down": [-1, 0],
  "stats.toughness_up": [0, 1],
  "stats.toughness_down": [0, -1],
  "stats.both_up": [1, 1],
  "stats.both_down": [-1, -1],
  "stats.power_up_toughness_down": [1, -1],
  "stats.power_down_toughness_up": [-1, 1],
};

export function pileSlots({ target, view }: MenuContext) {
  if (target.kind === "library" || target.kind === "graveyard")
    return view.players[target.side][target.kind];
  if (target.kind === "hand" || target.kind === "exile")
    return view.table?.orders[target.side][target.kind] ?? [];
  return [];
}

const phases = {
  untap: "untap",
  upkeep: "upkeep",
  draw: "draw",
  firstMain: "first_main",
  beginningOfCombat: "beginning_of_combat",
  declareAttackers: "declare_attackers",
  declareBlockers: "declare_blockers",
  combatDamage: "combat_damage",
  endOfCombat: "end_of_combat",
  secondMain: "second_main",
  end: "end",
} as const satisfies Record<TablePhase, TableActionPhase>;

export function nextPhase(phase: TablePhase): TableActionPhase | undefined {
  const order = Object.values(phases);
  return order[order.indexOf(phases[phase]) + 1];
}

function untapAll(view: PlayerView): TableAction {
  const tapped = new Set(
    view.objects
      .filter(
        (object) =>
          object.zone === "battlefield" &&
          object.controller === "you" &&
          object.tapped &&
          !object.doesntUntap,
      )
      .map(({ slot }) => slot),
  );
  const objects = (view.table?.objectActions ?? []).flatMap(
    ({ subject, reference }) =>
      subject.kind === "object" && tapped.has(subject.slot) ? [reference] : [],
  );
  return { tap: { objects, in: "battlefield", tapped: false } };
}

export function commandActions(
  entry: MenuCommand,
  context: MenuContext,
  values: ActionValues = {},
): TableAction[] {
  const { target, view } = context;
  const { command, token } = entry;
  if (command.id === "table.untap_all" || command.id === "phase.untap_active")
    return [untapAll(view)];
  if (command.id === "phase.next" || command.id === "phase.next_with_action") {
    const phase = view.table && nextPhase(view.table.phase);
    const advance: TableAction = phase
      ? { phase: { phase } }
      : { next_turn: {} };
    if (command.id === "phase.next" || view.turn.active !== "you")
      return [advance];
    if (phase === "draw")
      return [advance, { draw: { seat: seatOf(view, "you"), count: 1 } }];
    if (!phase && view.table?.seats === 1) return [advance, untapAll(view)];
    return [advance];
  }
  const template = command.action;
  if (!template) return [];
  const side = "side" in target ? target.side : "you";
  const seat = seatOf(view, side);
  const object = "name" in target ? target.object : undefined;
  const [first_counter, second_counter, third_counter] = counterNames(
    object,
    object && context.printed?.get(object.slot),
  );
  const handSize = view.table?.orders[side].hand.length ?? 0;
  const args: ActionValues = {
    seat,
    other_seat: 1 - seatOf(view, "you"),
    owner: seat,
    entry: view.table?.lastDraw,
    count: 1,
    text: target.kind === "counter" ? target.counter : null,
    on: !checked(command, context),
    tapped: context.cards.some(({ object }) => !object?.tapped),
    first_counter,
    second_counter,
    third_counter,
    card: token ? { named: token.name } : undefined,
    target:
      command.id === "token.attached" && "name" in target && target.action
        ? actionReference(target.action)
        : undefined,
    ...values,
  };
  if (command.id === "hand.mulligan_same_size") args.number = handSize;
  if (command.id === "hand.mulligan_one_fewer")
    args.number = Math.max(0, handSize - 1);
  if (command.id === "permanent.reduce_life_by_power")
    args.number = -context.cards.reduce(
      (total, card) => total + (card.object?.pt?.power ?? 0),
      0,
    );
  if (command.id === "counter.increment_all") {
    args.actions = context.cards.flatMap(({ action, object }) => {
      if (!action || !object) return [];
      const names = [
        ...object.counters.map(({ kind }) =>
          counterName(kind).replaceAll("−", "-"),
        ),
        ...(object.namedCounters ?? []).map(({ name }) => name),
      ];
      return names.map((name): TableAction => ({
        counter: { name, add: 1, objects: [action.reference], in: action.zone },
      }));
    });
  }
  if (command.id === "token.all") {
    args.actions = (context.tokens ?? []).map(({ name }) => ({
      token: { card: { named: name }, count: 1, attached_to: null, note: null },
    }));
  }

  if (!uses(template, "objects")) {
    if (
      target.kind === "hand" ||
      target.kind === "library" ||
      target.kind === "graveyard" ||
      target.kind === "exile"
    )
      args.in = { [target.kind]: seat };
    return [fill(template, args)];
  }

  let cards = context.cards;
  if (
    target.kind === "library" ||
    target.kind === "hand" ||
    target.kind === "graveyard" ||
    target.kind === "exile"
  ) {
    const order = pileSlots(context);
    const all = target.kind !== "library";
    const count = Number(values.number ?? 1);
    const selected = all
      ? target.kind === "hand"
        ? order
        : [...order].reverse()
      : command.id.includes("bottom")
        ? order.slice(0, count)
        : order.slice(-count).reverse();
    const bySlot = new Map(view.objects.map((object) => [object.slot, object]));
    const refs = new Map(
      (view.table?.objectActions ?? []).flatMap((action) =>
        action.subject.kind === "object"
          ? [[action.subject.slot, action] as const]
          : [],
      ),
    );
    cards = selected.map((name): CardTarget => ({
      kind: "viewer_card",
      name,
      object: bySlot.get(name),
      action: refs.get(name),
    }));
    if (cards.some(({ action }) => !action)) return [];
  }

  const adjust = statChanges[command.id];
  if (adjust)
    return cards.flatMap(({ object, action }) => {
      if (!action) return [];
      const { power, toughness } = baseStats(object);
      return [
        fill(template, {
          ...args,
          objects: [action.reference],
          in: action.zone,
          stats: [power + adjust[0], toughness + adjust[1]],
        }),
      ];
    });

  const groups: Array<{
    zone: TableZone;
    owner: number;
    face: string;
    refs: NonNullable<CardTarget["action"]>["reference"][];
  }> = [];
  for (const { action, object } of cards) {
    if (!action) continue;
    const owner = object ? seatOf(view, object.owner) : seat;
    const face =
      command.id === "card.transform"
        ? object?.face === "back"
          ? "front"
          : "back"
        : object?.face === "down"
          ? "front"
          : "down";
    const last = groups.at(-1);
    if (
      last &&
      sameZone(last.zone, action.zone) &&
      (!uses(template, "owner") || last.owner === owner) &&
      (!uses(template, "face") || last.face === face)
    )
      last.refs.push(action.reference);
    else
      groups.push({ zone: action.zone, owner, face, refs: [action.reference] });
  }
  // Each group moves as one action. Top insertion reverses the order of groups.
  if (command.id === "move.library_top" || command.id === "move.library_nth")
    groups.reverse();
  return groups.map(({ zone, owner, face, refs }) =>
    fill(template, {
      ...args,
      objects: refs,
      in: zone,
      owner,
      face,
    }),
  );
}
