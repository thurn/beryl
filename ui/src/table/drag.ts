import type { Box } from "../game/placement";
import { named } from "../stage/dom";
import type { Name, Point } from "../stage/layout";
import type { Drop, PointerKeys } from "../stage/pointer";
import type { ScriptStage } from "../stage/stage";
import type { TableClient } from "./client";
import { available, dropAction, type DragCard } from "./dropActions";
import {
  destinationAt,
  pileZone,
  type Destination,
  type DropArea,
} from "./dropZones";

interface Order {
  cards: DragCard[];
  drop: Drop;
  to: Destination;
}

interface Preview {
  cards: DragCard[];
  at: Point;
  width: number;
  external: boolean;
  returning?: { boxes: ReadonlyMap<number, Box>; duration: number };
  target?: Destination;
}

export interface DragDisplay {
  preview?: Preview;
  order?: Order;
}

export class TableDrag {
  areas: readonly DropArea[] = [];
  #display: DragDisplay = {};
  #source?: { name: Name; element: HTMLElement; box: Box };
  #cards: DragCard[] = [];
  #origins = new Map<number, Box>();
  #pending?: object;
  readonly #listeners = new Set<() => void>();

  constructor(
    private readonly client: TableClient,
    private readonly stage: ScriptStage,
    private readonly report: (error: unknown) => void,
  ) {
    client.onDrop(this.#drop, {
      dragSource: this.#dragSource,
      canHold: () => !this.#pending && !this.#display.order,
      lift: (name) => {
        if (name === undefined) {
          // Pointer emits the lift ending just before it delivers a drop.
          queueMicrotask(() => {
            if (!this.#pending && !this.#display.order) this.#return();
          });
        } else {
          this.#cards = this.#selected(name);
          const surface = this.client.surface;
          this.#origins = new Map(
            this.#cards.flatMap(({ object: { slot } }) => {
              if (this.#source?.name === slot)
                return [[slot, this.#source.box]];
              const node = surface?.node(slot);
              return node && slot !== name
                ? [[slot, surface!.bounds(node)]]
                : [];
            }),
          );
        }
      },
      over: (_under, at) => this.#hover(at),
      viewChanged: () => this.#reset(),
    });
  }

  /** Give the cards back to the stage as soon as an accepted update starts playing. */
  arriving() {
    if (this.#pending && this.#display.preview) this.#show({});
  }

  readonly snapshot = () => this.#display;
  readonly subscribe = (listener: () => void) => {
    this.#listeners.add(listener);
    return () => this.#listeners.delete(listener);
  };

  #show(display: DragDisplay) {
    this.#display = display;
    for (const listener of this.#listeners) listener();
  }

  #reset() {
    this.#pending = undefined;
    this.cancelOrder();
    this.#clear();
  }

  #clear() {
    if (this.#display.preview) this.#show({});
    this.#cards = [];
  }

  #return() {
    const preview = this.#display.preview;
    const duration = this.stage.release.duration;
    if (!preview || !duration || !this.#origins.size) {
      this.#clear();
      return;
    }
    this.#show({
      preview: {
        ...preview,
        target: undefined,
        returning: { boxes: this.#origins, duration },
      },
    });
  }

  finishReturn(preview: Preview) {
    if (preview === this.#display.preview) this.#clear();
  }

  #dragSource = (element: Element, { alt }: PointerKeys, at: Point) => {
    this.#source = undefined;
    const { view, surface } = this.client;
    if (!view || !surface || this.#pending || this.#display.order) return;
    const viewerCard = element.closest<HTMLElement>("[data-viewer-slot]");
    const pile = element.closest<HTMLButtonElement>(
      "button[data-pile][data-side]",
    );
    const side = pile?.dataset.side;
    const zone = pileZone(pile?.dataset.pile);
    let name: Name | undefined;
    if (viewerCard) name = Number(viewerCard.dataset.viewerSlot);
    else if (zone && (side === "you" || side === "opponent")) {
      const ids =
        zone === "library" || zone === "graveyard"
          ? view.players[side][zone]
          : view.table?.orders[side][zone];
      name = alt ? ids?.[0] : ids?.at(-1);
    }
    let control = viewerCard ?? pile;
    // The scrim still closes on an ordinary click. A drag can pick up a card
    // visible behind it and carry it into the open viewer.
    if (!control && element.matches(".viewer-scrim, .phone-scrim")) {
      const node = surface
        .hits(at)
        .find((hit) => hit.closest("[data-stage]"))
        ?.closest<HTMLElement>("[data-stage]");
      if (node?.dataset.stage !== undefined) {
        name = named(node.dataset.stage);
        control = node;
      }
    }
    if (!control || name === undefined || !this.client.card(name, true)?.action)
      return;
    this.#source = { name, element: control, box: surface.bounds(control) };
    const { left, top, right } = this.#source.box;
    return {
      name,
      capture:
        element instanceof HTMLElement &&
        element.matches(".viewer-scrim, .phone-scrim")
          ? element
          : control,
      origin: {
        x: left,
        y: top,
        width: right - left,
        rotation: 0,
        skew: 0,
        opacity: 1,
      },
    };
  };

  #selected(name: Name): DragCard[] {
    const names = this.client.selection.has(name)
      ? [
          name,
          ...[...this.client.selection].filter((selected) => selected !== name),
        ]
      : [name];
    return names.flatMap((slot) => {
      const card = this.client.card(slot, true);
      return card?.object && card.action
        ? [{ object: card.object, action: card.action }]
        : [];
    });
  }

  #destination(at: Point) {
    const names = this.#cards.map(({ object }) => object.slot);
    const to = destinationAt(this.client, this.areas, at, names);
    return to && available(this.client, to, this.#cards) ? to : undefined;
  }

  #hover(at: Point) {
    const card = this.#cards[0]?.object;
    if (!card) return;
    const pose = this.stage.pose(card.slot);
    if (!pose) return;
    this.#show({
      preview: {
        cards: this.#cards,
        width: pose.width,
        at: pose,
        external: this.#source?.name === card.slot,
        target: this.#destination(at),
      },
    });
  }

  #drop = (drop: Drop) => {
    const cards = this.#cards;
    if (!cards.length) return false;
    const to = this.#destination(drop.at);
    if (!to) return false;
    const pending = { cards, drop, to };
    if (to.kind === "library" && cards.length > 1) {
      this.#show({ order: pending });
      return true;
    }
    const action = dropAction(this.client, cards, to, drop.at, drop.keys.shift);
    if (!action) return false;
    this.#send(action, drop.name);
    return true;
  };

  readonly cancelOrder = () => {
    const order = this.#display.order;
    if (order) {
      this.client.rejectDrop(order.drop.name);
      this.#show({});
    }
  };

  confirmOrder(cards: DragCard[]) {
    const order = this.#display.order;
    if (!order) return;
    const { to, drop } = order;
    this.#show({});
    const action = dropAction(this.client, cards, to, drop.at, drop.keys.shift);
    if (action) this.#send(action, drop.name);
    else this.client.rejectDrop(drop.name);
  }

  #send(action: NonNullable<ReturnType<typeof dropAction>>, name: Name) {
    const pending = {};
    this.#pending = pending;
    const source = this.#source?.name === name ? this.#source.element : name;
    const preview = this.#display.preview;
    if (preview) this.#show({ preview: { ...preview, target: undefined } });
    const decline = () => {
      if (this.#pending !== pending) return;
      this.#pending = undefined;
      this.#return();
      this.client.rejectDrop(name);
    };
    void this.client.send(action, source).then(
      (reply) => {
        if ("refused" in reply) decline();
      },
      (failure: unknown) => {
        decline();
        this.report(failure);
      },
    );
  }
}
