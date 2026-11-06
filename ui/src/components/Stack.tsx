// The stack-item frame (a spell or ability on the stack) and the arrows that tie it to its source
// (gold) and targets (cyan).
import { useLayoutEffect, useRef, useState, type CSSProperties } from "react";
import { CardArt, frameKey, type CardFace } from "./Card";
import { useTextFit } from "./fit";
import { RulesText } from "./Mana";
import {
  retrace,
  route,
  type Box,
  type Choice,
  type Leg,
  type Vector,
} from "./route";

export interface StackItemProps {
  source: CardFace;
  /** What the item is, such as "Triggered Ability"; omitted for a spell. */
  label?: string;
  text: string;
  showArt?: boolean;
  width?: number;
  /** The item's height, when it is not the frame's usual aspect (as without art). */
  height?: number;
  /** The frame's unit in pixels, when not width / 12. */
  unit?: number;
  id?: string;
}

/** A stack item: the source's name bar over a shield-shaped rules box. */
export function StackItem({
  source,
  label,
  text,
  showArt = false,
  width = 170,
  height,
  unit,
  id,
}: StackItemProps) {
  const nameRef = useTextFit(source.name);
  return (
    <div
      id={id}
      className={`stack-item frame-${frameKey(source)}${showArt ? "" : " stack-item-compact"}`}
      style={{ width, height, fontSize: unit ?? width / 12 }}
    >
      <div className="card-rim">
        <div className="card-name-bar">
          <span className="card-name" ref={nameRef}>
            {source.name}
          </span>
        </div>
        {showArt && <CardArt card={source} />}
        {label && <div className="card-type-bar">{label}</div>}
        <div className="card-rules">
          <p>
            <RulesText text={text} />
          </p>
        </div>
      </div>
    </div>
  );
}

/** Where on an element an arrow meets it: a side's midpoint, the center, a point given as
 * fractions of the element's width and height, or, `facing`, the point of the ellipse inscribed
 * in it that faces the center of the element at the arrow's other end. */
export type Side =
  | "left"
  | "right"
  | "top"
  | "bottom"
  | "center"
  | "facing"
  | { x: number; y: number };

/** An arrow from the element with id `from` to the one with id `to`. Given no sides, bend or
 * handles, it is routed (route.ts): out of whichever edge and into whichever edge make the
 * clearest curve past the cards `Arrows` avoids and the arrows before it, the head square to
 * the edge it meets. An element marked `data-oval` is met round its inscribed ellipse. */
export interface ArrowSpec {
  from: string;
  to: string;
  tone: "gold" | "cyan";
  /** Where it leaves and meets its ends, unrouted: by default the right side of `from` and
   * the left of `to`. */
  fromSide?: Side;
  toSide?: Side;
  /** How far the curve bows, as a fraction of its length; negative bows the other way. Without
   * it, an unrouted curve bows away from the center of the arrows' parent (`outward`). */
  bend?: number;
  /** The curve's heading out of its start and into its end, as handle vectors in pixels; given
   * both, they replace `bend`. */
  leave?: Vector;
  arrive?: Vector;
  /** What marks each end: by default a dot at the start and an arrowhead at the end. */
  start?: "dot" | "none";
  end?: "head" | "dot" | "none";
  /** A hairline, as for an attack aimed at a player. */
  fine?: boolean;
}

interface Drawn {
  key: string;
  /** The ids of the elements it ties, as its spec names them. */
  from: string;
  to: string;
  d: string;
  tone: ArrowSpec["tone"];
  start: Vector;
  end: Vector;
  angle: number;
  startMark: NonNullable<ArrowSpec["start"]>;
  endMark: NonNullable<ArrowSpec["end"]>;
  fine: boolean;
}

const SIDES = {
  left: { x: 0, y: 0.5 },
  right: { x: 1, y: 0.5 },
  top: { x: 0.5, y: 0 },
  bottom: { x: 0.5, y: 1 },
  center: { x: 0.5, y: 0.5 },
} as const;

/** An element's box in the parent's own coordinates: client rects are in screen pixels, which
 * differ by `scale` inside a scaled stage. */
function box(rect: DOMRect, origin: DOMRect, scale: number): Box {
  return {
    x: (rect.left - origin.left) / scale,
    y: (rect.top - origin.top) / scale,
    width: rect.width / scale,
    height: rect.height / scale,
  };
}

const middle = (b: Box): Vector => [b.x + b.width / 2, b.y + b.height / 2];

/** The point `side` names on `b`, where a `facing` side looks toward `other`. */
function anchor(b: Box, side: Side, other: Box): Vector {
  if (side === "facing") {
    const [cx, cy] = middle(b);
    const [ox, oy] = middle(other);
    const [dx, dy] = [ox - cx, oy - cy];
    // How many times the ray from the center to `other` reaches past the ellipse.
    const reach = Math.hypot((2 * dx) / b.width, (2 * dy) / b.height);
    return reach > 0 && Number.isFinite(reach)
      ? [cx + dx / reach, cy + dy / reach]
      : [cx, cy];
  }
  const at = typeof side === "string" ? SIDES[side] : side;
  return [b.x + b.width * at.x, b.y + b.height * at.y];
}

const point = ([x, y]: Vector) => `${x.toFixed(1)} ${y.toFixed(1)}`;

/** How far a curve bows, as a fraction of its length, when not given. */
const BOW = 0.3;

/** Which way and how fully a curve from `start` to `end` bows to stay clear of the middle of a
 * `size` parent: along its normal away from the center, fully once the chord's midpoint lies an
 * eighth of its length off the line through the center, less nearer, so that a chord through
 * the center runs straight rather than flipping from one side to the other. */
function outward(start: Vector, end: Vector, [w, h]: Vector): number {
  const [dx, dy] = [end[0] - start[0], end[1] - start[1]];
  const length = Math.hypot(dx, dy);
  if (length === 0) return 0;
  // The midpoint's offset from the center along the normal (dy, -dx), times `length`.
  const off =
    ((start[0] + end[0]) / 2 - w / 2) * dy -
    ((start[1] + end[1]) / 2 - h / 2) * dx;
  return Math.max(-1, Math.min(1, (8 * off) / length ** 2));
}

/** The curve from `start` to `end` in a `size` parent and the heading it arrives with, in
 * degrees. Its control points stay inside the parent, and so then does the curve. */
function curve(
  a: ArrowSpec,
  start: Vector,
  end: Vector,
  size: Vector,
): { d: string; angle: number } {
  const [w, h] = size;
  const inside = ([x, y]: Vector): Vector =>
    w > 0 && h > 0
      ? [Math.max(0, Math.min(w, x)), Math.max(0, Math.min(h, y))]
      : [x, y];
  const heading = (from: Vector, to: Vector) =>
    (Math.atan2(to[1] - from[1], to[0] - from[0]) * 180) / Math.PI;
  if (a.leave && a.arrive) {
    const c1 = inside([start[0] + a.leave[0], start[1] + a.leave[1]]);
    const c2 = inside([end[0] - a.arrive[0], end[1] - a.arrive[1]]);
    return {
      d: `M${point(start)} C${point(c1)} ${point(c2)} ${point(end)}`,
      angle: heading(c2, end),
    };
  }
  const [dx, dy] = [end[0] - start[0], end[1] - start[1]];
  const bend = a.bend ?? BOW * outward(start, end, size);
  // The control point sits off the chord's midpoint, along its normal.
  const c = inside([
    (start[0] + end[0]) / 2 + dy * bend,
    (start[1] + end[1]) / 2 - dx * bend,
  ]);
  return {
    d: `M${point(start)} Q${point(c)} ${point(end)}`,
    angle: heading(c, end),
  };
}

/** Whether `a` is routed rather than drawn as its sides, bend or handles say. */
const routed = (a: ArrowSpec) =>
  a.fromSide === undefined &&
  a.toSide === undefined &&
  a.bend === undefined &&
  a.leave === undefined;

/** Draws arrows between elements (by id) over its parent, which must be positioned. They are
 * measured after layout and again whenever the parent resizes, or, to `follow` elements that
 * move (cards gliding to their places), on every animation frame. Routed arrows keep off the
 * elements `avoid` selects. `scale` thins lines, heads and dots for small cards. */
export function Arrows({
  arrows,
  scale = 1,
  follow = false,
  avoid,
}: {
  arrows: ArrowSpec[];
  scale?: number;
  follow?: boolean;
  avoid?: string;
}) {
  const svg = useRef<SVGSVGElement>(null);
  const [drawn, setDrawn] = useState<Drawn[]>([]);
  // Each routed arrow's ports and reach, which it keeps while its ends or the cards it avoids
  // move: routing afresh on every frame of a glide would cost too much and could flicker.
  const choices = useRef(new Map<string, Choice>());
  // Where everything was on the last frame, and whether the arrows were routed afresh there:
  // once nothing moves, they are, once.
  const seen = useRef({ where: "", searched: false });

  // Measured against the svg's own parent: a parent's ref is not attached yet when this runs.
  useLayoutEffect(() => {
    const root = svg.current?.parentElement;
    if (!root) return;
    seen.current = { where: "", searched: false };
    const measure = () => {
      const origin = root.getBoundingClientRect();
      const scale = root.offsetWidth ? origin.width / root.offsetWidth : 1;
      const size: Vector = [root.offsetWidth, root.offsetHeight];
      const obstacles = avoid
        ? [...document.querySelectorAll(avoid)].map((e) => ({
            element: e,
            box: box(e.getBoundingClientRect(), origin, scale),
          }))
        : [];
      const ends = arrows.flatMap((a) => {
        const from = document.getElementById(a.from);
        const to = document.getElementById(a.to);
        if (!from || !to) return [];
        const source = box(from.getBoundingClientRect(), origin, scale);
        const target = box(to.getBoundingClientRect(), origin, scale);
        return [{ a, from, to, source, target }];
      });
      const where = JSON.stringify([
        size,
        ...[
          ...ends.flatMap((e) => [e.source, e.target]),
          ...obstacles.map((o) => o.box),
        ].map((b) => [b.x, b.y, b.width, b.height].map(Math.round)),
      ]);
      const still = where === seen.current.where;
      if (still && seen.current.searched) return;
      // Each routed arrow keeps off every other card, not what holds or lies within its ends.
      const legs = new Map<ArrowSpec, Leg>(
        ends
          .filter(({ a }) => routed(a))
          .map(({ a, from, to, source, target }) => [
            a,
            {
              from: { box: source, oval: from.hasAttribute("data-oval") },
              to: { box: target, oval: to.hasAttribute("data-oval") },
              avoid: obstacles
                .filter(
                  ({ element: e }) =>
                    !e.contains(from) &&
                    !from.contains(e) &&
                    !e.contains(to) &&
                    !to.contains(e),
                )
                .map((o) => o.box),
            },
          ]),
      );
      const keyOf = (a: ArrowSpec) => `${a.from}-${a.to}`;
      // While anything moves, each follows its ports; standing still, or new, all are routed.
      const traced = [...legs].map(([a, leg]) => {
        const choice = choices.current.get(keyOf(a));
        return still || !choice ? undefined : retrace(leg, choice);
      });
      const searched = traced.some((r) => !r);
      const fresh = searched ? route([...legs.values()], size) : traced;
      const routes = new Map([...legs.keys()].map((a, i) => [a, fresh[i]!]));
      seen.current = { where, searched: still || searched };
      choices.current = new Map(
        [...routes].map(([a, r]) => [keyOf(a), r.choice]),
      );
      const next = ends.map(({ a, source, target }) => {
        const key = keyOf(a);
        const marks = {
          key,
          from: a.from,
          to: a.to,
          tone: a.tone,
          startMark: a.start ?? "dot",
          endMark: a.end ?? "head",
          fine: a.fine ?? false,
        } as const;
        const r = routes.get(a);
        if (r) {
          return {
            ...marks,
            d: r.d,
            start: r.start,
            end: r.end,
            angle: r.angle,
          };
        }
        const start = anchor(source, a.fromSide ?? "right", target);
        const end = anchor(target, a.toSide ?? "left", source);
        return { ...marks, ...curve(a, start, end, size), start, end };
      });
      // Unchanged, it keeps the drawn arrows.
      setDrawn((last) =>
        last.length === next.length &&
        last.every((l, i) => l.key === next[i]!.key && l.d === next[i]!.d)
          ? last
          : next,
      );
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(root);
    let frame = 0;
    const tick = () => {
      measure();
      frame = requestAnimationFrame(tick);
    };
    if (follow && arrows.length > 0) frame = requestAnimationFrame(tick);
    return () => {
      observer.disconnect();
      cancelAnimationFrame(frame);
    };
  }, [arrows, follow, avoid]);

  return (
    <svg
      ref={svg}
      className="arrows"
      style={{ "--arrow-scale": scale } as CSSProperties}
      aria-hidden
    >
      <defs>
        <filter id="arrow-blur" x="-20%" y="-20%" width="140%" height="140%">
          <feGaussianBlur stdDeviation="3.5" />
        </filter>
      </defs>
      {drawn.map((a) => {
        const head = `translate(${point(a.end)}) rotate(${a.angle.toFixed(1)}) scale(${scale})`;
        return (
          <g
            key={a.key}
            className={`arrow arrow-${a.tone}${a.fine ? " arrow-fine" : ""}`}
            data-from={a.from}
            data-to={a.to}
          >
            {(["glow", "line", "core"] as const).map((layer) => (
              <g key={layer} className={`arrow-${layer}`}>
                <path d={a.d} />
                {a.endMark === "head" && (
                  <path
                    className={
                      a.fine ? "arrow-head" : "arrow-head arrow-head-solid"
                    }
                    d={a.fine ? FINE_HEAD : HEADS[a.tone]}
                    transform={head}
                  />
                )}
              </g>
            ))}
            {a.startMark === "dot" && <Dot at={a.start} scale={scale} />}
            {a.endMark === "dot" && <Dot at={a.end} scale={scale} />}
          </g>
        );
      })}
    </svg>
  );
}

/** A white-hot dot in its tone's bloom where an arrow leaves or lands, scaled less than lines
 * are so it stays legible on small cards. */
function Dot({ at: [x, y], scale }: { at: Vector; scale: number }) {
  const size = scale ** 0.75;
  return (
    <>
      <circle
        className="arrow-dot-glow"
        cx={x}
        cy={y}
        r={16 * size}
        filter="url(#arrow-blur)"
      />
      <circle className="arrow-dot" cx={x} cy={y} r={7 * size} />
    </>
  );
}

/** Arrowheads, each with its tip at the origin, pointing along +x: solid barbed heads, and an
 * open one for hairlines. */
const HEADS = {
  gold: "M-16 -9 L2 0 L-16 9 L-11 0 Z",
  cyan: "M-30 -17 L0 0 L-30 17 L-23 0 Z",
} as const;
const FINE_HEAD = "M-9 -5.5 L0 0 L-9 5.5";
