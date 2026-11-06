// The game screen's cards: one element per public slot, placed by layout.ts and animated with
// Motion from wherever it was to wherever the latest view puts it, so a card drawn, played,
// cast, resolved, killed, bounced or exiled travels rather than jumps (decision D10). Hand
// cards with something to play drag onto the battlefield, attackers forward and blockers onto
// attackers; a click on any card asks the board what it offers.
import {
  useContext,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type PointerEvent as ReactPointerEvent,
} from "react";
import {
  animate,
  AnimatePresence,
  motion,
  useMotionValue,
  useReducedMotion,
  type MotionValue,
  type Transition,
} from "motion/react";
import { Card, CardBack, type Highlight } from "../components/Card";
import type { ObjectView, Side } from "../generated/protocol";
import { StageScale } from "./Stage";
import { useFace } from "./faces";
import { RAISE, type Layout, type Placement } from "./layout";

/** Where on the stage a pointer is, in stage pixels. */
export interface Point {
  x: number;
  y: number;
}

/** What a dragged card was let go over: another card, or a player's life orb. */
export type Onto = { slot: number } | { player: Side };

/** Where a drag went: the pointer at its start and where it was let go, in stage pixels, and
 * what it was let go over. */
export interface Drop {
  start: Point;
  at: Point;
  onto?: Onto;
}

/** The element ids arrows find a card by, and its combat mark by (`${id}-mark`). */
export const cardId = (slot: number) => `card-${slot}`;

/** How a card is shown beyond its placement. */
export interface CardState {
  highlight?: Highlight;
  /** It has something to play or activate now. */
  playable?: boolean;
  /** It can be dragged: onto the battlefield, forward to attack, onto an attacker to block. */
  draggable?: boolean;
  /** The payment of the cast being dragged or set up taps it. */
  paying?: boolean;
}

export interface CardLayerProps {
  /** Changes with each view shown (the view itself will do): a card left where it was dropped
   * to be played stays there until the epoch changes, then goes to its placement. */
  epoch: unknown;
  objects: ObjectView[];
  placements: Layout;
  /** The card the pointer is over: a hand card rises out of the fan (RAISE), drawn inside its
   * element, which keeps its place so that nothing else moves and the pointer keeps the card. */
  hovered?: number;
  states: Map<number, CardState>;
  /** A card was clicked (or tapped without dragging) at `at`. */
  onClick(slot: number, at: Point): void;
  /** A dragged card was let go; returns whether that did something, so the card stays where
   * it was dropped until the next view moves it, rather than flying back to its place. */
  onDrop(slot: number, drop: Drop): boolean;
  onHover(slot: number | undefined): void;
  /** A card was picked up to drag, or put down again (`undefined`). */
  onLift?(slot: number | undefined): void;
}

/** A drag starts once the pointer has moved this far, in stage pixels; less is a click. */
const DRAG_THRESHOLD = 6;

const SPRING: Transition = { type: "spring", stiffness: 210, damping: 28 };
/** A hand card rises and settles back quickly and without bounce. */
const RAISING: Transition = { type: "tween", duration: 0.14, ease: "easeOut" };
/** A raised hand card is drawn over the rest of the hand. */
const RAISED_Z = 300;

export function CardLayer(props: CardLayerProps) {
  const { objects, placements } = props;
  // Cards there from the start are simply there; later ones (tokens) fade and grow in.
  const shown = useRef(false);
  useEffect(() => {
    shown.current = true;
  }, []);
  return (
    <div className="card-layer">
      <AnimatePresence>
        {objects.map((o) => {
          const place = placements.get(o.slot);
          return (
            place && (
              <CardElement
                key={o.slot}
                object={o}
                place={place}
                state={props.states.get(o.slot) ?? {}}
                raised={props.hovered === o.slot}
                layer={props}
                appearing={shown.current}
              />
            )
          );
        })}
      </AnimatePresence>
    </div>
  );
}

/** Animates `value` to `to` (`how`, a spring by default) whenever `to` or `restart` changes,
 * unless the pointer is `holding` the value where it is; once let go, the value glides to
 * `to`. */
function useGlide(
  value: MotionValue<number>,
  to: number,
  instant: boolean,
  { holding = false, restart, how = SPRING }: GlideOptions = {},
) {
  useEffect(() => {
    if (holding) return;
    const controls = animate(value, to, instant ? { duration: 0 } : how);
    return () => controls.stop();
  }, [value, to, instant, holding, restart, how]);
}

interface GlideOptions {
  holding?: boolean;
  restart?: unknown;
  how?: Transition;
}

/** Where the pointer has a card: dragging it, or dropped where it was played until the view
 * it was played on (its epoch) is replaced. */
type Held = { dragging: true } | { dragging: false; epoch: unknown };

function CardElement({
  object,
  place,
  state,
  raised,
  layer,
  appearing,
}: {
  object: ObjectView;
  place: Placement;
  state: CardState;
  raised: boolean;
  layer: CardLayerProps;
  appearing: boolean;
}) {
  const reduce = useReducedMotion() ?? false;
  const stageScale = useContext(StageScale);
  const grow = appearing && !reduce;
  const x = useMotionValue(place.x);
  const y = useMotionValue(place.y);
  const scale = useMotionValue(place.scale * (grow ? 0.6 : 1));
  const rotate = useMotionValue(place.rotate);
  const skewX = useMotionValue(place.skew);
  const opacity = useMotionValue(grow ? 0 : place.opacity);
  const [held, setHeld] = useState<Held>();
  const dragging = held?.dragging === true;
  // The pointer places the card while it holds it, whatever its placement does meanwhile.
  const holding =
    held !== undefined && (held.dragging || held.epoch === layer.epoch);
  useGlide(x, place.x, reduce, { holding });
  useGlide(y, place.y, reduce, { holding });
  // A card changing looks is drawn at another type unit (a back, a full frame, a battlefield
  // frame): before that is painted, its scale makes up the difference, and then glides, so the
  // card's size changes smoothly too.
  const unit = place.width / 12;
  const drawnUnit = useRef(unit);
  useLayoutEffect(() => {
    if (drawnUnit.current === unit) return;
    scale.jump((scale.get() * drawnUnit.current) / unit);
    drawnUnit.current = unit;
  }, [scale, unit]);
  useGlide(scale, place.scale, reduce, { restart: unit });
  // A hand card is raised while hovered and while the pointer holds it; anything else dragged
  // straightens.
  const hand = place.look === "hand";
  const lifted = hand && (raised || held !== undefined);
  useGlide(rotate, dragging && !hand ? 0 : place.rotate, reduce);
  useGlide(skewX, place.skew, reduce);
  useGlide(opacity, place.opacity, reduce);
  // Raised, the card turns back upright about its top center and rises straight up the screen,
  // inside its element, which stays turned in the fan.
  const raise = useRaise(lifted ? place.rotate : undefined, reduce);
  const drag = useRef<{ start: Point; from: Point; moved: boolean }>(undefined);
  // A card in a library is unseen, and one on a graveyard or exile pile is the pile's: the
  // pile's button opens it (Piles.tsx). Neither is pointed at alone.
  const untouchable = place.look === "hidden" || place.look === "pile";

  const stagePoint = (e: ReactPointerEvent) => {
    const stage = (e.currentTarget as HTMLElement).closest(".stage");
    const box = stage?.getBoundingClientRect();
    return {
      x: (e.clientX - (box?.left ?? 0)) / stageScale,
      y: (e.clientY - (box?.top ?? 0)) / stageScale,
    };
  };

  const onPointerDown = (e: ReactPointerEvent) => {
    if (e.button !== 0) return;
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    drag.current = {
      start: stagePoint(e),
      from: { x: x.get(), y: y.get() },
      moved: false,
    };
  };
  const onPointerMove = (e: ReactPointerEvent) => {
    const d = drag.current;
    if (!d || !state.draggable) return;
    const at = stagePoint(e);
    const [dx, dy] = [at.x - d.start.x, at.y - d.start.y];
    if (!d.moved && Math.hypot(dx, dy) < DRAG_THRESHOLD) return;
    if (!d.moved) {
      d.moved = true;
      x.stop();
      y.stop();
      setHeld({ dragging: true });
      layer.onHover(undefined);
      layer.onLift?.(object.slot);
    }
    x.set(d.from.x + dx);
    y.set(d.from.y + dy);
  };
  const onPointerUp = (e: ReactPointerEvent) => {
    const d = drag.current;
    drag.current = undefined;
    if (!d) return;
    const at = stagePoint(e);
    if (!d.moved) {
      layer.onClick(object.slot, at);
      return;
    }
    layer.onLift?.(undefined);
    // A played card stays where it was dropped until the next view moves it on; any other
    // goes back to its place.
    const played = layer.onDrop(object.slot, {
      start: d.start,
      at,
      onto: under(e, e.currentTarget as HTMLElement),
    });
    setHeld(played ? { dragging: false, epoch: layer.epoch } : undefined);
  };

  const classes = [
    "card-el",
    `card-el-${place.look}`,
    `card-el-${object.controller}`,
    state.playable && "card-el-playable",
    state.draggable && "card-el-draggable",
    dragging && "card-el-dragging",
  ]
    .filter(Boolean)
    .join(" ");
  return (
    <motion.div
      className={classes}
      data-slot={object.slot}
      data-zone={object.zone}
      data-card={object.name ?? undefined}
      style={{
        x,
        y,
        scale,
        rotate,
        skewX,
        opacity,
        originX: 0,
        originY: 0,
        zIndex: dragging ? 1000 : lifted ? RAISED_Z : place.z,
      }}
      exit={reduce ? undefined : { opacity: 0, transition: { duration: 0.3 } }}
      onPointerDown={untouchable ? undefined : onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
      onPointerCancel={() => {
        if (drag.current?.moved) layer.onLift?.(undefined);
        drag.current = undefined;
        setHeld(undefined);
      }}
      onPointerEnter={() =>
        !drag.current && !untouchable && layer.onHover(object.slot)
      }
      onPointerLeave={() => !drag.current && layer.onHover(undefined)}
    >
      <motion.div style={raise}>
        <Look object={object} place={place} state={state} />
      </motion.div>
    </motion.div>
  );
}

/** The transform raising a hand card out of a fan that turns it `turn` degrees, or none when
 * `turn` is undefined: it turns back upright about the card's top center, rises `RAISE.lift`
 * straight up the screen and grows by `RAISE.scale`. */
function useRaise(turn: number | undefined, instant: boolean) {
  const x = useMotionValue(0);
  const y = useMotionValue(0);
  const rotate = useMotionValue(0);
  const scale = useMotionValue(1);
  const a = ((turn ?? 0) * Math.PI) / 180;
  // Straight up the screen, in the turned card's own axes.
  const lift = turn === undefined ? 0 : RAISE.lift;
  const how = { how: RAISING };
  useGlide(x, -lift * Math.sin(a), instant, how);
  useGlide(y, -lift * Math.cos(a), instant, how);
  useGlide(rotate, -(turn ?? 0), instant, how);
  useGlide(scale, turn === undefined ? 1 : RAISE.scale, instant, how);
  return { x, y, rotate, scale, originX: 0.5, originY: 0 };
}

/** What is under the pointer of event `e` besides the dragged element `self`: a card or a
 * life orb. */
function under(e: ReactPointerEvent, self: HTMLElement): Onto | undefined {
  for (const hit of document.elementsFromPoint(e.clientX, e.clientY)) {
    if (self.contains(hit)) continue;
    const card = hit.closest<HTMLElement>("[data-slot]");
    if (card?.dataset.slot) return { slot: Number(card.dataset.slot) };
    const life = hit.closest<HTMLElement>("[data-life]")?.dataset.life;
    if (life === "you" || life === "opponent") return { player: life };
  }
  return undefined;
}

/** The card itself, as its placement draws it. */
function Look({
  object,
  place,
  state,
}: {
  object: ObjectView;
  place: Placement;
  state: CardState;
}) {
  const card = useFace(object);
  if (place.look === "hidden") return null;
  // A face-down permanent shows its back to both players; its controller's hover still names
  // its card (CR 708.5).
  if (
    place.look === "back" ||
    object.name === null ||
    object.facing === "down"
  ) {
    return <CardBack width={place.width} />;
  }
  const field =
    place.look === "field" || place.look === "land" || place.look === "beneath";
  const combat = field ? object.combat?.kind : undefined;
  const card_ = (
    <Card
      id={cardId(object.slot)}
      card={card}
      combat={
        combat === "attacking"
          ? "attacker"
          : combat === "blocking"
            ? "blocker"
            : undefined
      }
      variant={field ? "field" : "full"}
      width={place.width}
      tapped={field && object.tapped}
      highlight={state.highlight}
      // A gold or cyan glow says more than the playable edge, which would hide it.
      className={
        [
          state.playable && !state.highlight && "card-playable",
          state.paying && "card-paying",
          place.look === "beneath" && "card-under",
        ]
          .filter(Boolean)
          .join(" ") || undefined
      }
    />
  );
  return field ? (
    <div className="on-table">{card_}</div>
  ) : place.look === "hand" ? (
    <div className="hand-card">{card_}</div>
  ) : (
    card_
  );
}
