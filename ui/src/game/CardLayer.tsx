// The game screen's cards: one element per public slot, placed by layout.ts and animated with
// Motion from wherever it was to wherever the latest view puts it, so a card drawn, played,
// cast, resolved, killed, bounced or exiled travels rather than jumps (decision D10). Hand
// cards with something to play drag onto the battlefield; a click on any card asks the board
// what it offers.
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
} from "motion/react";
import { Card, CardBack, type Highlight } from "../components/Card";
import { useCardArt } from "../cards";
import type { ObjectView } from "../generated/protocol";
import { StageScale } from "../screens/Table";
import { frameFace, unknownFace } from "./faces";
import type { Layout, Placement } from "./layout";

/** Where on the stage a pointer is, in stage pixels. */
export interface Point {
  x: number;
  y: number;
}

/** How a card is shown beyond its placement. */
export interface CardState {
  highlight?: Highlight;
  /** It has something to play or activate now. */
  playable?: boolean;
  /** It can be dragged onto the battlefield. */
  draggable?: boolean;
}

export interface CardLayerProps {
  /** Changes with each view shown (the view itself will do): a card left where it was dropped
   * to be played stays there until the epoch changes, then goes to its placement. */
  epoch: unknown;
  objects: ObjectView[];
  placements: Layout;
  states: Map<number, CardState>;
  /** A card was clicked (or tapped without dragging) at `at`. */
  onClick(slot: number, at: Point): void;
  /** A dragged card was let go at `at`; resolves to whether it was played, so it stays where
   * it was dropped until the next view moves it, rather than flying back to the hand. */
  onDrop(slot: number, at: Point): boolean;
  onHover(slot: number | undefined): void;
}

/** A drag starts once the pointer has moved this far, in stage pixels; less is a click. */
const DRAG_THRESHOLD = 6;

/** Dropped above this line, a card is on the battlefield rather than back in the hand. */
export const BATTLEFIELD_EDGE = 690;

const SPRING = { type: "spring", stiffness: 210, damping: 28 } as const;

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

/** Animates `value` to `to` whenever `to` or `restart` changes, unless the pointer is
 * `holding` the value where it is; once let go, the value glides to `to`. */
function useGlide(
  value: MotionValue<number>,
  to: number,
  instant: boolean,
  holding = false,
  restart?: unknown,
) {
  useEffect(() => {
    if (holding) return;
    const controls = animate(value, to, instant ? { duration: 0 } : SPRING);
    return () => controls.stop();
  }, [value, to, instant, holding, restart]);
}

/** Where the pointer has a card: dragging it, or dropped where it was played until the view
 * it was played on (its epoch) is replaced. */
type Held = { dragging: true } | { dragging: false; epoch: unknown };

function CardElement({
  object,
  place,
  state,
  layer,
  appearing,
}: {
  object: ObjectView;
  place: Placement;
  state: CardState;
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
  // The pointer places the card while it holds it, whatever its placement does meanwhile (it
  // changes as soon as a raised hand card stops being hovered).
  const holding =
    held !== undefined && (held.dragging || held.epoch === layer.epoch);
  useGlide(x, place.x, reduce, holding);
  useGlide(y, place.y, reduce, holding);
  // A card changing looks is drawn at another width (a back, a full frame, a battlefield
  // frame): before that is painted, its scale makes up the difference, and then glides, so the
  // card's size changes smoothly too.
  const drawnWidth = useRef(place.width);
  useLayoutEffect(() => {
    if (drawnWidth.current === place.width) return;
    scale.jump((scale.get() * drawnWidth.current) / place.width);
    drawnWidth.current = place.width;
  }, [scale, place.width]);
  useGlide(scale, place.scale, reduce, false, place.width);
  useGlide(rotate, dragging ? 0 : place.rotate, reduce);
  useGlide(skewX, place.skew, reduce);
  useGlide(opacity, place.opacity, reduce);
  const drag = useRef<{ start: Point; from: Point; moved: boolean }>(undefined);

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
    // A played card stays where it was dropped until the next view moves it on; any other
    // goes back to its place.
    const played = at.y < BATTLEFIELD_EDGE && layer.onDrop(object.slot, at);
    setHeld(played ? { dragging: false, epoch: layer.epoch } : undefined);
  };

  const classes = [
    "card-el",
    `card-el-${place.look}`,
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
        zIndex: dragging ? 1000 : place.z,
      }}
      exit={reduce ? undefined : { opacity: 0, transition: { duration: 0.3 } }}
      onPointerDown={place.look === "hidden" ? undefined : onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
      onPointerCancel={() => {
        drag.current = undefined;
        setHeld(undefined);
      }}
      onPointerEnter={() =>
        !drag.current && place.look !== "hidden" && layer.onHover(object.slot)
      }
      onPointerLeave={() => !drag.current && layer.onHover(undefined)}
    >
      <Look object={object} place={place} state={state} />
    </motion.div>
  );
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
  const art = useCardArt(object.name ?? "");
  if (place.look === "hidden") return null;
  if (place.look === "back" || object.name === null) {
    return <CardBack width={place.width} />;
  }
  const card = art ? frameFace(object, art) : unknownFace(object.name);
  const field = place.look === "field" || place.look === "land";
  const card_ = (
    <Card
      card={card}
      variant={field ? "field" : "full"}
      width={place.width}
      tapped={field && object.tapped}
      highlight={state.highlight}
      className={state.playable ? "card-playable" : undefined}
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
