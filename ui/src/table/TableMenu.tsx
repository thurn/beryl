import { useLayoutEffect } from "react";
import { requestBugReport } from "../components/BugReport";
import { cardRecords } from "../cardRecords";
import { useCardRecords } from "../cards";
import type { LogLine, PlayerView } from "../generated/protocol";
import { askCards, cardTokens } from "../ipc";
import { BLANK } from "../game/faces";
import { Inspect } from "../game/Piles";
import { isPhone } from "../mobile/phone";
import type { Point } from "../stage/layout";
import type { Under } from "../stage/pointer";
import { desktopDropAreas } from "./dropZones";
import { AskPrompt, type AskPromptProps } from "./AskPrompt";
import type { Bindings } from "./bindings";
import type { TableClient, TableTarget } from "./client";
import { commands as registry, type Command } from "./commands";
import { currentTarget } from "./keyboard";
import { selectAt } from "./select";
import { playerCounterValue } from "./players";
import { describe } from "./shortcuts";
import { TablePalette } from "./TablePalette";
import { commandActions } from "./commandActions";
import { activateAbility } from "./abilities";
import { flowLabel, usesLookSort } from "./flows";
import { canUseZone } from "./zoneAccess";
import {
  baseStats,
  counterNames,
  counterSuggestions,
  counterValue,
  MAX_INT,
  MIN_INT,
  normalizedCounter,
  type TokenChoice,
} from "./cardState";
import { TokenSearch } from "./TokenSearch";
import type { ActionValues } from "./fill";
import { sendHistory, type HistoryAction } from "./tableLog";
import { Menu, type Row } from "./Menu";
import { DragFace } from "./DragFace";
import {
  actionReference,
  availableCommands,
  kindOf,
  menuRows,
  pileMoveLimit,
  pileSlots,
  type MenuCommand,
  type MenuContext,
} from "./menuRows";

export type TableOverlay =
  | { kind: "menu"; context: MenuContext; at: Point }
  | { kind: "ask"; prompt: AskPromptProps; context: MenuContext; step?: string }
  | {
      kind: "list";
      context: MenuContext;
      settings?: boolean;
      contextual?: boolean;
    }
  | { kind: "related"; name: string; context: MenuContext }
  | {
      kind: "tokens";
      context: MenuContext;
      choose: (token: TokenChoice, shift: boolean) => void;
    };

const table: TableTarget = { kind: "table" };
const defaults: readonly TableTarget[] = [
  { kind: "library", side: "you" },
  { kind: "hand", side: "you" },
  { kind: "life", side: "you" },
  { kind: "player", side: "you" },
  { kind: "graveyard", side: "you" },
  { kind: "exile", side: "you" },
  { kind: "sideboard", side: "you" },
];

const applies = (command: Command, target: TableTarget) => {
  const kind = kindOf(target);
  return (
    command.applies_to === kind || command.also_applies_to?.[kind] !== undefined
  );
};

export class TableCommands {
  #overlay?: TableOverlay;
  #lastToken?: TokenChoice;
  /** Pile viewers install their client-only commands here. */
  readonly clients = new Map<string, (context: MenuContext) => void>();

  constructor(
    readonly client: TableClient,
    readonly bindings: Bindings,
    readonly show: (overlay: TableOverlay | undefined) => void,
    private readonly report: (error: unknown) => void,
    private readonly leave: () => void,
    private readonly quit: () => void,
    private readonly showBlockers: (entries?: readonly number[]) => void,
  ) {
    client.onSecondary((target, at, element) => {
      if (isPhone() && this.overlayOpen) return true;
      if (!target) {
        // The desktop hand has no separate control behind its fan.
        if (
          client.surface?.hits(at).some((item) => item.matches(".phone-board"))
        )
          return;
        const area = desktopDropAreas().find(
          ({ kind, box }) =>
            kind === "hand" &&
            at.x >= box.left &&
            at.x <= box.right &&
            at.y >= box.top &&
            at.y <= box.bottom,
        );
        if (!area) return;
        target = { kind: "hand", side: area.side };
      }
      if (target.kind === "counter") return;
      const point = element ? at : client.surface?.viewportPoint(at);
      if (point) this.open(target, point, element);
      return true;
    });
  }

  get overlayOpen() {
    return this.#overlay !== undefined;
  }

  #set(overlay?: TableOverlay) {
    this.#overlay = overlay;
    this.show(overlay);
  }

  readonly close = () => this.#set();
  readonly reportError = (error: unknown) => this.report(error);

  readonly history = (action: HistoryAction) =>
    sendHistory(this.client, action, this.showBlockers).catch(this.report);

  readonly openLogLine = (logEntry: LogLine, at: Point) => {
    const context = this.context(table);
    if (context)
      this.#set({ kind: "menu", context: { ...context, logEntry }, at });
  };

  context(
    target: TableTarget,
    view = this.client.view,
  ): MenuContext | undefined {
    if (!view?.table) return;
    const cards = !("name" in target)
      ? []
      : this.client.selection.has(target.name)
        ? [...this.client.selection].flatMap(
            (name) =>
              this.client.card(name) ?? this.client.card(name, true) ?? [],
          )
        : [target];
    const printed = new Map(
      cards.flatMap(({ object }) => {
        if (!object?.name || object.customColors) return [];
        const found = cardRecords.find(object.tokenPrinting ?? object.name);
        if (!found) return [];
        const face =
          object.tokenPrinting && object.face === "back" ? 1 : found.face;
        return [[object.slot, { card: found.card, face }] as const];
      }),
    );
    const viewer =
      target.kind === "viewer_card"
        ? this.client.viewers?.snapshot()
        : undefined;
    return {
      target,
      view,
      cards,
      printed,
      lastToken: this.#lastToken,
      viewer: viewer && {
        revealed: viewer.zone === "revealed",
        wholeLibrary: viewer.zone === "library" && viewer.portion === "all",
        canShuffle:
          viewer.zone === "library" && canUseZone(view, viewer.side, "library"),
      },
      pointed: this.client.surface?.pointed(),
      key: (id) => describe(this.bindings.get(id), this.bindings.platform),
    };
  }

  open(target: TableTarget, at: Point, element?: HTMLElement) {
    const context = this.context(target);
    if (!context) return;
    const { groups } = menuRows(kindOf(target), context);
    if (!groups.length && target.kind !== "hand") return;
    if (
      target.kind === "hand" &&
      target.side === "opponent" &&
      context.view.table?.seats !== 2
    )
      return;
    // WebKit may blur the clicked button before delivering contextmenu.
    if (element && element.tabIndex >= 0)
      element.focus({ preventScroll: true });
    context.element = element;
    this.#showWithTokens({ kind: "menu", context, at });
  }

  readonly openSelection = (element: HTMLElement) => {
    const [name] = this.client.selection;
    if (name === undefined) return;
    const target = this.client.card(name) ?? this.client.card(name, true);
    if (target) this.open(target, { x: 0, y: 0 }, element);
  };

  #showWithTokens(overlay: TableOverlay) {
    this.#set(overlay);
    void this.#withTokens(overlay.context)
      .then((context) => {
        if (
          context !== overlay.context &&
          this.#overlay === overlay &&
          this.client.view === context.view
        )
          this.#set({ ...overlay, context });
      })
      .catch(this.report);
  }

  async #withTokens(context: MenuContext): Promise<MenuContext> {
    const { target } = context;
    if (!("name" in target) || context.tokens || target.object?.customColors)
      return context;
    const card = target.object;
    const marker =
      !card && typeof target.name === "string"
        ? context.view.stack.find(
            (item) =>
              item.kind === "ability" && `a${item.manual?.id}` === target.name,
          )
        : undefined;
    const name =
      (card && context.printed?.get(card.slot)?.card.name) ??
      card?.name ??
      (marker?.kind === "ability" ? marker.name : undefined);
    if (!name || card?.facing === "down") return context;
    const tokens = await cardTokens(name);
    return tokens.length ? { ...context, tokens } : context;
  }

  #supports({ command }: MenuCommand) {
    return (
      !command.client ||
      this.clients.has(command.client) ||
      [
        "open_command_list",
        "activate_ability",
        "view_related_cards",
        "select_all",
        "select_row",
        "select_column",
        "report_bug",
        "leave_table",
        "open_shortcuts",
      ].includes(command.client)
    );
  }

  list(context: MenuContext, contextual = false) {
    const targets = contextual
      ? []
      : [table, ...defaults].filter(
          (target) => target.kind !== context.target.kind,
        );
    const entries = [
      context,
      ...targets.flatMap((target) => {
        const next = this.context(target);
        if (!next) return [];
        return [
          target.kind === "table" ? { ...next, tokens: context.tokens } : next,
        ];
      }),
    ].flatMap((context) =>
      availableCommands(kindOf(context.target), context)
        .filter(
          (entry) =>
            this.#supports(entry) &&
            entry.command.client !== "open_command_list",
        )
        .map((entry) => ({ entry, context })),
    );
    const seen = new Set<string>();
    return entries.filter(({ entry }) => {
      if (seen.has(entry.id)) return false;
      seen.add(entry.id);
      return true;
    });
  }

  openShortcuts = () => this.openList(currentTarget(this.client), true);

  openList(target = currentTarget(this.client), settings = false) {
    const context = this.context(target ?? table);
    if (context) this.#showWithTokens({ kind: "list", context, settings });
  }

  async shortcut(id: string, pointed?: TableTarget, shift = false) {
    if (id === "token.all") {
      // Shift belongs to the All tokens binding. With no token-making card,
      // the same chord opens a chooser for one face-down token.
      if (pointed && (await this.runAt(id, pointed))) return;
      if (!this.overlayOpen)
        await this.runAt("token.create", table, undefined, shift);
      return;
    }
    if (id === "table.command_list") {
      this.openList(pointed);
      return;
    }
    if (id === "viewer.hide_revealed") {
      this.client.viewers?.hideRevealed();
      return;
    }
    const command = registry.find((command) => command.id === id);
    if (!command) return;
    const target =
      pointed && applies(command, pointed)
        ? pointed
        : [table, ...defaults].find((target) => applies(command, target));
    if (target) await this.runAt(id, target, undefined, shift);
  }

  async runAt(
    id: string,
    target: TableTarget,
    element?: HTMLElement,
    shift = false,
  ) {
    let context = this.context(target);
    if (!context) return;
    context.element = element;
    if (id === "token.all") context = await this.#withTokens(context);
    const entry = availableCommands(kindOf(target), context).find(
      (entry) => entry.id === id && this.#supports(entry),
    );
    if (!entry) return;
    if (id === "table.leave") this.quit();
    else await this.run(entry, context, shift);
    return true;
  }

  async run(entry: MenuCommand, context: MenuContext, shift = false) {
    const { client } = this;
    if (client.frozen || context.view !== client.view) return;
    const { command, token } = entry;
    const { target } = context;
    const source = "name" in target ? target : undefined;
    const object = source?.object;
    if (entry.ability) {
      activateAbility(
        client,
        entry.ability,
        context,
        this.close,
        this.report,
        (prompt) => this.#set({ kind: "ask", prompt, context }),
      );
      return;
    }
    const send = (values?: ActionValues, faceDown = shift) => {
      if (context.view !== client.view) return;
      const actions = commandActions(entry, context, values, faceDown);
      const [first] = actions;
      if (first && "look" in first && client.viewers) {
        void client.viewers
          .look(first, context.view, context.element)
          .catch(this.report);
      } else if (first && ("undo" in first || "rewind" in first)) {
        void this.history(first);
      } else if (first)
        void client
          .send(
            actions.length === 1 ? first : { batch: { actions } },
            context.element ?? source?.name,
          )
          .then((reply) => {
            const last = actions.at(-1);
            if ("ok" in reply && last && "token" in last)
              this.#lastToken = {
                card: last.token.card,
                note: last.token.note,
              };
          })
          .catch(this.report);
    };

    if (command.client === "open_command_list") {
      this.#showWithTokens({ kind: "list", context, contextual: true });
      return;
    }
    if (command.client === "view_related_cards" && token) {
      this.#set({ kind: "related", name: token.name, context });
      return;
    }
    switch (command.client) {
      case "select_all":
        selectAt(client, "selection.all", context.pointed);
        return;
      case "select_row":
        selectAt(client, "selection.row", context.pointed);
        return;
      case "select_column":
        selectAt(client, "selection.column", context.pointed);
        return;
      case "report_bug":
        requestBugReport();
        return;
      case "open_shortcuts":
        this.openShortcuts();
        return;
      case "leave_table":
        this.leave();
        return;
      case undefined:
        break;
      default:
        this.clients.get(command.client)?.(context);
        return;
    }

    if (command.asks === "target") {
      if (!source || !client.arrows) return;
      const attach = command.id !== "card.arrow";
      const hit = await client.arrows.pickTarget(
        source.name,
        attach
          ? (under) => this.#attachment(under, context) !== undefined
          : undefined,
      );
      if (!hit || context.view !== client.view) return;
      if (!attach)
        await client.arrows.draw(
          source,
          hit,
          context.cards.map(({ name }) => name),
        );
      else {
        const target = this.#attachment(hit, context);
        if (target) send({ target });
      }
      return;
    }

    if (command.asks === "token") {
      const previous = this.#overlay;
      const related = !context.tokens && currentTarget(client);
      const picked = related && this.context(related, context.view);
      const { tokens } = picked ? await this.#withTokens(picked) : context;
      if (context.view !== client.view || previous !== this.#overlay) return;
      this.#set({
        kind: "tokens",
        context: { ...context, tokens },
        choose: ({ card, note }, chosenWithShift) => {
          this.close();
          send({ card, text: note }, shift || chosenWithShift);
        },
      });
      return;
    }

    const show = (prompt: AskPromptProps, step?: string) =>
      this.#set({ kind: "ask", prompt, context, step });
    const confirm = (values: ActionValues) => {
      this.close();
      send(values);
    };
    const label = entry.label.replace(/…$/, "");
    if (command.id === "table.roll") {
      show({
        kind: "number",
        start: 6,
        min: 2,
        max: 1_000_000,
        fieldLabel: "Sides",
        label: () => "Next",
        onCancel: this.close,
        onConfirm: (number) =>
          show(
            {
              kind: "number",
              start: 1,
              min: 1,
              max: 100,
              fieldLabel: "Dice",
              label: (count) => `Roll ${count}d${number}`,
              onCancel: this.close,
              onConfirm: (count) => confirm({ number, count }),
            },
            "dice",
          ),
      });
    } else if (command.asks === "text" || command.asks === "text_and_number") {
      const stackUntil = command.id === "library.stack_until";
      const namedCounter = command.id === "counter.add_named";
      show({
        kind: "text",
        suggestions: namedCounter ? counterSuggestions : undefined,
        start: command.id === "permanent.note" ? (object?.note ?? "") : "",
        fieldLabel: stackUntil ? "Card name or type" : label,
        label: () =>
          stackUntil
            ? "Next"
            : command.id === "permanent.note"
              ? "Set note"
              : label,
        onCancel: this.close,
        onConfirm: (text) => {
          if (namedCounter) {
            const name = normalizedCounter(text);
            if (name) confirm({ text: name });
            return;
          }
          if (command.asks === "text_and_number" && !text.trim()) return;
          if (command.asks === "text_and_number") {
            show({
              kind: "number",
              start: 1,
              min: 1,
              max: stackUntil ? 65535 : MAX_INT,
              fieldLabel: stackUntil ? "Hits" : undefined,
              label: (number) =>
                stackUntil ? "Put cards on stack" : `Add ${number} ${text}`,
              onCancel: this.close,
              onConfirm: (number) => confirm({ text, number }),
            });
          } else
            confirm({
              text: command.id === "permanent.note" && !text ? null : text,
            });
        },
      });
    } else if (command.asks === "number") {
      const life =
        target.kind === "life"
          ? context.view.players[target.side].life
          : undefined;
      const playerCounter = "player_counter" in (command.action ?? {});
      const playerCounterName =
        target.kind === "counter" && !command.id.startsWith("player.storm_")
          ? target.counter
          : "storm";
      const playerValue =
        playerCounter && "side" in target
          ? playerCounterValue(context.view, target.side, playerCounterName)
          : undefined;
      const counter = command.id.startsWith("counter.");
      const counterSlot = command.id.includes(".second_")
        ? 1
        : command.id.includes(".third_")
          ? 2
          : 0;
      const name = counterNames(context.view, object?.slot)[counterSlot];
      const current = counter
        ? counterValue(object, name)
        : (life ?? playerValue);
      const count =
        target.kind === "library"
          ? (pileMoveLimit(command, target, context.view) ??
            pileSlots(context).length)
          : undefined;
      const verb =
        command.id === "library.draw_cards"
          ? "Draw"
          : command.id === "library.move_top_cards_to_graveyard"
            ? "Mill"
            : label;
      show({
        kind: "number",
        start:
          current ??
          (command.id === "hand.mulligan"
            ? (context.view.table?.orders[
                target.kind === "hand" ? target.side : "you"
              ].hand.length ?? 7)
            : 1),
        current,
        min:
          life !== undefined
            ? MIN_INT
            : counter || playerCounter || command.id === "hand.mulligan"
              ? 0
              : 1,
        max:
          life !== undefined || playerCounter
            ? MAX_INT
            : counter
              ? 999
              : Math.max(1, Math.min(65535, count ?? 65535)),
        fieldLabel:
          command.id === "library.draw_cards" ? "Cards to draw" : label,
        label: (number) =>
          command.id === "library.look_top" && usesLookSort(context)
            ? flowLabel({ kind: "look_at_top", count: number })
            : `${verb} ${number}`,
        onCancel: this.close,
        onConfirm: (number) => confirm({ number }),
      });
    } else if (command.asks === "stats") {
      const start = baseStats(context.view, object?.slot);
      show({
        kind: "stats",
        start,
        current: start,
        min: MIN_INT,
        max: MAX_INT,
        label: ({ power, toughness }) => `Set ${power}/${toughness}`,
        onCancel: this.close,
        onConfirm: ({ power, toughness }) =>
          confirm({ stats: [power, toughness] }),
      });
    } else send();
  }

  #attachment(hit: Under, context: MenuContext) {
    if (!("name" in hit) || context.cards.some(({ name }) => name === hit.name))
      return;
    const chosen = this.client.card(hit.name);
    if (chosen?.action?.zone === "battlefield")
      return actionReference(chosen.action);
  }
}

export function TableMenu({
  commands,
  overlay,
  view,
  phone,
}: {
  commands: TableCommands;
  overlay?: TableOverlay;
  view: PlayerView;
  phone: boolean;
}) {
  useLayoutEffect(() => {
    if (!overlay) return;
    if (view !== overlay.context.view) commands.close();
  }, [commands, overlay, view]);
  useLayoutEffect(() => {
    if (overlay) return commands.client.pushOpen(commands.close);
  }, [commands, overlay?.kind]);

  if (!overlay || view !== overlay.context.view) return null;
  const { context } = overlay;
  if (overlay.kind === "ask")
    return (
      <AskPrompt
        {...overlay.prompt}
        key={overlay.step ?? overlay.prompt.kind}
      />
    );
  if (overlay.kind === "related")
    return <RelatedCard name={overlay.name} onClose={commands.close} />;
  if (overlay.kind === "tokens")
    return (
      <TokenSearch
        related={context.tokens}
        onChoose={overlay.choose}
        onClose={commands.close}
        phone={phone}
      />
    );
  const { header, groups, entries } = menuRows(kindOf(context.target), context);
  const choose = (entries: MenuCommand[], id: string, shift: boolean) => {
    const entry = entries.find((entry) => entry.id === id);
    if (entry)
      void commands.run(entry, context, shift).catch(commands.reportError);
  };
  if (overlay.kind === "list")
    return (
      <TablePalette
        commands={commands}
        context={context}
        phone={phone}
        settings={overlay.settings}
        contextual={overlay.contextual}
      />
    );
  const { target, cards } = context;
  const touchRows: Row[] = [];
  const faceDown =
    phone && kindOf(target) === "pile_card"
      ? availableCommands("pile_card", context).find(
          (entry) => entry.id === "hand_card.play_face_down",
        )
      : undefined;
  if (faceDown) touchRows.push({ id: faceDown.id, label: faceDown.label });
  const preview = phone && cards.length === 1 ? cards[0]?.object : undefined;
  const pile =
    target.kind === "library" ||
    target.kind === "graveyard" ||
    target.kind === "exile"
      ? target
      : undefined;
  if (
    phone &&
    "name" in target &&
    target.action &&
    !commands.client.selectingSnapshot()
  )
    touchRows.push({ id: "touch.select", label: "Select" });
  if (phone && pile && commands.client.drag?.canTakeBottom(pile))
    touchRows.push({ id: "touch.bottom", label: "Bottom card" });
  return (
    <Menu
      header={header}
      preview={preview && <DragFace object={preview} width={144} />}
      groups={
        touchRows.length
          ? [...groups.slice(0, 1), { rows: touchRows }, ...groups.slice(1)]
          : groups
      }
      at={overlay.at}
      presentation={phone ? "sheet" : "menu"}
      onClose={commands.close}
      onChoose={(id, shift) => {
        if (id === "touch.select") commands.client.setSelecting(true);
        else if (id === "touch.bottom" && pile)
          commands.client.drag?.takeBottom(pile);
        else choose(faceDown ? [...entries, faceDown] : entries, id, shift);
      }}
    />
  );
}

function RelatedCard({ name, onClose }: { name: string; onClose: () => void }) {
  useCardRecords([name], askCards);
  return (
    <div className="table-menu-layer" onClick={onClose}>
      <Inspect object={{ ...BLANK, name, token: true }} place={{ left: 24 }} />
    </div>
  );
}
