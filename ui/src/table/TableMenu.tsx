import { useLayoutEffect } from "react";
import { cardRecords } from "../cardRecords";
import { useCardRecords } from "../cards";
import type { PlayerView, TableCard } from "../generated/protocol";
import { askCards, cardTokens } from "../ipc";
import { BLANK } from "../game/faces";
import { Inspect } from "../game/Piles";
import type { Point } from "../stage/layout";
import type { Under } from "../stage/pointer";
import { desktopDropAreas } from "./dropZones";
import { AskPrompt, type AskPromptProps } from "./AskPrompt";
import type { Bindings } from "./bindings";
import type { TableClient, TableTarget } from "./client";
import { CommandList } from "./CommandList";
import {
  baseStats,
  commandActions,
  counterValue,
  pileSlots,
} from "./commandActions";
import type { ActionValues } from "./fill";
import { Menu } from "./Menu";
import {
  actionReference,
  availableCommands,
  counterNames,
  kindOf,
  menuRows,
  type MenuCommand,
  type MenuContext,
} from "./menuRows";

export type TableOverlay =
  | { kind: "menu"; context: MenuContext; at: Point }
  | { kind: "ask"; prompt: AskPromptProps; context: MenuContext }
  | { kind: "list"; context: MenuContext }
  | { kind: "related"; name: string; context: MenuContext };

const MIN_INT = -2147483648;
const MAX_INT = 2147483647;

export class TableCommands {
  #overlay?: TableOverlay;
  /** Token search installs this when its overlay is available. */
  openTokenSearch?: (choose: (card: TableCard) => void) => void;
  /** The shortcut editor can install this handler for Option+Return. */
  bindCommand?: (id: string) => void;

  constructor(
    readonly client: TableClient,
    readonly bindings: Bindings,
    readonly show: (overlay: TableOverlay | undefined) => void,
    private readonly report: (error: unknown) => void,
  ) {
    client.onSecondary((target, at, element) => {
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
      const point = element ? at : client.surface?.viewportPoint(at);
      if (point) this.open(target, point, element);
      return true;
    });
  }

  #set(overlay?: TableOverlay) {
    this.#overlay = overlay;
    this.show(overlay);
  }

  readonly close = () => this.#set();
  readonly reportError = (error: unknown) => this.report(error);

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
        const found = object?.name && cardRecords.find(object.name);
        return object && found ? [[object.slot, found] as const] : [];
      }),
    );
    return { target, view, cards, printed, key: (id) => this.bindings.get(id) };
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
    const overlay: TableOverlay = { kind: "menu", context, at };
    this.#set(overlay);
    const card = "name" in target ? target.object : undefined;
    const marker =
      !card && "name" in target && typeof target.name === "string"
        ? context.view.stack.find(
            (item) =>
              item.kind === "ability" && `a${item.manual?.id}` === target.name,
          )
        : undefined;
    const name =
      (card && context.printed?.get(card.slot)?.card.name) ??
      card?.name ??
      (marker?.kind === "ability" ? marker.name : undefined);
    if (name && card?.facing !== "down")
      void cardTokens(name)
        .then((tokens) => {
          if (
            this.#overlay !== overlay ||
            this.client.view !== context.view ||
            !tokens.length
          )
            return;
          this.#set({ ...overlay, context: { ...context, tokens } });
        })
        .catch(this.report);
  }

  openList(context: MenuContext) {
    this.#set({ kind: "list", context });
  }

  async run(entry: MenuCommand, context: MenuContext) {
    const { client } = this;
    if (context.view !== client.view) return;
    const { command, token } = entry;
    const { target } = context;
    const source = "name" in target ? target : undefined;
    const object = source?.object;
    const send = (values?: ActionValues) => {
      if (context.view !== client.view) return;
      const actions = commandActions(entry, context, values);
      const [first] = actions;
      if (first)
        void client
          .send(
            actions.length === 1 ? first : { batch: { actions } },
            context.element ?? source?.name,
          )
          .catch(this.report);
    };

    if (command.client === "open_command_list") {
      this.openList(context);
      return;
    }
    if (command.client === "view_related_cards" && token) {
      this.#set({ kind: "related", name: token.name, context });
      return;
    }
    if (
      command.client === "view_graveyard" ||
      command.client === "view_exile"
    ) {
      if (target.kind === "graveyard" || target.kind === "exile")
        target.open?.();
      return;
    }
    if (command.client) return;

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
      this.openTokenSearch?.((card) =>
        send({ card, text: null, number: 1, target: null }),
      );
      return;
    }

    const show = (prompt: AskPromptProps) =>
      this.#set({ kind: "ask", prompt, context });
    const confirm = (values: ActionValues) => {
      this.close();
      send(values);
    };
    const label = entry.label.replace(/…$/, "");
    if (command.asks === "text" || command.asks === "text_and_number") {
      show({
        kind: "text",
        start: command.id === "permanent.note" ? (object?.note ?? "") : "",
        fieldLabel: label,
        label: () => (command.id === "permanent.note" ? "Set note" : label),
        onCancel: this.close,
        onConfirm: (text) => {
          if (command.asks === "text_and_number") {
            show({
              kind: "number",
              start: 1,
              min: 1,
              max: 65535,
              label: (number) => `Add ${number} ${text}`,
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
      const counter = command.id.startsWith("counter.");
      const counterSlot = command.id.includes(".second_")
        ? 1
        : command.id.includes(".third_")
          ? 2
          : 0;
      const name =
        counterNames(object, object && context.printed?.get(object.slot))[
          counterSlot
        ] ?? "generic";
      const current = counter ? counterValue(object, name) : life;
      const count =
        target.kind === "library" ? pileSlots(context).length : undefined;
      const verb =
        command.id === "library.draw_cards"
          ? "Draw"
          : command.id === "library.move_top_cards_to_graveyard"
            ? "Mill"
            : label;
      show({
        kind: "number",
        start: current ?? 1,
        current,
        min: life === undefined ? (counter ? 0 : 1) : MIN_INT,
        max:
          life !== undefined
            ? MAX_INT
            : counter
              ? 999
              : Math.max(1, Math.min(65535, count ?? 65535)),
        fieldLabel:
          command.id === "library.draw_cards" ? "Cards to draw" : label,
        label: (number) => `${verb} ${number}`,
        onCancel: this.close,
        onConfirm: (number) => confirm({ number }),
      });
    } else if (command.asks === "stats") {
      const start = baseStats(object);
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
    return <AskPrompt {...overlay.prompt} key={overlay.prompt.kind} />;
  if (overlay.kind === "related")
    return <RelatedCard name={overlay.name} onClose={commands.close} />;
  const { header, groups, entries } = menuRows(kindOf(context.target), context);
  const choose = (entries: MenuCommand[], id: string) => {
    const entry = entries.find((entry) => entry.id === id);
    if (entry) void commands.run(entry, context).catch(commands.reportError);
  };
  if (overlay.kind === "list") {
    const list = availableCommands(kindOf(context.target), context).filter(
      ({ command }) =>
        ![
          "table.every_action",
          "library.stack_until",
          "library.keep_top_known",
        ].includes(command.id) &&
        (!command.client ||
          ["view_graveyard", "view_exile", "view_related_cards"].includes(
            command.client,
          )),
    );
    return (
      <CommandList
        acting={header.title}
        commands={list.map(({ id, label, command }) => ({
          id,
          label,
          group: command.group,
          key: context.key?.(command.id),
        }))}
        presentation={phone ? "fullscreen" : "panel"}
        onClose={commands.close}
        onRun={(id) => {
          commands.close();
          choose(list, id);
        }}
        onBind={(id) => commands.bindCommand?.(id)}
      />
    );
  }
  return (
    <Menu
      header={header}
      groups={groups}
      at={overlay.at}
      presentation={phone ? "sheet" : "menu"}
      onClose={commands.close}
      onChoose={(id) => choose(entries, id)}
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
