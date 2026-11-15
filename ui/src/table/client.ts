import type { Box } from "../game/placement";
import type { Step } from "../generated/protocol";
import {
  tableAct,
  type ObjectView,
  type PlayerView,
  type Side,
  type TableActReply,
  type TableAction,
  type TableObjectAction,
} from "../ipc";
import type { DomSurface } from "../stage/dom";
import type { Name, Point } from "../stage/layout";
import type {
  Drop,
  Pointer,
  PointerClient,
  PointerKeys,
  Under,
} from "../stage/pointer";
import { shakeRefusal } from "../stage/Refusal";
import type { TableArrows } from "./arrows";
import type { TableDrag } from "./drag";
import type { TableViewers } from "./viewers";

type ObjectKind =
  "permanent" | "their_permanent" | "hand_card" | "stack_entry" | "viewer_card";
type PileKind = "library" | "graveyard" | "exile" | "hand";

export interface CardTarget {
  kind: ObjectKind;
  name: Name;
  object?: ObjectView;
  action?: TableObjectAction;
}

interface PileTarget {
  kind: PileKind;
  side: Side;
  open?: () => void;
}

export type TableTarget =
  | CardTarget
  | PileTarget
  | { kind: "life" | "player" | "sideboard"; side: Side }
  | { kind: "table" }
  | { kind: "counter"; side: Side; counter: string }
  | { kind: "step"; side: Side; step: Step };

type TargetOf<K> = TableTarget extends infer Target
  ? Target extends TableTarget
    ? K extends Target["kind"]
      ? Target & { kind: K }
      : never
    : never
  : never;
type Click = (
  target: TableTarget,
  keys: PointerKeys,
  element?: HTMLElement,
) => void;
type Secondary = (
  target: TableTarget | undefined,
  at: Point,
  element?: HTMLElement,
) => boolean | void;
type Aim = (
  from: CardTarget,
  at: Point,
  over: TableTarget | undefined,
  done: boolean,
) => void;
interface AimRegistration {
  drag: Aim;
  cancel: () => void;
  pick: (target: TableTarget) => boolean;
  picking: () => boolean;
}
type Sweep = (from: Point, to: Point, done: boolean) => void;
type DragRegistration = Pick<PointerClient, "dragSource" | "over" | "lift"> & {
  canHold?(): boolean;
  viewChanged?(): void;
};
type DropHandler = (
  drop: Drop,
  from: CardTarget,
  onto: TableTarget | undefined,
) => boolean | void;

// Table zones with a seat are single-field tagged objects from the same protocol.
export const sameZone = (
  a: TableObjectAction["zone"],
  b: TableObjectAction["zone"],
) => a === b || JSON.stringify(a) === JSON.stringify(b);

export class TableClient {
  #frozen = false;
  #selection: ReadonlySet<Name> = new Set();
  #selectionBox?: Box;
  readonly #selectionListeners = new Set<() => void>();
  readonly #open: Array<{ close: () => void }> = [];
  #cardClick?: (target: CardTarget, keys: PointerKeys) => boolean;
  view?: PlayerView;
  drag?: TableDrag;
  viewers?: TableViewers;
  arrows?: TableArrows;
  #surface?: DomSurface;
  readonly #clicks = new Map<TableTarget["kind"], Click>();
  readonly #secondaries = new Set<Secondary>();
  #drop?: DropHandler;
  #drag?: DragRegistration;
  #aim?: AimRegistration;
  #sweep?: Sweep;
  #cancelSweep?: () => void;
  readonly #keys = new Set<(event: KeyboardEvent) => void>();
  readonly #actions = new Map<Name, TableObjectAction>();
  readonly #objects = new Map<number, ObjectView>();

  constructor(private readonly pointer: Pointer) {}

  get frozen() {
    return this.#frozen;
  }

  setFrozen(frozen: boolean) {
    if (this.#frozen === frozen) return;
    this.#frozen = frozen;
    if (frozen) {
      this.pointer.cancel();
      this.#drag?.viewChanged?.();
      this.#aim?.cancel();
    }
  }

  setView(view: PlayerView) {
    if (view === this.view) return;
    const selected = [...this.#selection].map(
      (name) => [name, this.#actions.get(name)] as const,
    );
    this.view = view;
    this.#actions.clear();
    this.#objects.clear();
    for (const object of view.objects) this.#objects.set(object.slot, object);
    for (const action of view.table?.objectActions ?? []) {
      const subject = action.subject;
      if (subject.kind === "object") this.#actions.set(subject.slot, action);
      else if (subject.kind === "ability")
        this.#actions.set(`a${subject.id}`, action);
    }
    // The adapter reserves the old view's slots while assigning new ones. A surviving
    // slot still identifies the same card, but a move can keep the slot and change its zone.
    if (selected.length)
      this.setSelection(
        selected.flatMap(([name, previous]) => {
          const current = this.#actions.get(name);
          return previous && current && sameZone(previous.zone, current.zone)
            ? [name]
            : [];
        }),
      );
    // References expire with the view. A held press cannot acquire new ones on release.
    this.pointer.cancel();
    this.#drag?.viewChanged?.();
    this.#aim?.cancel();
  }

  get surface() {
    return this.#surface;
  }

  get selection() {
    return this.#selection;
  }

  readonly selectionSnapshot = () => this.#selection;
  readonly sweepSnapshot = () => this.#selectionBox;
  readonly subscribeSelection = (listener: () => void) => {
    this.#selectionListeners.add(listener);
    return () => this.#selectionListeners.delete(listener);
  };

  setSelection(names: Iterable<Name>) {
    const selection = new Set(names);
    if (
      selection.size === this.#selection.size &&
      [...selection].every((name) => this.#selection.has(name))
    )
      return;
    this.#selection = selection;
    this.#announceSelection();
  }

  setSelectionBox(box?: Box) {
    this.#selectionBox = box;
    this.#announceSelection();
  }

  #announceSelection() {
    for (const listener of this.#selectionListeners) listener();
  }

  /** Close callbacks run in reverse opening order. Dispose on unmount or ordinary close. */
  readonly pushOpen = (close: () => void) => {
    const entry = { close };
    this.#open.push(entry);
    return () => {
      const index = this.#open.indexOf(entry);
      if (index !== -1) this.#open.splice(index, 1);
    };
  };

  closeTop() {
    this.#open.at(-1)?.close();
  }

  get overlayOpen() {
    return this.#open.length !== 0;
  }

  get holdingCard() {
    return this.pointer.held !== undefined;
  }

  onCardClick(handler: (target: CardTarget, keys: PointerKeys) => boolean) {
    this.#cardClick = handler;
  }

  readonly setSurface = (surface: DomSurface | undefined) => {
    if (!surface) this.#aim?.cancel();
    this.#surface = surface;
  };

  onClick<K extends TableTarget["kind"]>(
    kind: K,
    handler: (
      target: TargetOf<K>,
      keys: PointerKeys,
      element?: HTMLElement,
    ) => void,
  ) {
    this.#clicks.set(kind, (target, keys, element) => {
      // The map dispatches only the kind this handler registered for.
      handler(target as TargetOf<K>, keys, element);
    });
  }

  onSecondary(handler: Secondary) {
    this.#secondaries.add(handler);
  }

  onDrop(handler: DropHandler, drag: DragRegistration) {
    this.#drop = handler;
    this.#drag = drag;
  }

  rejectDrop(name: Name) {
    this.pointer.rejectDrop(name);
  }

  onAim(handler: AimRegistration) {
    this.#aim = handler;
  }

  onSweep(handler: Sweep, cancel?: () => void) {
    this.#sweep = handler;
    this.#cancelSweep = cancel;
  }

  onKey(handler: (event: KeyboardEvent) => void) {
    this.#keys.add(handler);
  }

  readonly key = (event: KeyboardEvent) => {
    if (this.frozen || event.defaultPrevented || event.isComposing) return;
    if (event.key === "Escape" && this.overlayOpen) {
      event.preventDefault();
      this.closeTop();
      return;
    }
    for (const handler of this.#keys) {
      if (event.defaultPrevented) break;
      handler(event);
    }
  };

  card(name: Name, viewer = false): CardTarget | undefined {
    const action = this.#actions.get(name);
    const object =
      typeof name === "number" ? this.#objects.get(name) : undefined;
    if (viewer && object) return { kind: "viewer_card", name, action, object };
    if (action?.zone === "stack" || object?.zone === "stack")
      return { kind: "stack_entry", name, action, object };
    if (object?.zone === "battlefield")
      return {
        kind: object.controller === "you" ? "permanent" : "their_permanent",
        name,
        action,
        object,
      };
    if (object?.zone === "hand")
      return { kind: "hand_card", name, action, object };
    return undefined;
  }

  #under(under: Under | Name | undefined): TableTarget | undefined {
    if (under === undefined) return undefined;
    if (typeof under === "object" && "player" in under)
      return { kind: "life", side: under.player };
    const card = this.card(typeof under === "object" ? under.name : under);
    if (
      card?.kind === "hand_card" &&
      !card.action &&
      card.object &&
      (card.object.zoneSide ?? card.object.owner) === "opponent"
    )
      return { kind: "hand", side: "opponent" };
    return card;
  }

  hovered() {
    return this.#under(this.pointer.hovered);
  }

  hasClick(kind: TableTarget["kind"]): boolean {
    return this.#clicks.has(kind);
  }

  click(
    target: TableTarget,
    keys: PointerKeys,
    element?: HTMLElement,
  ): boolean {
    if (this.frozen) return true;
    if (this.#aim?.pick(target)) return true;
    if ("name" in target && this.#cardClick?.(target, keys)) return true;
    const handler = this.#clicks.get(target.kind);
    handler?.(target, keys, element);
    return handler !== undefined;
  }

  secondary(target: TableTarget | undefined, at: Point, element?: HTMLElement) {
    if (this.frozen || this.#aim?.picking()) return;
    for (const handler of this.#secondaries) {
      if (handler(target, at, element)) break;
    }
  }

  cancelGesture() {
    this.pointer.cancel();
  }

  readonly gestures: PointerClient = {
    dragSource: (element, keys, at) =>
      this.frozen ? undefined : this.#drag?.dragSource?.(element, keys, at),
    holdable: (name) =>
      !this.frozen &&
      !this.#aim?.picking() &&
      !!this.#drop &&
      (this.#drag?.canHold?.() ?? true) &&
      !!this.card(name, true)?.action &&
      typeof name === "number",
    over: (target, at) => this.#drag?.over?.(target, at),
    lift: (name) => this.#drag?.lift?.(name),
    click: (name, _at, keys, unhandled) => {
      const target = this.card(name);
      if (!target || !this.click(target, keys)) unhandled();
    },
    playerClick: (side, _at, keys) => this.click({ kind: "life", side }, keys),
    secondary: (under, at) => this.secondary(this.#under(under), at),
    drop: (drop) => {
      if (this.frozen) return;
      const from = this.card(drop.name, true);
      return from && this.#drop?.(drop, from, this.#under(drop.onto));
    },
    aim: (name, at, over, done) => {
      if (this.frozen) return;
      const from = this.card(name);
      if (from) this.#aim?.drag(from, at, this.#under(over), done);
    },
    cancelAim: () => this.#aim?.cancel(),
    sweep: (from, to, done) => {
      if (!this.frozen && !this.#aim?.picking()) this.#sweep?.(from, to, done);
    },
    cancelSweep: () => this.#cancelSweep?.(),
  };

  async send(
    action: TableAction,
    source?: Name | HTMLElement,
  ): Promise<TableActReply> {
    if (this.frozen) return { refused: "stale" };
    const view = this.view;
    const reply = await tableAct(action);
    if (
      !this.frozen &&
      "refused" in reply &&
      view === this.view &&
      this.#surface
    ) {
      const element =
        typeof source === "number" || typeof source === "string"
          ? this.#surface.node(source)
          : source;
      shakeRefusal(element);
    }
    return reply;
  }
}
