// The stack-item frame (a spell or ability on the stack) and the arrows that tie it to its source
// (gold) and targets (cyan).
import { useLayoutEffect, useRef, useState } from "react";
import { CardArt, frameKey, type CardFace } from "./Card";
import { RulesText } from "./Mana";

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
  return (
    <div
      id={id}
      className={`stack-item frame-${frameKey(source)}${showArt ? "" : " stack-item-compact"}`}
      style={{ width, height, fontSize: unit ?? width / 12 }}
    >
      <div className="card-rim">
        <div className="card-name-bar">
          <span className="card-name">{source.name}</span>
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

/** Where on an element an arrow meets it: a side's midpoint, the center, or a point given as
 * fractions of the element's width and height. */
export type Side =
  "left" | "right" | "top" | "bottom" | "center" | { x: number; y: number };

type Vector = readonly [number, number];

export interface ArrowSpec {
  from: string;
  to: string;
  tone: "gold" | "cyan";
  fromSide?: Side;
  toSide?: Side;
  /** How far the curve bows, as a fraction of its length; negative bows the other way. */
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

/** A point on `rect` in the parent's own coordinates: client rects are in screen pixels, which
 * differ by `scale` inside a scaled stage. */
function anchor(
  rect: DOMRect,
  origin: DOMRect,
  scale: number,
  side: Side,
): Vector {
  const at = typeof side === "string" ? SIDES[side] : side;
  return [
    (rect.left - origin.left + rect.width * at.x) / scale,
    (rect.top - origin.top + rect.height * at.y) / scale,
  ];
}

const point = ([x, y]: Vector) => `${x.toFixed(1)} ${y.toFixed(1)}`;

/** The curve from `start` to `end` and the heading it arrives with, in degrees. */
function curve(
  a: ArrowSpec,
  start: Vector,
  end: Vector,
): { d: string; angle: number } {
  const heading = (from: Vector, to: Vector) =>
    (Math.atan2(to[1] - from[1], to[0] - from[0]) * 180) / Math.PI;
  if (a.leave && a.arrive) {
    const c1: Vector = [start[0] + a.leave[0], start[1] + a.leave[1]];
    const c2: Vector = [end[0] - a.arrive[0], end[1] - a.arrive[1]];
    return {
      d: `M${point(start)} C${point(c1)} ${point(c2)} ${point(end)}`,
      angle: heading(c2, end),
    };
  }
  const [dx, dy] = [end[0] - start[0], end[1] - start[1]];
  const bend = a.bend ?? 0.25;
  // The control point sits off the chord's midpoint, along its normal.
  const c: Vector = [
    (start[0] + end[0]) / 2 + dy * bend,
    (start[1] + end[1]) / 2 - dx * bend,
  ];
  return {
    d: `M${point(start)} Q${point(c)} ${point(end)}`,
    angle: heading(c, end),
  };
}

/** Draws arrows between elements (by id) over its parent, which must be positioned. They are
 * measured after layout and again whenever the parent resizes. */
export function Arrows({ arrows }: { arrows: ArrowSpec[] }) {
  const svg = useRef<SVGSVGElement>(null);
  const [drawn, setDrawn] = useState<Drawn[]>([]);

  // Measured against the svg's own parent: a parent's ref is not attached yet when this runs.
  useLayoutEffect(() => {
    const root = svg.current?.parentElement;
    if (!root) return;
    const measure = () => {
      const origin = root.getBoundingClientRect();
      const scale = root.offsetWidth ? origin.width / root.offsetWidth : 1;
      setDrawn(
        arrows.flatMap((a) => {
          const from = document.getElementById(a.from);
          const to = document.getElementById(a.to);
          if (!from || !to) return [];
          const start = anchor(
            from.getBoundingClientRect(),
            origin,
            scale,
            a.fromSide ?? "right",
          );
          const end = anchor(
            to.getBoundingClientRect(),
            origin,
            scale,
            a.toSide ?? "left",
          );
          return [
            {
              key: `${a.from}-${a.to}`,
              ...curve(a, start, end),
              tone: a.tone,
              start,
              end,
              startMark: a.start ?? "dot",
              endMark: a.end ?? "head",
              fine: a.fine ?? false,
            },
          ];
        }),
      );
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(root);
    return () => observer.disconnect();
  }, [arrows]);

  return (
    <svg ref={svg} className="arrows" aria-hidden>
      <defs>
        <filter id="arrow-blur" x="-20%" y="-20%" width="140%" height="140%">
          <feGaussianBlur stdDeviation="3.5" />
        </filter>
      </defs>
      {drawn.map((a) => {
        const head = `translate(${point(a.end)}) rotate(${a.angle.toFixed(1)})`;
        return (
          <g
            key={a.key}
            className={`arrow arrow-${a.tone}${a.fine ? " arrow-fine" : ""}`}
          >
            {(["glow", "line", "core"] as const).map((layer) => (
              <g key={layer} className={`arrow-${layer}`}>
                <path d={a.d} />
                {a.endMark === "head" && (
                  <path
                    className={`arrow-head arrow-head-${a.tone}`}
                    d={a.fine ? FINE_HEAD : HEADS[a.tone]}
                    transform={head}
                  />
                )}
              </g>
            ))}
            {a.startMark === "dot" && <Dot at={a.start} />}
            {a.endMark === "dot" && <Dot at={a.end} />}
          </g>
        );
      })}
    </svg>
  );
}

/** A bright dot in its own glow, where an arrow leaves or lands. */
function Dot({ at: [x, y] }: { at: Vector }) {
  return (
    <>
      <circle
        className="arrow-dot-glow"
        cx={x}
        cy={y}
        r="9"
        filter="url(#arrow-blur)"
      />
      <circle className="arrow-dot" cx={x} cy={y} />
    </>
  );
}

/** An open arrowhead, its tip at the origin, pointing along +x. */
const HEADS = {
  /** A filled tip. */
  gold: "M-17 -9 L2 0 L-17 9 Z",
  /** Two nested chevrons. */
  cyan: "M-26 -17 L0 0 L-26 17 M-38 -17 L-12 0 L-38 17",
} as const;
const FINE_HEAD = "M-9 -5.5 L0 0 L-9 5.5";
