import { counterName } from "../game/log";
import type { TableAction, TableZone } from "../generated/protocol";
import {
  baseStats,
  statValue,
  storeToken,
  type TokenChoice,
} from "./cardState";
import { sameZone, type CardTarget } from "./client";
import { fill, uses, type ActionValues } from "./fill";
import { advancePhase, untapPermanents } from "./turn";
import {
  actionReference,
  checked,
  counterNames,
  seatOf,
  type MenuCommand,
  type MenuContext,
} from "./menuRows";

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

export function commandActions(
  entry: MenuCommand,
  context: MenuContext,
  values: ActionValues = {},
  faceDown = false,
): TableAction[] {
  const { target, view } = context;
  const { command } = entry;
  if (command.id === "table.untap_all" || command.id === "phase.untap_active")
    return [untapPermanents(view)];
  if (command.id === "phase.next" || command.id === "phase.next_with_action")
    return advancePhase(view, command.id === "phase.next_with_action");
  const template = command.action;
  if (!template) return [];
  const side = "side" in target ? target.side : "you";
  const seat = seatOf(view, side);
  const handSize = view.table?.orders[side].hand.length ?? 0;
  const args: ActionValues = {
    seat,
    other_seat: 1 - seatOf(view, "you"),
    owner: seat,
    entry: context.logEntry?.n ?? view.table?.lastDraw,
    count: 1,
    text: target.kind === "counter" ? target.counter : null,
    on: !checked(command, context),
    tapped: context.cards.some(({ object }) => !object?.tapped),
    target:
      command.id === "token.attached" && "name" in target && target.action
        ? actionReference(target.action)
        : undefined,
    ...values,
  };
  if (command.id === "hand.mulligan_same_size") args.number = handSize;
  if (command.id === "hand.mulligan_one_fewer")
    args.number = Math.max(0, handSize - 1);
  if (command.id === "permanent.reduce_life_by_power") {
    const reductions = new Map<number, number>();
    for (const { object } of context.cards) {
      if (!object?.pt) continue;
      const recipient = 1 - seatOf(view, object.owner);
      reductions.set(
        recipient,
        (reductions.get(recipient) ?? 0) - object.pt.power,
      );
    }
    return [...reductions].map(([seat, number]) =>
      fill(template, { ...args, seat, number }),
    );
  }
  if (command.id === "counter.increment_all") {
    return context.cards.flatMap(({ action, object }) => {
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
  if (command.id === "token.all")
    return (context.tokens ?? []).map((token) => ({
      token: {
        card: storeToken(token).card,
        count: 1,
        attached_to: null,
        note: null,
        face_down: faceDown,
      },
    }));

  if ("token" in template) {
    const token = tokenChoice(entry, context);
    const action = fill(template, {
      ...args,
      card: token?.card,
      text: token?.note ?? args.text,
      ...values,
    });
    if ("token" in action && faceDown) action.token.face_down = true;
    return [action];
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
          stats: [
            statValue(power + adjust[0]),
            statValue(toughness + adjust[1]),
          ],
        }),
      ];
    });

  if (
    uses(template, "first_counter") ||
    uses(template, "second_counter") ||
    uses(template, "third_counter")
  ) {
    return cards.flatMap(({ object, action }) => {
      if (!action) return [];
      const [first_counter, second_counter, third_counter] = counterNames(
        object,
        object && context.printed?.get(object.slot),
      );
      return [
        fill(template, {
          ...args,
          objects: [action.reference],
          in: action.zone,
          first_counter,
          second_counter,
          third_counter,
        }),
      ];
    });
  }

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

function tokenChoice(
  entry: MenuCommand,
  context: MenuContext,
): TokenChoice | undefined {
  if (entry.token) return storeToken(entry.token);
  if (entry.id === "token.repeat") return context.lastToken;
  if (entry.id.startsWith("token.deck_")) {
    const index = Number(entry.id.slice("token.deck_".length)) - 1;
    const token = context.view.table?.deckTokens?.[index];
    if (token) return storeToken(token);
  }
}
