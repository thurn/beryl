// The stack-item frame (a spell or ability on the stack) and the arrows that tie it to its source
// (gold) and targets (red or green).
import { useLayoutEffect, useRef, useState, type CSSProperties } from "react";
import {
  AbilityPlate,
  CardArt,
  CARD_ASPECT,
  frameKey,
  type CardFace,
} from "./Card";
import { useTextFit } from "./fit";
import { RulesText } from "./Mana";
import type { PaidCost } from "../generated/protocol";
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

/** A stack item: with art, its source's scan wearing a plate that says the item; without it,
 * or until the scan loads, the source's name bar over a shield-shaped rules box. */
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
  if (showArt && source.scan) {
    return (
      <div
        id={id}
        className="stack-item stack-item-scanned"
        style={{ width, height: width / CARD_ASPECT, fontSize: width / 12 }}
      >
        <img className="card-scan" src={source.scan} alt={source.name} />
        <AbilityPlate label={label} text={text} />
      </div>
    );
  }
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

/** The keyword of each cost a spell can be cast with, as its card prints it. */
const PAID: Record<PaidCost, string> = {
  kicker: "Kicker",
  offspring: "Offspring",
  impending: "Impending",
  overload: "Overload",
  evoke: "Evoke",
  flashback: "Flashback",
};

/** What a stack item's frame cannot say, on its edges: whose it is, named for assistive
 * technology only (a corner mark in each player's colour said nothing a player could read), the
 * X it was announced with, spell or ability alike, and
 * the alternative or additional costs a spell was cast with, as Arena tags them, since its
 * printed text does not say what they change (an overloaded spell targets nothing). An X of 0
 * goes unmarked, as for an item without X. */
export function StackMarks({
  controller,
  x,
  paid = [],
}: {
  controller: "you" | "opponent";
  x: number;
  paid?: PaidCost[];
}) {
  return (
    <>
      <span
        className={`stack-controller stack-controller-${controller}`}
        data-controller={controller}
        role="img"
        aria-label={controller === "you" ? "Yours" : "Opponent's"}
      />
      {x > 0 && (
        <span className="stack-x" aria-label={`X is ${x}`}>
          X={x}
        </span>
      )}
      {paid.length > 0 && (
        <span className="stack-paid" data-paid={paid.join(" ")}>
          {paid.map((p) => PAID[p]).join(" · ")}
        </span>
      )}
    </>
  );
}

/** An arrow from the element with id `from` to the one with id `to`, routed (route.ts): out of
 * whichever edge and into whichever edge make the clearest curve past the cards `Arrows` avoids
 * and the arrows before it, the head square to the edge it meets. An element marked
 * `data-oval` is met round its inscribed ellipse. One marked `data-within` with another's id
 * is a part of that one (the strip of a stack item that peeks out from under the next): the
 * arrow keeps off neither. Every arrow looks alike; only `tone` tells
 * a kind apart: gold for a source, red for an attack, red or green for a target. */
export interface ArrowSpec {
  from: string;
  to: string;
  tone: "gold" | "cyan" | "red" | "green";
  /** A subtle direct combat line between facing edges, without an arrowhead. */
  direct?: boolean;
  /** A target link shown while both ends are settled, invalidated by a zone change. */
  pulse?: {
    key: string;
    zone: string | undefined;
    departed: boolean;
  };
}

type Drawn = {
  key: string;
  /** The ids of the elements it ties, as its spec names them. */
  from: string;
  to: string;
  d: string;
  tone: ArrowSpec["tone"];
} & ({ direct: true } | { direct: false; end: Vector; angle: number });

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

const point = ([x, y]: Vector) => `${x.toFixed(1)} ${y.toFixed(1)}`;

/** Draws arrows between elements (by id) over its parent, which must be positioned, above
 * everything in it. They are measured after layout and again whenever the parent resizes, or,
 * to `follow` elements that move (cards gliding to their places), on every animation frame.
 * They keep off the elements `avoid` selects. `scale` thins lines and heads for small cards. */
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
  // Each arrow's ports and reach, which it keeps while its ends or the cards it avoids move:
  // routing afresh on every frame of a glide would cost too much and could flicker.
  const choices = useRef(new Map<string, Choice>());
  // Where everything was on the last frame, and whether the arrows were routed afresh there:
  // once nothing moves, they are, once.
  const seen = useRef({ where: "", searched: false });
  // Following, where each arrow's ends were and since when they have stood there: an arrow
  // shows only once both its ends have stood still for `REST_MS`, so it never stretches after
  // a card on its way (a countered spell to the graveyard, a creature still landing when its
  // trigger goes on the stack), and it goes the moment either end starts moving.
  const rest = useRef(new Map<string, { ends: string; since: number }>());
  const pulses = useRef(
    new Map<string, { zone: string | undefined; until?: number }>(),
  );

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
      const now = performance.now();
      const keyOf = (a: ArrowSpec) => `${a.from}-${a.to}`;
      const pulsing = new Map<
        string,
        { zone: string | undefined; until?: number }
      >();
      const resting = new Map<string, { ends: string; since: number }>();
      const ends = arrows.flatMap((a) => {
        const pulse =
          a.pulse &&
          (pulses.current.get(a.pulse.key) ?? { zone: a.pulse.zone });
        if (a.pulse && pulse) {
          pulsing.set(a.pulse.key, pulse);
          if (a.pulse.departed || pulse.zone !== a.pulse.zone)
            pulse.until = now;
          if (pulse.until !== undefined && pulse.until <= now) return [];
        }
        const from = document.getElementById(a.from);
        const to = document.getElementById(a.to);
        if (!from || !to) return [];
        const source = box(from.getBoundingClientRect(), origin, scale);
        const target = box(to.getBoundingClientRect(), origin, scale);
        if (follow) {
          const key = keyOf(a);
          // A stack peek is laid out at its destination while its card is still flying.
          const hosts = [from, to].flatMap((end) => {
            const id = end.dataset.within;
            const host = id && document.getElementById(id);
            return host
              ? [box(host.getBoundingClientRect(), origin, scale)]
              : [];
          });
          const at = JSON.stringify(
            [source, target, ...hosts].map((b) => [
              b.x,
              b.y,
              b.width,
              b.height,
            ]),
          );
          const was = rest.current.get(key);
          const since = was?.ends === at ? was.since : now;
          resting.set(key, { ends: at, since });
          if (now - since < REST_MS) {
            return [];
          }
          if (pulse && pulse.until === undefined) {
            pulse.until = Number.POSITIVE_INFINITY;
          }
        }
        return [{ a, from, to, source, target }];
      });
      if (follow) rest.current = resting;
      pulses.current = pulsing;
      const where = JSON.stringify([
        size,
        ends.map((e) => [keyOf(e.a), e.a.tone]),
        ...[
          ...ends.flatMap((e) => [e.source, e.target]),
          ...obstacles.map((o) => o.box),
        ].map((b) => [b.x, b.y, b.width, b.height].map(Math.round)),
      ]);
      const still = where === seen.current.where;
      if (still && seen.current.searched) return;
      // Each arrow keeps off every other card, not what holds or lies within its ends, or
      // within what its ends are part of.
      const routed = ends.filter(({ a }) => !a.direct);
      const legs = routed.map(({ from, to, source, target }): Leg => {
        const own = [from, to].flatMap((end) => {
          const host = end.dataset.within;
          const whole = host ? document.getElementById(host) : null;
          return whole ? [end, whole] : [end];
        });
        return {
          from: { box: source, oval: from.hasAttribute("data-oval") },
          to: { box: target, oval: to.hasAttribute("data-oval") },
          avoid: obstacles
            .filter(
              ({ element: e }) =>
                !own.some((end) => e.contains(end) || end.contains(e)),
            )
            .map((o) => o.box),
        };
      });
      // While anything moves, each follows its ports; standing still, or new, all are routed.
      const traced = routed.map(({ a }, i) => {
        const choice = choices.current.get(keyOf(a));
        return still || !choice ? undefined : retrace(legs[i]!, choice);
      });
      const searched = traced.some((r) => !r);
      const routes = searched
        ? route(
            legs,
            size,
            routed.map(({ a }) => choices.current.get(keyOf(a))),
          )
        : traced;
      seen.current = { where, searched: still || searched };
      choices.current = new Map(
        routed.map(({ a }, i) => [keyOf(a), routes[i]!.choice]),
      );
      const next = routed.map(({ a }, i): Drawn => {
        const r = routes[i]!;
        return {
          key: keyOf(a),
          from: a.from,
          to: a.to,
          tone: a.tone,
          d: shaft(r.p, r.angle, scale),
          end: r.end,
          angle: r.angle,
          direct: false,
        };
      });
      for (const { a, source, target } of ends) {
        if (!a.direct) continue;
        const above = source.y < target.y;
        const start: Vector = [
          source.x + source.width / 2,
          source.y + (above ? source.height : 0),
        ];
        const end: Vector = [
          target.x + target.width / 2,
          target.y + (above ? 0 : target.height),
        ];
        next.push({
          ...a,
          key: keyOf(a),
          d: `M${point(start)} L${point(end)}`,
          direct: true,
        });
      }
      // Unchanged, it keeps the drawn arrows.
      setDrawn((last) =>
        last.length === next.length &&
        last.every(
          (l, i) =>
            l.key === next[i]!.key &&
            l.d === next[i]!.d &&
            l.tone === next[i]!.tone,
        )
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
        if (a.direct) {
          return (
            <g
              key={a.key}
              className="arrow combat-link"
              data-from={a.from}
              data-to={a.to}
            >
              <g className="arrow-line">
                <path d={a.d} />
              </g>
            </g>
          );
        }
        const head = `translate(${point(a.end)}) rotate(${a.angle.toFixed(1)}) scale(${scale})`;
        return (
          <g
            key={a.key}
            className={`arrow arrow-${a.tone}`}
            data-from={a.from}
            data-to={a.to}
          >
            {(["glow", "line", "core"] as const).map((layer) => (
              <g key={layer} className={`arrow-${layer}`}>
                <path d={a.d} />
                <path
                  className="arrow-head"
                  d={layer === "core" ? HEAD_CORE : HEAD}
                  transform={head}
                />
              </g>
            ))}
          </g>
        );
      })}
    </svg>
  );
}

/** How long both ends of a followed arrow stand still before it shows, in ms. */
const REST_MS = 90;
/** A settled target link gets one brief moment of emphasis. */

/** The arrowhead, its tip at the origin, pointing along +x: a solid barbed head, three and a
 * half tubes wide, whose base the shaft runs into. */
const HEAD = "M-25 -15 L0 0 L-25 15 L-19 0 Z";
/** The head's core, the shaft's core carried on to near the tip: one tone, as the tube's. */
const HEAD_CORE = "M-22 -11 L-5 0 L-22 11 L-17 0 Z";
/** How far short of the tip the shaft stops: inside the head, past its notch, so its round cap
 * is hidden in the head and the two read as one piece. */
const SHAFT_SHORT = 15;

/** An arrow's shaft: its cubic `p` stopped `SHAFT_SHORT` short of the tip along its heading
 * `angle` (degrees), both end and last control point, so it arrives square in the head. */
function shaft(p: readonly Vector[], angle: number, scale: number): string {
  const [p0, p1, p2, p3] = p as [Vector, Vector, Vector, Vector];
  const r = (angle * Math.PI) / 180;
  const back: Vector = [
    -Math.cos(r) * SHAFT_SHORT * scale,
    -Math.sin(r) * SHAFT_SHORT * scale,
  ];
  const move = ([x, y]: Vector): Vector => [x + back[0], y + back[1]];
  return `M${point(p0)} C${point(p1)} ${point(move(p2))} ${point(move(p3))}`;
}
