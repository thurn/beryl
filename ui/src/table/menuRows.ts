import type {
  CardEntry,
  CardTokenLink,
  LogLine,
  PlayerView,
  Side,
  TableObjectAction,
} from "../generated/protocol.ts";
import type { CardTarget, TableTarget } from "./client.ts";
import {
  commandsFor,
  type AppliesTo,
  type Assist,
  type Command,
} from "./commands.ts";
import { counterNames, type TokenChoice } from "./cardState.ts";
import { uses } from "./fill.ts";
import { manaColor } from "./players.ts";
import type { Row } from "./Menu.tsx";

export type PrintedCard = { card: CardEntry; face: number };

export interface MenuContext {
  target: TableTarget;
  view: PlayerView;
  /** Object references in the order the player selected them. */
  cards: readonly CardTarget[];
  tokens?: readonly CardTokenLink[];
  lastToken?: TokenChoice;
  printed?: ReadonlyMap<number, PrintedCard>;
  element?: HTMLElement;
  pointed?: Element | null;
  logEntry?: LogLine;
  viewer?: { revealed: boolean; wholeLibrary: boolean };
  key?: (id: string) => string | undefined;
}

export interface MenuCommand {
  id: string;
  command: Command;
  label: string;
  token?: CardTokenLink;
}

export const seatOf = (view: PlayerView, side: Side) => {
  const you = view.table?.seat ?? 0;
  return side === "you" ? you : 1 - you;
};

export function kindOf(target: TableTarget): AppliesTo {
  if (
    target.kind === "viewer_card" &&
    (target.object?.zone === "graveyard" || target.object?.zone === "exile")
  )
    return "pile_card";
  if (target.kind === "counter") return "player";
  if (target.kind === "step") return "table";
  return target.kind;
}

function canUseZone(view: PlayerView, side: Side, zone: string) {
  return (
    side === "you" ||
    (zone === "hand"
      ? !!view.table?.lentHand
      : !!view.table?.acting.some(
          (access) => access.side === side && access.zone === zone,
        ))
  );
}

export function allowed(
  command: Command,
  target: TableTarget,
  view: PlayerView,
): boolean {
  const table = view.table;
  if (!table) return false;
  if ("name" in target) {
    if (!target.action) return false;
    if (command.id === "permanent.reduce_life_by_power")
      return (
        table.seats === 2 && target.object?.types.includes("creature") === true
      );
    if (!target.object)
      return (
        command.id === "stack_entry.resolve" ||
        command.id === "card.remove" ||
        command.id === "table.every_action"
      );
    if (command.id === "permanent.peek")
      return (
        target.object.controller === "you" && target.object.facing === "down"
      );
  } else if ("side" in target) {
    const { side, kind } = target;
    if (seatOf(view, side) >= table.seats) return false;
    if (kind === "hand" && side !== "you") return false;
    if (kind === "library") {
      if (!canUseZone(view, side, kind)) return false;
      if (
        "always_reveal_top" in (command.action ?? {}) ||
        "always_look_at_top" in (command.action ?? {})
      )
        return side === "you";
      if (
        "draw" in (command.action ?? {}) ||
        "mulligan" in (command.action ?? {})
      )
        return canUseZone(view, side, "hand");
      if (command.id === "library.undo_draw") return side === "you";
      if (uses(command.action, "objects") && !view.players[side].library.length)
        return false;
    }
  }
  // A reveal has a recipient only on a two-seat table.
  return table.seats === 2 || !uses(command.action, "other_seat");
}

function conditions({
  target,
  view,
  cards,
  printed,
  viewer,
  tokens = [],
  logEntry,
  lastToken,
}: MenuContext) {
  const object = "name" in target ? target.object : undefined;
  const result = new Set<string>();
  if (logEntry) result.add("log_entry");
  if (viewer) result.add("open_viewer");
  if (viewer?.revealed) result.add("revealed_cards");
  if (viewer?.wholeLibrary) result.add("whole_library");
  if (
    cards.length &&
    cards.every(({ object }) => {
      const card = object && printed?.get(object.slot)?.card;
      return (
        card?.faces.length === 2 &&
        !["split", "adventure", "flip"].includes(card.layout)
      );
    })
  )
    result.add("two_faced");
  if (target.kind === "counter")
    result.add(
      manaColor(target.counter) ? "mana_selected" : "counter_selected",
    );
  if (view.turn.active === "you" && view.table?.phase === "untap")
    result.add("active_untap");
  if (view.turn.active === "you" && view.table?.phase === "draw")
    result.add("active_draw");
  if (object?.facing === "down") result.add("face_down");
  if (object?.attachedTo != null) result.add("attached");
  if (
    cards.some(
      ({ object }) => object && view.table?.manualStats?.includes(object.slot),
    )
  )
    result.add("manual_stats");
  if (tokens.length) {
    result.add("related_cards");
    result.add("card_tokens");
  }
  if (lastToken) result.add("last_token");
  view.table?.deckTokens?.forEach((_, index) =>
    result.add(`deck_token_${index + 1}`),
  );
  if (view.table?.lastDraw !== undefined) result.add("last_draw");
  if (
    object?.token ||
    ("name" in target && !object && target.kind === "stack_entry")
  )
    result.add("token_or_marker");
  return result;
}

function assists(view: PlayerView): ReadonlySet<Assist> {
  const a = view.table?.assists;
  const result = new Set<Assist>();
  if (a?.turnUpkeep) result.add("turn_upkeep");
  if (a?.combat) result.add("combat");
  if (a?.stateNudges) result.add("state_nudges");
  if (a?.libraryFlows) result.add("library_flows");
  if (a?.abilityMenus) result.add("ability_menus");
  return result;
}

export function checked(
  command: Command,
  { target, view, cards }: MenuContext,
) {
  if (command.id === "permanent.skip_untap")
    return (
      cards.length > 0 &&
      cards.every(({ object }) => object?.doesntUntap === true)
    );
  if (command.id === "library.keep_top_revealed" && target.kind === "library")
    return !!view.table?.topRevealed;
  const assist = command.id.replace(/^assist\./, "");
  if (command.id.startsWith("assist."))
    return assists(view).has(assist as Assist);
  if (command.id === "library.keep_top_known") return !!view.table?.topKnown;
  return undefined;
}

export function availableCommands(
  kind: AppliesTo,
  context: MenuContext,
): MenuCommand[] {
  const { cards, target, view, tokens = [] } = context;
  const object = "name" in target ? target.object : undefined;
  const [firstCounter] = counterNames(view, object?.slot);
  return commandsFor(kind, {
    scope: "table",
    conditions: conditions(context),
    assists: assists(view),
  }).flatMap((command): MenuCommand[] => {
    if (
      !allowed(command, target, view) ||
      cards.some(
        (card) =>
          !allowed(command, card, view) ||
          (uses(command.action, "objects") &&
            kindOf(card) !== command.applies_to &&
            command.also_applies_to?.[kindOf(card)] === undefined),
      )
    )
      return [];
    const { id, label } = command;
    if (
      id === "token.from_card" ||
      id === "token.attached" ||
      id === "card.related"
    )
      return tokens.map((token) => ({
        id: `${id}:${token.scryfallId}`,
        label:
          id === "card.related"
            ? token.name
            : label.replace("(name)", token.name),
        command,
        token,
      }));
    return [
      {
        id,
        label:
          id === "permanent.tap" &&
          cards.length > 0 &&
          cards.every(({ object }) => object?.tapped)
            ? "Untap"
            : label.replace("(name)", firstCounter),
        command,
      },
    ];
  });
}

export function menuRows(kind: AppliesTo, context: MenuContext) {
  const { target, view, cards, key, logEntry } = context;
  const ability =
    "name" in target && typeof target.name === "string"
      ? view.stack.find(
          (item) =>
            item.kind === "ability" && `a${item.manual?.id}` === target.name,
        )
      : undefined;
  const card = "name" in target ? target.object : undefined;
  const side =
    card?.owner ??
    (ability?.kind === "ability"
      ? ability.controller
      : "side" in target
        ? target.side
        : undefined);
  const pile =
    "side" in target
      ? {
          library: "Library",
          graveyard: "Graveyard",
          exile: "Exile",
          life: "Life",
          hand: "Hand",
          counter: "Counters",
          step: "Step",
          player: "Player",
          sideboard: "Sideboard",
        }[target.kind]
      : undefined;
  const count =
    side && (kind === "hand" || kind === "library")
      ? kind === "hand"
        ? view.table?.orders[side].hand.length
        : view.players[side].library.length
      : undefined;
  const header = {
    title: logEntry
      ? String(logEntry.n)
      : cards.length > 1
        ? `${cards.length} cards`
        : card?.name ||
          (ability?.kind === "ability" ? ability.name : pile) ||
          (target.kind === "table" ? "Table" : "Card"),
    whose: logEntry
      ? logEntry.by
      : count !== undefined
        ? `${count} ${count === 1 ? "card" : "cards"}${
            kind === "library"
              ? side === "you"
                ? " · yours"
                : " · opponent’s"
              : ""
          }`
        : side === "you"
          ? "Yours"
          : side === "opponent"
            ? "Opponent’s"
            : "",
  };
  const entries = availableCommands(kind, context)
    .filter(
      ({ command, id }) =>
        command.menu &&
        (!logEntry || id === "table.rewind") &&
        (id !== "token.all" || (context.tokens?.length ?? 0) > 1),
    )
    .sort((a, b) => a.command.menu!.order - b.command.menu!.order);
  const groups: { heading?: string; rows: Row[] }[] = [];
  for (const { id, command, label } of entries) {
    const { group, submenu } = command.menu!;
    const heading = [
      "Primary",
      "Card",
      "Library",
      "Life",
      "Hand",
      "Graveyard",
      "Exile",
      "",
    ].includes(group)
      ? undefined
      : group;
    let section = groups.at(-1);
    if (
      !section ||
      section.heading !== heading ||
      id === "table.every_action"
    ) {
      section = { heading, rows: [] };
      groups.push(section);
    }
    const click =
      [
        "permanent.tap",
        "hand_card.play",
        "stack_entry.resolve",
        "pile_card.play",
        "library.draw",
        "graveyard.view",
        "exile.view",
      ].includes(id) &&
      kind !== "their_permanent" &&
      (kind === "permanent" || kind === "stack_entry" || side !== "opponent");
    const row = {
      id,
      label,
      key: key?.(command.id),
      click,
      checked: checked(command, context),
    };
    const folder = command.id === "card.related" ? command.label : submenu;
    if (folder) {
      const existing = section.rows.find((item) => item.id === folder);
      if (existing) existing.submenu = [...existing.submenu!, row];
      else section.rows.push({ id: folder, label: folder, submenu: [row] });
    } else section.rows.push(row);
  }
  return { header, groups, entries };
}

export function actionReference(action: TableObjectAction) {
  return { id: action.reference, in: action.zone };
}
