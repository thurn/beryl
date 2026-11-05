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
import { useCardArt } from "../cards";
import type { ObjectView, PlayerView } from "../generated/protocol";
import { StageScale } from "../screens/Table";
import type { Arrange, Ask, Choice, Pick, Source } from "./choices";
import type { Intent } from "./decisions";
import { frameFace, unknownFace } from "./faces";

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
  const art = useCardArt(shown ?? "");
  if (!shown) return <CardBack width={width} />;
  const face = art
    ? frameFace(object ?? { ...NOWHERE, name: shown }, art)
    : unknownFace(shown);
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
  const laid = [...sequence, ...ghosts];
  const centers = row(laid.length, CARD_WIDTH, CARD_WIDTH * 0.8);
  const center = new Map(laid.map((item, i) => [item, centers[i]!]));
  const split = (items: number[]) => {
    const at = items.indexOf(LIBRARY);
    setPiles({ left: items.slice(0, at), right: items.slice(at + 1) });
  };
  // Dropped at `x`, a card goes between the items either side of it; never left of the
  // library when the left pile is closed.
  const drop = (slot: number, x: number) => {
    const others = sequence.filter((item) => item !== slot);
    let at = others.filter((item) => center.get(item)! < x).length;
    if (choice.leftClosed) {
      at = Math.max(at, others.indexOf(LIBRARY) + 1);
    }
    split([...others.slice(0, at), slot, ...others.slice(at)]);
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
  const libraryX = center.get(LIBRARY)!;
  const objects = new Map(view.objects.map((o) => [o.slot, o]));
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
          animate={{ x: libraryX }}
          transition={SPRING}
        >
          <CardBack
            width={CARD_WIDTH}
            style={{ ...tilted(-31, -10), zIndex: 0 }}
          />
          <CardBack width={CARD_WIDTH} style={{ ...tilted(31, 5) }} />
        </motion.div>
        {ghosts.map((ghost) => {
          const at = corner(center.get(ghost)!, CARD_WIDTH);
          return (
            <div
              key={ghost}
              className="choice-ghost"
              style={{
                left: at.x,
                top: at.y,
                width: CARD_WIDTH,
                height: CARD_WIDTH / ASPECT.full,
              }}
            />
          );
        })}
        {sequence.map(
          (item, i) =>
            item !== LIBRARY && (
              <DraggableCard
                key={item}
                object={objects.get(item)}
                x={centers[i]!}
                z={i + 1}
                turn={i < sequence.indexOf(LIBRARY) ? -4 : 3}
                disabled={busy}
                onDrop={(x) => drop(item, x)}
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

/** A card back in the library pile, `dx` off its center and turned `turn` degrees. */
const tilted = (dx: number, turn: number) =>
  ({
    position: "absolute",
    left: dx - CARD_WIDTH / 2,
    top: corner(0, CARD_WIDTH).y,
    rotate: `${turn}deg`,
  }) as const;

/** A drag starts once the pointer has moved this far, in stage pixels; less is a click. */
const DRAG_THRESHOLD = 6;

/** A tray card that glides to its place and can be dragged along the tray. */
function DraggableCard({
  object,
  x: centerX,
  z,
  turn,
  disabled,
  onDrop,
  onClick,
}: {
  object?: ObjectView;
  x: number;
  z: number;
  turn: number;
  disabled: boolean;
  onDrop(x: number): void;
  onClick(): void;
}) {
  const reduce = useReducedMotion() ?? false;
  const stageScale = useContext(StageScale);
  const home = corner(centerX, CARD_WIDTH);
  const x = useMotionValue(home.x);
  const y = useMotionValue(home.y);
  const [dragging, setDragging] = useState(false);
  const drag = useRef<{
    start: number;
    startY: number;
    from: number;
    moved: boolean;
  }>(undefined);
  useEffect(() => {
    if (dragging) return;
    const glide = reduce ? { duration: 0 } : SPRING;
    const a = animate(x, home.x, glide);
    const b = animate(y, home.y, glide);
    return () => {
      a.stop();
      b.stop();
    };
  }, [x, y, home.x, home.y, dragging, reduce]);
  const point = (e: PointerEvent) => ({
    x: e.clientX / stageScale,
    y: e.clientY / stageScale,
  });
  return (
    <motion.div
      className={dragging ? "choice-card choice-card-dragging" : "choice-card"}
      data-slot={object?.slot}
      data-card={object?.name ?? undefined}
      style={{ x, y, rotate: dragging ? 0 : turn, zIndex: dragging ? 100 : z }}
      onPointerDown={(e) => {
        if (disabled || e.button !== 0) return;
        e.currentTarget.setPointerCapture(e.pointerId);
        const p = point(e);
        drag.current = { start: p.x, startY: p.y, from: x.get(), moved: false };
      }}
      onPointerMove={(e) => {
        const d = drag.current;
        if (!d) return;
        const p = point(e);
        const dx = p.x - d.start;
        if (!d.moved && Math.hypot(dx, p.y - d.startY) < DRAG_THRESHOLD) return;
        if (!d.moved) {
          d.moved = true;
          x.stop();
          setDragging(true);
        }
        x.set(d.from + dx);
      }}
      onPointerUp={() => {
        const d = drag.current;
        drag.current = undefined;
        if (!d) return;
        setDragging(false);
        if (d.moved) onDrop(x.get() + CARD_WIDTH / 2);
        else onClick();
      }}
      onPointerCancel={() => {
        drag.current = undefined;
        setDragging(false);
      }}
    >
      <ShownCard
        object={object}
        width={CARD_WIDTH}
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
