import type {
  PlayerView,
  Side,
  TableAction,
  TablePortion,
} from "../generated/protocol";
import type { TableClient } from "./client";
import type { TableCommands } from "./TableMenu";

type Look = Extract<TableAction, { look: unknown }>;
export type PileView =
  | { zone: "library"; side: Side; portion: TablePortion; slots?: Set<number> }
  | { zone: "graveyard" | "exile" | "sideboard"; side: Side }
  | { zone: "linked"; host: number }
  | { zone: "revealed" };

export function portionSlots(slots: number[], portion: TablePortion) {
  if (portion === "all") return slots;
  return "top" in portion
    ? slots.slice(Math.max(0, slots.length - portion.top))
    : slots.slice(0, portion.bottom);
}

export type OpenPile = PileView & { key: number };

interface ViewerControls {
  close(): void;
  search(): void;
  group(): void;
  sort(by: "name" | "mana"): void;
}

export class TableViewers {
  #open: OpenPile[] = [];
  #nextKey = 0;
  #pending = false;
  #listeners = new Set<() => void>();
  #controls?: ViewerControls;

  constructor(
    private readonly client: TableClient,
    readonly reportError: (error: unknown) => void,
  ) {}

  readonly snapshot = () => this.#open.at(-1);
  readonly subscribe = (listener: () => void) => {
    this.#listeners.add(listener);
    return () => this.#listeners.delete(listener);
  };

  #show(open: OpenPile[]) {
    this.#open = open;
    for (const listener of this.#listeners) listener();
  }

  open(pile: PileView) {
    const same = (open: PileView) =>
      open.zone === pile.zone &&
      ("host" in open
        ? "host" in pile && open.host === pile.host
        : "side" in open
          ? "side" in pile && open.side === pile.side
          : true);
    this.#show([
      ...this.#open.filter((open) => !same(open)),
      { ...pile, key: this.#nextKey++ },
    ]);
  }

  async look(action: Look, view: PlayerView, source?: HTMLElement) {
    if (this.#pending) return;
    this.#pending = true;
    try {
      const { seat, portion } = action.look;
      const side = seat === (view.table?.seat ?? 0) ? "you" : "opponent";
      const slots =
        portion === "all"
          ? undefined
          : new Set(portionSlots(view.players[side].library, portion));
      const result = await this.client.send(action, source);
      if ("refused" in result) return;
      this.#open = this.#open.filter(({ zone }) => zone !== "library");
      this.open({ zone: "library", side, portion, slots });
    } finally {
      this.#pending = false;
    }
  }

  async close(pile: OpenPile, shuffle = false) {
    if (!this.#open.includes(pile)) return;
    if (pile.zone === "library") {
      if (this.#pending) return;
      this.#pending = true;
      try {
        const close: TableAction = { close_look: {} };
        const local = this.client.view?.table?.seat ?? 0;
        const seat = pile.side === "you" ? local : 1 - local;
        const result = await this.client.send(
          shuffle
            ? {
                batch: {
                  actions: [{ shuffle: { seat, portion: "all" } }, close],
                },
              }
            : close,
        );
        if ("refused" in result) return;
      } finally {
        this.#pending = false;
      }
    }
    this.#show(this.#open.filter((open) => open !== pile));
  }

  async #finishLibrary(shuffle: boolean) {
    const pile = this.snapshot();
    if (pile?.zone === "library" && pile.portion === "all")
      await this.close(pile, shuffle);
  }

  controls(controls: ViewerControls) {
    this.#controls = controls;
    return () => {
      if (this.#controls === controls) this.#controls = undefined;
    };
  }

  hideRevealed() {
    this.#show(this.#open.filter(({ zone }) => zone !== "revealed"));
  }

  register(commands: TableCommands) {
    for (const [id, zone] of [
      ["view_graveyard", "graveyard"],
      ["view_exile", "exile"],
      ["view_sideboard", "sideboard"],
    ] as const) {
      commands.clients.set(id, ({ target }) => {
        if ("side" in target) this.open({ zone, side: target.side });
      });
    }
    for (const [id, shuffle] of [
      ["close_and_shuffle", true],
      ["close_without_shuffle", false],
    ] as const) {
      commands.clients.set(id, () => {
        void this.#finishLibrary(shuffle).catch(this.reportError);
      });
    }
    commands.clients.set("hide_revealed_cards", () => this.hideRevealed());
    commands.clients.set("close_viewer", () => this.#controls?.close());
    commands.clients.set("search_viewer", () => this.#controls?.search());
    commands.clients.set("group_viewer_by_type", () => this.#controls?.group());
    commands.clients.set("sort_viewer_by_name", () =>
      this.#controls?.sort("name"),
    );
    commands.clients.set("sort_viewer_by_mana_value", () =>
      this.#controls?.sort("mana"),
    );
  }
}
