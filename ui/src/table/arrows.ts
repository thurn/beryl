import type {
  PlayerView,
  TableAction,
  TableObjectAction,
  TableObjectRef,
  TableSeenObject,
  TableTarget as ActionTarget,
} from "../generated/protocol";
import { tableCurrent } from "../ipc";
import type { Name, Point } from "../stage/layout";
import { dark, sought } from "../stage/light";
import { shapeOf, type Reach } from "../stage/lines";
import type { Under } from "../stage/pointer";
import { keyOf } from "../stage/spans";
import type { ScriptStage } from "../stage/stage";
import { Bindings } from "./bindings";
import type { CardTarget, TableClient, TableTarget } from "./client";
import { resolve } from "./shortcuts";

type Reference = TableSeenObject<TableObjectRef>;
type Destination = ActionTarget<TableObjectRef>;

function seen(action: TableObjectAction): Reference {
  return { id: action.reference, in: action.zone };
}

function destination(target: Under, view: PlayerView): Destination | undefined {
  const table = view.table;
  if (!table) return undefined;
  if ("player" in target) {
    const viewer = table.seat ?? 0;
    const player = target.player === "you" ? viewer : 1 - viewer;
    return player < table.seats ? { player } : undefined;
  }
  const entry = table.objectActions.find(
    ({ subject }) => subject.kind === "object" && subject.slot === target.name,
  );
  return entry && { object: seen(entry) };
}

function under(target: TableTarget | undefined): Under | undefined {
  if (target?.kind === "life") return { player: target.side };
  if (
    target &&
    "name" in target &&
    typeof target.name === "number" &&
    target.action
  )
    return { name: target.name };
  return undefined;
}

function actionList(actions: TableAction[]): TableAction {
  return actions.length === 1 ? actions[0]! : { batch: { actions } };
}

const inHand = ({ zone }: TableObjectAction) =>
  typeof zone === "object" && "hand" in zone;

function sources(client: TableClient, from: CardTarget): number[] {
  const selected = client.selection.has(from.name)
    ? client.selection
    : [from.name];
  return [...selected].filter(
    (name): name is number =>
      typeof name === "number" && !!client.card(name)?.action,
  );
}

function drawActions(
  view: PlayerView,
  from: readonly number[],
  to: Destination,
): TableAction[] {
  return from.flatMap((slot) => {
    const entry = view.table?.objectActions.find(
      ({ subject }) => subject.kind === "object" && subject.slot === slot,
    );
    return entry && !inHand(entry)
      ? [{ arrow: { objects: [entry.reference], in: entry.zone, to } }]
      : [];
  });
}

/** The table owns the aim shared by a right-drag and a menu command. */
export class TableArrows {
  #finishPick?: (target?: Under) => void;
  #dragging = false;
  #traces = 0;

  constructor(
    private readonly client: TableClient,
    private readonly stage: ScriptStage,
    report: (error: unknown) => void,
    bindings = new Bindings(/Mac/.test(navigator.platform) ? "macos" : "linux"),
  ) {
    client.onAim({
      drag: (from, at, over, done) => {
        if (this.#finishPick) return;
        if (done) this.#cancel();
        if (client.overlayOpen || typeof from.name !== "number") return;
        if (!done) {
          this.#dragging = true;
          this.#trace(sources(client, from), at);
          return;
        }
        const target = under(over);
        if (target) void this.draw(from, target).catch(report);
      },
      cancel: this.#cancel,
      picking: () => this.#finishPick !== undefined,
      pick: (target) => {
        if (!this.#finishPick) return false;
        const hit = under(target);
        const view = client.view;
        if (hit && view && destination(hit, view)) this.#finishPick(hit);
        return true;
      },
    });

    client.onSecondary((_, at, element) => {
      if (element || client.overlayOpen) return;
      const view = client.view;
      const surface = client.surface;
      if (!view || !surface) return;
      for (const arrow of [...(view.table?.arrows ?? [])].reverse()) {
        if (
          !arrow.own ||
          arrow.from.kind !== "object" ||
          arrow.to.kind === "ability"
        )
          continue;
        const name = arrow.from.slot;
        const from = client.card(name)?.action;
        const to = destination(
          arrow.to.kind === "player"
            ? { player: arrow.to.player }
            : { name: arrow.to.slot },
          view,
        );
        if (!from || !to) continue;
        const path = surface.linePath(
          keyOf({ ...arrow, verb: "point", span: "begin" }),
        );
        if (!path || !hits(path, at)) continue;
        void client
          .send({ clear_arrows: { from: seen(from), to } }, name)
          .catch(report);
        return true;
      }
    });

    client.onKey((event) => {
      if (event.key === "Escape" && this.#dragging) {
        event.preventDefault();
        client.cancelGesture();
      }
      if (
        !client.overlayOpen &&
        !(
          event.target instanceof Element &&
          event.target.closest("input, textarea, select, [contenteditable]")
        ) &&
        resolve(event, "table", bindings) === "table.clear_arrows"
      ) {
        event.preventDefault();
        if (!event.repeat)
          void client
            .send({ clear_arrows: { from: null, to: null } })
            .catch(report);
      }
    });
  }

  /** Start a menu's aim. The click is consumed before normal table actions run. */
  pickTarget(from: Name): Promise<Under | undefined> {
    this.#cancel();
    const surface = this.client.surface;
    if (
      typeof from !== "number" ||
      !surface ||
      !this.client.card(from, true)?.action
    )
      return Promise.resolve(undefined);
    return new Promise((resolve) => {
      const stop = surface.followPointer((at) => this.#trace([from], at));
      const close = this.client.pushOpen(() => this.#finishPick?.());
      this.#finishPick = (target) => {
        this.#finishPick = undefined;
        stop();
        close();
        this.#trace([]);
        resolve(target);
      };
    });
  }

  /** Submit the same action for a right drag or the Draw arrow menu row. */
  async draw(from: CardTarget, target: Under) {
    const { client } = this;
    const view = client.view;
    if (
      !view ||
      !from.action ||
      client.card(from.name, true)?.action !== from.action
    )
      return;
    let to = destination(target, view);
    if (!to) return;
    const cards = sources(client, from).filter(
      (name) => !("name" in target && name === target.name),
    );
    const moves: TableAction[] = cards.flatMap((name) => {
      const action = client.card(name)?.action;
      return action && inHand(action)
        ? [
            {
              move: {
                to: "play",
                objects: [action.reference],
                in: action.zone,
              },
            },
          ]
        : [];
    });
    let ready = view;
    if (moves.length) {
      // The host validates a batch against the original view. Wait for the next
      // view so the arrow can use the played cards' public zones and fresh references.
      const previous = view.table?.log.at(-1)?.n;
      const reply = await client.send(actionList(moves), from.name);
      if (!("ok" in reply) || previous === undefined) return;
      const next = (await tableCurrent())?.view;
      if (!client.surface || next?.table?.log.at(-1)?.n !== previous + 1)
        return;
      ready = next;
      to = destination(target, ready);
      if (!to) return;
    }
    const arrows = drawActions(ready, cards, to);
    if (arrows.length) await client.send(actionList(arrows), from.name);
  }

  readonly #cancel = () => {
    this.#dragging = false;
    if (this.#finishPick) this.#finishPick();
    else this.#trace([]);
  };

  #trace(names: readonly number[], at?: Point) {
    const surface = this.client.surface;
    if (!surface) return;
    const from = at
      ? names.flatMap((name) => this.stage.reach(name) ?? [])
      : [];
    for (let index = from.length; index < this.#traces; index++)
      surface.line("table-aim " + index);
    const previous = this.#traces;
    this.#traces = from.length;
    if (!at || !from.length) {
      if (previous) surface.aim();
      return;
    }
    // Keep a path for browsers without WebGL, just as the stage does for a committed arrow.
    const tip: Reach = {
      middle: at,
      half: { x: 1, y: 1 },
      round: true,
      toward: 0,
      ahead: 0,
    };
    for (const [index, reach] of from.entries()) {
      const inside =
        Math.abs(at.x - reach.middle.x) <= reach.half.x &&
        Math.abs(at.y - reach.middle.y) <= reach.half.y;
      surface.line(
        "table-aim " + index,
        "hostile-target",
        inside ? null : shapeOf("hostile-target", reach, tip, 1),
      );
    }
    surface.aim((now) => {
      const over = dark(now);
      for (const reach of from)
        over.ribbons.push(...sought(now, reach, at).ribbons);
      return { under: dark(now), over };
    });
  }
}

const HIT_RADIUS = 9;

function hits(path: SVGPathElement, at: Point): boolean {
  if (!path.getAttribute("d")) return false;
  const length = path.getTotalLength();
  // Sample the same path drawn by the stage, including the bow and the arrowhead.
  const segments = Math.max(1, Math.ceil(length / (HIT_RADIUS / 2)));
  for (let step = 0; step <= segments; step++) {
    const point = path.getPointAtLength((step * length) / segments);
    if (Math.hypot(point.x - at.x, point.y - at.y) <= HIT_RADIUS) return true;
  }
  return false;
}
