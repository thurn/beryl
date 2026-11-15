import type { Step } from "../generated/protocol";
import {
  tableAct,
  type ObjectView,
  type PlayerView,
  type Side,
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

type ObjectKind =
  "permanent" | "their_permanent" | "hand_card" | "stack_entry" | "viewer_card";
type PileKind = "library" | "graveyard" | "exile";

interface CardTarget {
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
  | { kind: "life"; side: Side }
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
type Sweep = (from: Point, to: Point, done: boolean) => void;
type DropHandler = (
  drop: Drop,
  from: CardTarget,
  onto: TableTarget | undefined,
) => boolean | void;

export class TableClient {
  readonly selection = new Set<Name>();
  view?: PlayerView;
  #surface?: DomSurface;
  readonly #clicks = new Map<TableTarget["kind"], Click>();
  readonly #secondaries = new Set<Secondary>();
  #drop?: DropHandler;
  #aim?: Aim;
  #sweep?: Sweep;
  readonly #keys = new Set<(event: KeyboardEvent) => void>();
  readonly #actions = new Map<Name, TableObjectAction>();
  readonly #objects = new Map<number, ObjectView>();

  constructor(private readonly pointer: Pointer) {}

  setView(view: PlayerView) {
    if (view === this.view) return;
    // A view's references expire together. A slot can also have a different occupant in
    // the next view, so a held press or selection cannot carry its slot into that view.
    this.view = view;
    this.selection.clear();
    this.#actions.clear();
    this.#objects.clear();
    for (const object of view.objects) this.#objects.set(object.slot, object);
    for (const action of view.table?.objectActions ?? []) {
      const subject = action.subject;
      if (subject.kind === "object") this.#actions.set(subject.slot, action);
      else if (subject.kind === "ability")
        this.#actions.set(`a${subject.id}`, action);
    }
    this.pointer.cancel();
  }

  readonly setSurface = (surface: DomSurface | undefined) => {
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

  onDrop(handler: DropHandler) {
    this.#drop = handler;
  }

  onAim(handler: Aim) {
    this.#aim = handler;
  }

  onSweep(handler: Sweep) {
    this.#sweep = handler;
  }

  onKey(handler: (event: KeyboardEvent) => void) {
    this.#keys.add(handler);
  }

  readonly key = (event: KeyboardEvent) => {
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
    if (typeof under !== "object") return this.card(under);
    return "player" in under
      ? { kind: "life", side: under.player }
      : this.card(under.name);
  }

  hasClick(kind: TableTarget["kind"]): boolean {
    return this.#clicks.has(kind);
  }

  click(
    target: TableTarget,
    keys: PointerKeys,
    element?: HTMLElement,
  ): boolean {
    const handler = this.#clicks.get(target.kind);
    handler?.(target, keys, element);
    return handler !== undefined;
  }

  secondary(target: TableTarget | undefined, at: Point, element?: HTMLElement) {
    for (const handler of this.#secondaries) {
      if (handler(target, at, element)) break;
    }
  }

  readonly gestures: PointerClient = {
    holdable: (name) => this.#drop !== undefined && !!this.card(name)?.action,
    click: (name, _at, keys, unhandled) => {
      const target = this.card(name);
      if (!target || !this.click(target, keys)) unhandled();
    },
    playerClick: (side, _at, keys) => this.click({ kind: "life", side }, keys),
    secondary: (under, at) => this.secondary(this.#under(under), at),
    drop: (drop) => {
      const from = this.card(drop.name);
      return from && this.#drop?.(drop, from, this.#under(drop.onto));
    },
    aim: (name, at, over, done) => {
      const from = this.card(name);
      if (from) this.#aim?.(from, at, this.#under(over), done);
    },
    sweep: (from, to, done) => this.#sweep?.(from, to, done),
  };

  async send(action: TableAction, source?: Name | HTMLElement) {
    const view = this.view;
    const reply = await tableAct(action);
    if ("refused" in reply && view === this.view && this.#surface) {
      const element =
        typeof source === "number" || typeof source === "string"
          ? this.#surface.node(source)
          : source;
      shakeRefusal(element);
    }
    return reply;
  }
}
