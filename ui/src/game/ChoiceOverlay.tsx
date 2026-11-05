// A choose-or-order decision over the dimmed table, in the overlay of docs/ui_mockups/scry.png
// (decision D10): the cards in a tray, picked by clicking or ordered by dragging (each drag has
// a click fallback), the source card at the right, and "View Battlefield" to peek under it.
// The overlay says the whole choice; `onPlan` gets the picks that answer it (game/choices.ts).
import {
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
import { ASPECT, Card, CardBack, type Highlight } from "../components/Card";
import { Overlay, type OverlayAction } from "../components/Overlay";
import { RulesText } from "../components/Mana";
import type { ObjectView, PlayerView } from "../generated/protocol";
import { StageScale } from "../screens/Table";
import type { Arrange, Ask, Choice, Pick, Source } from "./choices";
import type { Intent } from "./decisions";
import { useFace } from "./faces";

export interface ChoiceOverlayProps {
  view: PlayerView;
  choice: Choice;
  /** An answer is on its way: the choice shows but takes no input. */
  busy: boolean;
  onPlan(plan: Intent[]): void;
  onViewBattlefield(): void;
}

export function ChoiceOverlay(props: ChoiceOverlayProps) {
  const { choice } = props;
  return choice.kind === "arrange" ? (
    <ArrangeChoice {...props} choice={choice} />
  ) : choice.kind === "pick" ? (
    <PickChoice {...props} choice={choice} />
  ) : (
    <AskChoice {...props} choice={choice} />
  );
}

/** The tray's size (the overlay's choice column) and the cards' in it, stage pixels. */
const TRAY = { width: 1000, height: 468 };
const CARD_WIDTH = 220;
const SPRING = { type: "spring", stiffness: 260, damping: 30 } as const;

/** The shell around a tray: title, instruction, source card, buttons. */
function Shell({
  view,
  choice,
  actions,
  onViewBattlefield,
  children,
}: {
  view: PlayerView;
  choice: Choice;
  actions: OverlayAction[];
  onViewBattlefield(): void;
  children?: ReactNode;
}) {
  const source = choice.source;
  return (
    <Overlay
      className={`choice choice-${choice.kind}`}
      title={<RulesText text={choice.title} />}
      instruction={<RulesText text={choice.instruction} />}
      aside={source && <SourceCard view={view} source={source} />}
      actions={actions}
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
  const face = useFace(object ?? { ...NOWHERE, name: shown ?? null });
  if (!shown) return <CardBack width={width} />;
  return <Card card={face} width={width} highlight={highlight} />;
}

/** A card known only by name, for frameFace. */
const NOWHERE: ObjectView = {
  slot: 0,
  zone: "nowhere",
  owner: "you",
  controller: "you",
  name: null,
  token: false,
  tapped: false,
  summoningSick: false,
  damage: 0,
  counters: [],
  types: [],
  pt: null,
  attachedTo: null,
  exiledBy: null,
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
  y: (TRAY.height - width / ASPECT.full) / 2 - lift,
});

// --- Arranging: scry, Jace's put-back ---

/** The library between the piles, in the sequence of tray items; then the outlines of the
 * cards the right pile still needs, from `GHOST` down. */
const LIBRARY = -1;
const GHOST = -2;

/** An arrangement's geometry in the tray, as scry.png draws it: faces larger than the
 * library's backs, which sit fanned and higher; neighbors `STEP` apart, closer when many;
 * the held card lifted, tilted and scaled up in the frame's aspect, its slot in the row
 * opened only `HELD_STEP`. */
const FACE = { width: 282, y: 237, turn: 3.2 };
const BACKS = [
  { dx: -31, y: 174, turn: -10 },
  { dx: 31, y: 171, turn: 5 },
];
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
      onViewBattlefield={onViewBattlefield}
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
          {BACKS.map((b, i) => (
            <CardBack
              key={i}
              width={BACK_WIDTH}
              style={{
                position: "absolute",
                left: b.dx - BACK_WIDTH / 2,
                top: b.y - BACK_WIDTH / ASPECT.full / 2,
                rotate: `${b.turn}deg`,
              }}
            />
          ))}
        </motion.div>
        {ghosts.map((ghost) => (
          <div
            key={ghost}
            className="choice-ghost"
            style={{
              left: center.get(ghost)! - FACE.width / 2,
              top: FACE.y - FACE.width / ASPECT.full / 2,
              width: FACE.width,
              height: FACE.width / ASPECT.full,
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
                z={i + 1}
                turn={i < libraryAt ? -FACE.turn : FACE.turn}
                disabled={busy}
                onMove={(x) =>
                  setHeld(
                    x === undefined
                      ? undefined
                      : {
                          slot: item,
                          sequence: place(item, x, heldFrom(item)),
                        },
                  )
                }
                onDrop={(x) => split(place(item, x, heldFrom(item)))}
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

/** A tray card that glides to its place and can be dragged about the tray, lifted while
 * held; `onMove` follows the held card's center (undefined once let go). */
function DraggableCard({
  object,
  x: centerX,
  z,
  turn,
  disabled,
  onMove,
  onDrop,
  onClick,
}: {
  object?: ObjectView;
  x: number;
  z: number;
  turn: number;
  disabled: boolean;
  onMove(x: number | undefined): void;
  onDrop(x: number): void;
  onClick(): void;
}) {
  const reduce = useReducedMotion() ?? false;
  const stageScale = useContext(StageScale);
  const x = useMotionValue(centerX);
  const y = useMotionValue(FACE.y);
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
    const b = animate(y, FACE.y, glide);
    return () => {
      a.stop();
      b.stop();
    };
  }, [x, y, centerX, dragging, reduce]);
  const point = (e: PointerEvent) => ({
    x: e.clientX / stageScale,
    y: e.clientY / stageScale,
  });
  const width = dragging ? HELD.width : FACE.width;
  const height = width / ASPECT.full;
  const release = () => {
    drag.current = undefined;
    setDragging(false);
    onMove(undefined);
  };
  return (
    <motion.div
      className={dragging ? "choice-card choice-card-dragging" : "choice-card"}
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
        onMove(x.get());
      }}
      onPointerUp={() => {
        const d = drag.current;
        if (!d) return;
        const at = x.get();
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
}: ChoiceOverlayProps & { choice: Pick }) {
  const [picked, setPicked] = useState<number[]>([]);
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
      onViewBattlefield={onViewBattlefield}
      actions={[
        {
          label: choice.confirm(count),
          disabled: busy || !ok,
          onClick: () => onPlan(choice.plan(picked)),
        },
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
}: ChoiceOverlayProps & { choice: Ask }) {
  const objects = new Map(view.objects.map((o) => [o.slot, o]));
  const width = rowWidth(choice.cards.length);
  const centers = row(choice.cards.length, width, width + 24);
  return (
    <Shell
      view={view}
      choice={choice}
      onViewBattlefield={onViewBattlefield}
      actions={choice.answers.map((a) => ({
        label: <RulesText text={a.label} />,
        disabled: busy,
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
