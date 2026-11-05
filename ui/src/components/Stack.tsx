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
  id?: string;
}

/** A stack item: the source's name bar over a shield-shaped rules box. */
export function StackItem({
  source,
  label,
  text,
  showArt = false,
  width = 170,
  id,
}: StackItemProps) {
  return (
    <div
      id={id}
      className={`stack-item frame-${frameKey(source)}`}
      style={{ width, fontSize: width / 12 }}
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

export type Side = "left" | "right" | "top" | "bottom" | "center";

export interface ArrowSpec {
  from: string;
  to: string;
  tone: "gold" | "cyan";
  fromSide?: Side;
  toSide?: Side;
  /** How far the curve bows, as a fraction of its length; negative bows the other way. */
  bend?: number;
  /** A plain connector with dots at both ends instead of an arrowhead. */
  link?: boolean;
}

interface Drawn {
  key: string;
  d: string;
  tone: ArrowSpec["tone"];
  start: [number, number];
  end: [number, number];
  angle: number;
  link: boolean;
}

function anchor(rect: DOMRect, origin: DOMRect, side: Side): [number, number] {
  const x = rect.left - origin.left;
  const y = rect.top - origin.top;
  switch (side) {
    case "left":
      return [x, y + rect.height / 2];
    case "right":
      return [x + rect.width, y + rect.height / 2];
    case "top":
      return [x + rect.width / 2, y];
    case "bottom":
      return [x + rect.width / 2, y + rect.height];
    case "center":
      return [x + rect.width / 2, y + rect.height / 2];
  }
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
      setDrawn(
        arrows.flatMap((a) => {
          const from = document.getElementById(a.from);
          const to = document.getElementById(a.to);
          if (!from || !to) return [];
          const start = anchor(
            from.getBoundingClientRect(),
            origin,
            a.fromSide ?? "right",
          );
          const end = anchor(
            to.getBoundingClientRect(),
            origin,
            a.toSide ?? "left",
          );
          const [dx, dy] = [end[0] - start[0], end[1] - start[1]];
          const bend = a.bend ?? 0.25;
          // The control point sits off the chord's midpoint, along its normal.
          const c: [number, number] = [
            (start[0] + end[0]) / 2 + dy * bend,
            (start[1] + end[1]) / 2 - dx * bend,
          ];
          return [
            {
              key: `${a.from}-${a.to}`,
              d: `M${start[0]} ${start[1]} Q${c[0]} ${c[1]} ${end[0]} ${end[1]}`,
              tone: a.tone,
              start,
              end,
              angle: (Math.atan2(end[1] - c[1], end[0] - c[0]) * 180) / Math.PI,
              link: a.link ?? false,
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
      {drawn.map((a) => (
        <g key={a.key} className={`arrow arrow-${a.tone}`}>
          <path className="arrow-glow" d={a.d} />
          <path className="arrow-line" d={a.d} />
          <path className="arrow-core" d={a.d} />
          <circle className="arrow-dot" cx={a.start[0]} cy={a.start[1]} r="5" />
          {a.link ? (
            <circle className="arrow-dot" cx={a.end[0]} cy={a.end[1]} r="5" />
          ) : (
            <path
              className="arrow-head"
              d="M0 0 L-13 -7 L-9 0 L-13 7 Z"
              transform={`translate(${a.end[0]} ${a.end[1]}) rotate(${a.angle})`}
            />
          )}
        </g>
      ))}
    </svg>
  );
}
