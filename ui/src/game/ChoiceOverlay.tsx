// A choose-or-order decision over the dimmed table, in the overlay of docs/ui_mockups/scry.png
// (decision D10): the cards in a tray, picked by clicking or ordered by dragging (each drag has
// a click fallback), the source card at the right, and "View Battlefield" to peek under it.
// A question of no cards is the floating prompt instead (game/FloatPrompt.tsx), over the
// undimmed table. `onPlan` gets the picks that answer the choice (game/choices.ts).
import {
  createContext,
  useContext,
  useEffect,
  useRef,
  useState,
  type PointerEvent,
  type ReactNode,
} from "react";
import {
  animate,
  motion,
  useMotionValue,
  useReducedMotion,
} from "motion/react";
import {
  CARD_ASPECT,
  Card,
  CardBack,
  type Highlight,
} from "../components/Card";
import { Library } from "../components/Player";
import { Button } from "../components/Controls";
import { useOpenSound } from "../audio/SoundControls";
import { Overlay, type OverlayAction } from "../components/Overlay";
import { RulesText } from "../components/Mana";
import type { ObjectView, PlayerView } from "../generated/protocol";
import { StageScale, useStageWidth } from "./Stage";
import { STAGE, STAGE_WIDTHS } from "./layout";
import {
  floats,
  type Arrange,
  type Ask,
  type Choice,
  type Pick,
  type Source,
  type Stack,
} from "./choices";
import type { Intent } from "./decisions";
import { useFace } from "./faces";
import { FloatPrompt, PromptRow } from "./FloatPrompt";
import { INSPECT_PAD, useHoverInspect, type HoverHandlers } from "./Piles";

export interface ChoiceOverlayProps {
  view: PlayerView;
  choice: Choice;
  /** An answer is on its way: the choice shows but takes no input. */
  busy: boolean;
  onPlan(plan: Intent[]): void;
  onViewBattlefield(): void;
  /** Takes back the cast the choice is part of, while it is unpaid (an evoke's exile): a Cancel
   * button, Escape or a right-click. */
  onCancel?(): void;
}

export function ChoiceOverlay(props: ChoiceOverlayProps) {
  useOpenSound();
  // The source card's column moves with the stage's width.
  useStageWidth();
  const { choice } = props;
  if (floats(choice)) return <AskFloat {...props} choice={choice} />;
  // Its cards are small: hovering one shows it whole, over the source card's column, clear of
  // the title, the choice and its buttons.
  const [inspector, inspect] = useHoverInspect(side());
  return (
    <Inspecting.Provider value={inspect}>
      {choice.kind === "arrange" ? (
        <ArrangeChoice {...props} choice={choice} />
      ) : choice.kind === "stack" ? (
        <StackChoice {...props} choice={choice} />
      ) : choice.kind === "pick" ? (
        <PickChoice {...props} choice={choice} />
      ) : (
        <AskChoice {...props} choice={choice} />
      )}
      {inspector}
    </Inspecting.Provider>
  );
}

/** The handlers that inspect an overlay's card while it is hovered. */
const Inspecting = createContext<
  ((object: ObjectView | undefined) => HoverHandlers) | undefined
>(undefined);

/** The inspector over the source card, its margin around the card's edges (components.css
 * .overlay: a 338 px side column right of the 1000 px tray, which starts at x 136 on the base
 * stage, held to its middle, and lies `column-gap` from it; the card from y 205 under View
 * Battlefield). */
const side = () => {
  const shift = STAGE.width - STAGE_WIDTHS.base;
  const gap = Math.min(72, Math.max(16, 72 + 0.35 * shift));
  return {
    left: 136 + shift / 2 + 1000 + gap - INSPECT_PAD,
    top: 205 - INSPECT_PAD,
    width: 338 + 2 * INSPECT_PAD,
  };
};

/** The tray's size (the overlay's choice column) and the cards' in it, stage pixels. */
const TRAY = { width: 1000, height: 468 };
const CARD_WIDTH = 220;
const SPRING = { type: "spring", stiffness: 260, damping: 30 } as const;

/** The shell around a tray: title, source card, buttons. */
function Shell({
  view,
  choice,
  actions,
  busy,
  onViewBattlefield,
  onCancel,
  children,
}: {
  view: PlayerView;
  choice: Choice;
  actions: OverlayAction[];
  busy: boolean;
  onViewBattlefield(): void;
  onCancel?(): void;
  children?: ReactNode;
}) {
  const source = choice.source;
  useEffect(() => {
    if (!onCancel || busy) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape" && !e.defaultPrevented) onCancel();
    };
    const onMenu = (e: MouseEvent) => {
      e.preventDefault();
      onCancel();
    };
    window.addEventListener("keydown", onKey);
    window.addEventListener("contextmenu", onMenu);
    return () => {
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("contextmenu", onMenu);
    };
  }, [onCancel, busy]);
  return (
    <Overlay
      className={`choice choice-${choice.kind}`}
      title={<RulesText text={choice.title} />}
      aside={source && <SourceCard view={view} source={source} />}
      actions={
        onCancel
          ? [...actions, { label: "Cancel", disabled: busy, onClick: onCancel }]
          : actions
      }
      onViewBattlefield={onViewBattlefield}
    >
      {children}
    </Overlay>
  );
}

function SourceCard({ view, source }: { view: PlayerView; source: Source }) {
  const object = view.objects.find((o) => o.slot === source.slot);
  return <ShownCard object={object} name={source.name} width={338} />;
}

/** A card in the overlay: its full frame, or its back when the viewer doesn't know it. */
function ShownCard({
  object,
  name,
  width,
  highlight,
}: {
  object?: ObjectView;
  name?: string;
  width: number;
  highlight?: Highlight;
}) {
  const shown = object?.name ?? name;
  const known = object ?? { ...NOWHERE, name: shown ?? null };
  const face = useFace(known);
  const inspect = useContext(Inspecting);
  if (!shown) return <CardBack width={width} />;
  return (
    <span className="shown-card" {...inspect?.(known)}>
      <Card card={face} width={width} highlight={highlight} />
    </span>
  );
}

/** A card known only by name, for frameFace. */
const NOWHERE: ObjectView = {
  slot: 0,
  zone: "nowhere",
  owner: "you",
  controller: "you",
  name: null,
  facing: "up",
  token: false,
  tapped: false,
  summoningSick: false,
  damage: 0,
  counters: [],
  types: [],
  keywords: [],
  pt: null,
  attachedTo: null,
  exiledBy: null,
  exiledWith: null,
  combat: null,
};

/** Centers for `count` cards in a row across the tray, at most `step` apart. */
function row(count: number, width: number, step: number): number[] {
  const room = TRAY.width - 60 - width;
  const gap = count > 1 ? Math.min(step, room / (count - 1)) : 0;
  const start = TRAY.width / 2 - (gap * (count - 1)) / 2;
  return Array.from({ length: count }, (_, i) => start + gap * i);
}

/** Where a tray card's top left goes for its center at `x`, raised by `lift`. */
const corner = (x: number, width: number, lift = 0) => ({
  x: x - width / 2,
  y: (TRAY.height - width / CARD_ASPECT) / 2 - lift,
});

// --- Arranging: scry, a look's hand and bottom ---

/** The library between the piles, in the sequence of tray items; then the outlines of the
 * cards the right pile still needs, from `GHOST` down. */
const LIBRARY = -1;
const GHOST = -2;

/** An arrangement's geometry in the tray, as scry.png draws it: faces larger than the
 * library's backs, which sit fanned and higher; neighbors `STEP` apart, closer when many;
 * the held card lifted, tilted and scaled up in the frame's aspect, its slot in the row
 * opened only `HELD_STEP`. */
const FACE = { width: 282, y: 237, turn: 3.2 };
const HELD = { width: 297, turn: -8.6 };
const STEP = 264;
const HELD_STEP = 166;

/** Centers for `items` in a row across the tray, `STEP` apart (`HELD_STEP` either side of
 * `held`), closer if they would not fit. */
function arranged(items: number[], held?: number): number[] {
  const steps = items
    .slice(1)
    .map((item, i) => (item === held || items[i] === held ? HELD_STEP : STEP));
  const span = steps.reduce((a, b) => a + b, 0);
  const fit = Math.min(1, (TRAY.width - 60 - FACE.width) / Math.max(span, 1));
  const centers = [TRAY.width / 2 - (span * fit) / 2];
  for (const step of steps)
    centers.push(centers[centers.length - 1]! + step * fit);
  return centers;
}

function ArrangeChoice({
  view,
  choice,
  busy,
  onPlan,
  onViewBattlefield,
  onCancel,
}: ChoiceOverlayProps & { choice: Arrange }) {
  const [piles, setPiles] = useState({
    left: choice.left,
    right: choice.right,
  });
  const sequence = [...piles.left, LIBRARY, ...piles.right];
  // Outlines where the right pile still needs cards.
  const missing = Math.max(0, (choice.rightCount ?? 0) - piles.right.length);
  const ghosts = Array.from({ length: missing }, (_, i) => GHOST - i);
  // Dropped at `x`, a card goes between the items either side of it as they stand (as `from`
  // lays them out with it held), so the row holds still until the card passes one; never left
  // of the library when the left pile is closed.
  const place = (slot: number, x: number, from: number[]) => {
    let items = from;
    for (let tries = 0; tries < from.length; tries++) {
      const stand = centersOf([...items, ...ghosts], slot);
      const others = items.filter((item) => item !== slot);
      let at = others.filter((item) => stand.get(item)! < x).length;
      if (choice.leftClosed) {
        at = Math.max(at, others.indexOf(LIBRARY) + 1);
      }
      const next = [...others.slice(0, at), slot, ...others.slice(at)];
      if (next.every((item, i) => item === items[i])) break;
      items = next;
    }
    return items;
  };
  // While a card is held, the others make room where it would drop.
  const [held, setHeld] = useState<{ slot: number; sequence: number[] }>();
  const heldFrom = (slot: number) =>
    held?.slot === slot ? held.sequence : sequence;
  const shown = held?.sequence ?? sequence;
  const center = centersOf([...shown, ...ghosts], held?.slot);
  const split = (items: number[]) => {
    const at = items.indexOf(LIBRARY);
    setPiles({ left: items.slice(0, at), right: items.slice(at + 1) });
  };
  // A click moves a card to the far end of the other pile: the bottom of the library, or its
  // top; with the left pile closed, to the top.
  const toggle = (slot: number) => {
    const others = sequence.filter((item) => item !== slot);
    split(
      piles.left.includes(slot) || choice.leftClosed
        ? [...others, slot]
        : [slot, ...others],
    );
  };
  const ready =
    choice.rightCount === undefined || piles.right.length === choice.rightCount;
  const objects = new Map(view.objects.map((o) => [o.slot, o]));
  const libraryAt = shown.indexOf(LIBRARY);
  return (
    <Shell
      view={view}
      choice={choice}
      busy={busy}
      onViewBattlefield={onViewBattlefield}
      onCancel={onCancel}
      actions={[
        {
          label: "Done",
          disabled: busy || !ready,
          onClick: () => onPlan(choice.plan(piles.left, piles.right)),
        },
      ]}
    >
      <div className="scry-zone choice-zone" data-choice="arrange">
        <motion.div
          className="choice-library"
          initial={false}
          animate={{ x: center.get(LIBRARY)! }}
          transition={SPRING}
        >
          <div
            style={{
              position: "absolute",
              left: -BACK_WIDTH / 2,
              top: 174 - BACK_WIDTH / CARD_ASPECT / 2,
            }}
          >
            <Library
              count={view.players.you.library.length}
              width={BACK_WIDTH}
            />
          </div>
        </motion.div>
        {ghosts.map((ghost) => (
          <div
            key={ghost}
            className="choice-ghost"
            style={{
              left: center.get(ghost)! - FACE.width / 2,
              top: FACE.y - FACE.width / CARD_ASPECT / 2,
              width: FACE.width,
              height: FACE.width / CARD_ASPECT,
            }}
          />
        ))}
        {shown.map(
          (item, i) =>
            item !== LIBRARY && (
              <DraggableCard
                key={item}
                object={objects.get(item)}
                x={center.get(item)!}
                y={FACE.y}
                width={FACE.width}
                z={i + 1}
                turn={i < libraryAt ? -FACE.turn : FACE.turn}
                disabled={busy}
                onMove={(p) =>
                  setHeld(
                    p === undefined
                      ? undefined
                      : {
                          slot: item,
                          sequence: place(item, p.x, heldFrom(item)),
                        },
                  )
                }
                onDrop={(p) => split(place(item, p.x, heldFrom(item)))}
                onClick={() => toggle(item)}
              />
            ),
        )}
        <span className="scry-label scry-bottom">{choice.labels[0]}</span>
        <span className="scry-label scry-top">{choice.labels[1]}</span>
      </div>
    </Shell>
  );
}

/** Each item's center in a row of `items`, as `arranged` lays them. */
function centersOf(items: number[], held?: number) {
  const centers = arranged(items, held);
  return new Map(items.map((item, i) => [item, centers[i]!]));
}

/** The library's backs, smaller than the faces either side. */
const BACK_WIDTH = 220;

/** A drag starts once the pointer has moved this far, in stage pixels; less is a click. */
const DRAG_THRESHOLD = 6;

/** A point in the tray, stage pixels. */
interface Point {
  x: number;
  y: number;
}

/** A tray card that glides to its place, centered at (`x`, `y`), and can be dragged about the
 * tray, lifted while held; `onMove` follows the held card's center (undefined once let go). */
function DraggableCard({
  object,
  x: centerX,
  y: centerY,
  width: restWidth,
  z,
  turn,
  out = false,
  disabled,
  onMove,
  onDrop,
  onClick,
}: {
  object?: ObjectView;
  x: number;
  y: number;
  width: number;
  z: number;
  turn: number;
  /** Dimmed: it can't go anywhere now, though it still shows whole on hover. */
  out?: boolean;
  disabled: boolean;
  onMove(at: Point | undefined): void;
  onDrop(at: Point): void;
  onClick(): void;
}) {
  const reduce = useReducedMotion() ?? false;
  const stageScale = useContext(StageScale);
  const x = useMotionValue(centerX);
  const y = useMotionValue(centerY);
  const [dragging, setDragging] = useState(false);
  const drag = useRef<{
    start: number;
    startY: number;
    from: number;
    fromY: number;
    moved: boolean;
  }>(undefined);
  useEffect(() => {
    if (dragging) return;
    const glide = reduce ? { duration: 0 } : SPRING;
    const a = animate(x, centerX, glide);
    const b = animate(y, centerY, glide);
    return () => {
      a.stop();
      b.stop();
    };
  }, [x, y, centerX, centerY, dragging, reduce]);
  const point = (e: PointerEvent) => ({
    x: e.clientX / stageScale,
    y: e.clientY / stageScale,
  });
  const width = dragging ? (restWidth * HELD.width) / FACE.width : restWidth;
  const height = width / CARD_ASPECT;
  const release = () => {
    drag.current = undefined;
    setDragging(false);
    onMove(undefined);
  };
  return (
    <motion.div
      className={[
        "choice-card",
        dragging && "choice-card-dragging",
        out && !dragging && "choice-card-out",
      ]
        .filter(Boolean)
        .join(" ")}
      data-slot={object?.slot}
      data-card={object?.name ?? undefined}
      style={{
        x,
        y,
        left: -width / 2,
        top: -height / 2,
        rotate: dragging ? HELD.turn : turn,
        zIndex: dragging ? 100 : z,
      }}
      onPointerDown={(e) => {
        if (disabled || e.button !== 0) return;
        e.currentTarget.setPointerCapture(e.pointerId);
        const p = point(e);
        drag.current = {
          start: p.x,
          startY: p.y,
          from: x.get(),
          fromY: y.get(),
          moved: false,
        };
      }}
      onPointerMove={(e) => {
        const d = drag.current;
        if (!d) return;
        const p = point(e);
        const dx = p.x - d.start;
        const dy = p.y - d.startY;
        if (!d.moved && Math.hypot(dx, dy) < DRAG_THRESHOLD) return;
        if (!d.moved) {
          d.moved = true;
          x.stop();
          y.stop();
          setDragging(true);
        }
        x.set(d.from + dx);
        y.set(d.fromY + dy);
        onMove({ x: x.get(), y: y.get() });
      }}
      onPointerUp={() => {
        const d = drag.current;
        if (!d) return;
        const at = { x: x.get(), y: y.get() };
        release();
        if (d.moved) onDrop(at);
        else onClick();
      }}
      onPointerCancel={release}
    >
      <ShownCard
        object={object}
        width={width}
        highlight={dragging ? "target" : undefined}
      />
    </motion.div>
  );
}

// --- Stacking: Jace's put-back ---

/** The library at the tray's left, face down, with the cards put on it cascading down its
 * face, the top one in front and whole; the hand fanned at the right. A card goes from one to
 * the other by a click or a drag, and into order on the library by a drag. */
const DECK = { x: 150, width: 230 };
/** How far each card on the library lies below the one under it: its name bar shows. */
const CASCADE = 46;
/** The library's backs, each this far up and left of the one above it: a deck's edge. */
const DECK_EDGE = { dx: -1.5, dy: -2.5 };
/** The hand's fan: from `left` to the tray's right edge less `right`, neighbors at most
 * `step` apart, turned up to `turn` degrees at its ends and dropped `drop` there. */
const FAN = { left: 350, right: 12, width: 230, step: 150, turn: 7, drop: 22 };
/** A card dropped left of this goes on the library; right of it, back to the hand. */
const DECK_REACH = (DECK.x + DECK.width / 2 + FAN.left) / 2 + 40;

/** The centers and turns of the cards on the library, the deepest first, for a column built
 * for `room` of them, centered down the tray. */
function cascade(count: number, room: number) {
  const height = DECK.width / CARD_ASPECT;
  const span = Math.min(
    CASCADE,
    (TRAY.height - 24 - height) / Math.max(room - 1, 1),
  );
  const top = TRAY.height / 2 - (span * (room - 1)) / 2;
  return {
    top,
    cards: Array.from({ length: count }, (_, i) => ({
      y: top + span * i,
      turn: i === count - 1 ? 0 : i % 2 === 0 ? -1.6 : 1.4,
    })),
  };
}

/** Centers and turns of `count` cards fanned across the tray's right. */
function fan(count: number) {
  const right = TRAY.width - FAN.right - FAN.width / 2;
  const left = FAN.left + FAN.width / 2;
  const step = count > 1 ? Math.min(FAN.step, (right - left) / (count - 1)) : 0;
  const start = (left + right) / 2 - (step * (count - 1)) / 2;
  return Array.from({ length: count }, (_, i) => {
    const t = count > 1 ? (i / (count - 1)) * 2 - 1 : 0;
    return {
      x: start + step * i,
      y: TRAY.height / 2 + FAN.drop * t * t,
      turn: FAN.turn * t,
    };
  });
}

function StackChoice({
  view,
  choice,
  busy,
  onPlan,
  onViewBattlefield,
  onCancel,
}: ChoiceOverlayProps & { choice: Stack }) {
  const [onto, setOnto] = useState<number[]>([]);
  const [held, setHeld] = useState<{ slot: number; at: Point }>();
  const full = onto.length >= choice.count;
  const room = Math.max(choice.count, 1);
  // Where a card held at `at` would land: on the library at the place its height picks, if
  // there's room for it there; else in the hand.
  const landing = (slot: number, at: Point): number[] => {
    const others = onto.filter((s) => s !== slot);
    if (at.x > DECK_REACH || others.length >= choice.count) return others;
    // Over the library, it goes onto every card whose middle it is below: dropped anywhere on
    // the deck but high up, it lands on top.
    const index = cascade(others.length, room).cards.filter(
      (c) => c.y < at.y,
    ).length;
    return [...others.slice(0, index), slot, ...others.slice(index)];
  };
  const shown = held ? landing(held.slot, held.at) : onto;
  const hand = choice.hand.filter((s) => !shown.includes(s));
  const column = cascade(shown.length, room);
  const fanned = fan(hand.length);
  const place = new Map<
    number,
    { x: number; y: number; turn: number; z: number }
  >();
  shown.forEach((slot, i) =>
    place.set(slot, { x: DECK.x, ...column.cards[i]!, z: 50 + i }),
  );
  hand.forEach((slot, i) => place.set(slot, { ...fanned[i]!, z: 1 + i }));
  const objects = new Map(view.objects.map((o) => [o.slot, o]));
  const library = view.players.you.library.length;
  const backs = Math.min(library, 6, 2 + Math.ceil(library / 10));
  const deckY = column.top - 14;
  // A click moves a card across: from the hand onto the top of the library while it has room,
  // from the library back to the hand.
  const toggle = (slot: number) =>
    setOnto((now) =>
      now.includes(slot)
        ? now.filter((s) => s !== slot)
        : now.length < choice.count
          ? [...now, slot]
          : now,
    );
  return (
    <Shell
      view={view}
      choice={choice}
      busy={busy}
      onViewBattlefield={onViewBattlefield}
      onCancel={onCancel}
      actions={[
        {
          label: "Done",
          disabled: busy || onto.length !== choice.count,
          onClick: () => onPlan(choice.plan(onto)),
        },
      ]}
    >
      <div className="scry-zone stack-zone choice-zone" data-choice="stack">
        <div
          className="choice-library"
          data-count={library}
          style={{
            left: DECK.x - DECK.width / 2,
            top: deckY - DECK.width / CARD_ASPECT / 2,
            width: DECK.width,
            height: DECK.width / CARD_ASPECT,
          }}
        >
          {library === 0 ? (
            <div className="stack-empty" />
          ) : (
            Array.from({ length: backs }, (_, i) => {
              const depth = backs - 1 - i;
              return (
                <CardBack
                  key={i}
                  width={DECK.width}
                  style={{
                    position: "absolute",
                    left: DECK_EDGE.dx * depth,
                    top: DECK_EDGE.dy * depth,
                  }}
                />
              );
            })
          )}
        </div>
        {choice.hand.map((slot) => {
          const at = place.get(slot)!;
          const inHand = !shown.includes(slot);
          return (
            <DraggableCard
              key={slot}
              object={objects.get(slot)}
              x={at.x}
              y={at.y}
              width={inHand ? FAN.width : DECK.width}
              z={at.z}
              turn={at.turn}
              out={inHand && full && held?.slot !== slot}
              disabled={busy}
              onMove={(p) => setHeld(p && { slot, at: p })}
              onDrop={(p) => setOnto(landing(slot, p))}
              onClick={() => toggle(slot)}
            />
          );
        })}
      </div>
    </Shell>
  );
}

// --- Picking: looks, discards, lands, legends ---

/** A card's width in a row of `count`. */
const rowWidth = (count: number) =>
  count <= 3 ? 260 : count <= 4 ? CARD_WIDTH : count <= 6 ? 190 : 160;

function PickChoice({
  view,
  choice,
  busy,
  onPlan,
  onViewBattlefield,
  onCancel,
}: ChoiceOverlayProps & { choice: Pick }) {
  const [picked, setPicked] = useState<number[]>(choice.picked ?? []);
  const toggle = (slot: number) =>
    setPicked((now) =>
      now.includes(slot)
        ? now.filter((s) => s !== slot)
        : choice.max === 1
          ? [slot]
          : now.length < choice.max
            ? [...now, slot]
            : now,
    );
  const count = picked.length;
  const ok = count >= choice.min && count <= choice.max;
  const objects = new Map(view.objects.map((o) => [o.slot, o]));
  const width = rowWidth(choice.cards.length);
  const centers = row(choice.cards.length, width, width + 24);
  return (
    <Shell
      view={view}
      choice={choice}
      busy={busy}
      onViewBattlefield={onViewBattlefield}
      onCancel={onCancel}
      actions={[
        ...(choice.pickable.length > 0 || !choice.decline
          ? [
              {
                label: choice.confirm(count),
                disabled: busy || !ok,
                onClick: () => onPlan(choice.plan(picked)),
              },
            ]
          : []),
        ...(choice.decline
          ? [
              {
                label: choice.decline,
                disabled: busy,
                onClick: () => onPlan(choice.plan([])),
              },
            ]
          : []),
      ]}
    >
      <div className="scry-zone choice-zone" data-choice="pick">
        {choice.cards.map((slot, i) => {
          const can = choice.pickable.includes(slot);
          const on = picked.includes(slot);
          const at = corner(centers[i]!, width, on ? 28 : 0);
          return (
            <motion.button
              key={slot}
              type="button"
              className={can ? "choice-card" : "choice-card choice-card-out"}
              data-slot={slot}
              data-card={objects.get(slot)?.name ?? undefined}
              aria-pressed={on}
              disabled={busy || !can}
              initial={false}
              animate={at}
              transition={SPRING}
              style={{ zIndex: i + 1 }}
              onClick={() => toggle(slot)}
            >
              <ShownCard
                object={objects.get(slot)}
                width={width}
                highlight={on ? "source" : can ? "target" : undefined}
              />
            </motion.button>
          );
        })}
      </div>
    </Shell>
  );
}

// --- Asking: pay or counter, pay life, yes or no, X ---

function AskChoice({
  view,
  choice,
  busy,
  onPlan,
  onViewBattlefield,
  onCancel,
}: ChoiceOverlayProps & { choice: Ask }) {
  const objects = new Map(view.objects.map((o) => [o.slot, o]));
  const width = rowWidth(choice.cards.length);
  const centers = row(choice.cards.length, width, width + 24);
  return (
    <Shell
      view={view}
      choice={choice}
      busy={busy}
      onViewBattlefield={onViewBattlefield}
      onCancel={onCancel}
      actions={choice.answers.map((a) => ({
        label: <RulesText text={a.label} />,
        disabled: busy,
        secondary: choice.equalAnswers,
        onClick: () => onPlan(a.plan),
      }))}
    >
      {choice.cards.length > 0 && (
        <div className="scry-zone choice-zone" data-choice="ask">
          {choice.cards.map((slot, i) => {
            const at = corner(centers[i]!, width);
            return (
              <div
                key={slot}
                className="choice-card"
                data-slot={slot}
                data-card={objects.get(slot)?.name ?? undefined}
                style={{ left: at.x, top: at.y, zIndex: i + 1 }}
              >
                <ShownCard object={objects.get(slot)} width={width} />
              </div>
            );
          })}
        </div>
      )}
    </Shell>
  );
}

// --- Asking, floating: pay life, yes or no, X, cast it ---

/** A question of no cards: its title over a button per answer and, while it can be taken
 * back, Cancel. Escape cancels. */
function AskFloat({
  busy,
  choice,
  onPlan,
  onCancel,
}: ChoiceOverlayProps & { choice: Ask }) {
  useEffect(() => {
    if (!onCancel || busy) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape" && !e.defaultPrevented) onCancel();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onCancel, busy]);
  return (
    <FloatPrompt label={choice.title} title={<RulesText text={choice.title} />}>
      <PromptRow>
        {choice.answers.map((a, i) => (
          <Button
            key={i}
            variant={i === 0 && !choice.equalAnswers ? "primary" : "secondary"}
            disabled={busy}
            onClick={() => onPlan(a.plan)}
          >
            <RulesText text={a.label} />
          </Button>
        ))}
        {onCancel && (
          <Button variant="secondary" disabled={busy} onClick={onCancel}>
            Cancel
          </Button>
        )}
      </PromptRow>
    </FloatPrompt>
  );
}
