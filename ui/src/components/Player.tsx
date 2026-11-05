// Player and zones: life orbs, player badges, the library, the graveyard and stacked lands.
import type { ReactNode } from "react";
import { ASPECT, Card, CardBack, type CardFace } from "./Card";

// The life orb, drawn in docs/ui_mockups pixels about the orb's center (battlefield.png: the
// orbs at (836, 107) and (836, 703)). Two silver blades sweep out from each side of a dark oval,
// down toward the near edge for you and up toward the far edge for the opponent, over a darker
// blade and a dark arch; a gold ring underlines the oval. The active player's orb flares.

type Point = readonly [number, number];

/** A curved blade `width` thick at its root, its tip cut square (`tip` = 1) or drawn to a point
 * (`tip` = 0). */
function blade(
  root: Point,
  control: Point,
  tip: Point,
  width: number,
  end = 1,
) {
  const [dx, dy] = [tip[0] - root[0], tip[1] - root[1]];
  const length = Math.hypot(dx, dy);
  const [nx, ny] = [(-dy / length) * (width / 2), (dx / length) * (width / 2)];
  const p = (x: number, y: number) => `${x.toFixed(1)} ${y.toFixed(1)}`;
  return (
    `M${p(root[0] + nx, root[1] + ny)} Q${p(control[0] + nx, control[1] + ny)} ` +
    `${p(tip[0] + nx * 0.6 * end, tip[1] + ny * 0.6 * end)} ` +
    `L${p(tip[0] - nx * 0.5 * end, tip[1] - ny * 0.5 * end)} ` +
    `Q${p(control[0] - nx, control[1] - ny)} ${p(root[0] - nx, root[1] - ny)} Z`
  );
}

/** How a light blade is shaded, root to tip: the mockups paint the near orb's blades warm tan,
 * the far orb's upper blade grey silver and its lower one a pale wing, each coppery at its root. */
const TONES = {
  tan: ["#7a6450", "#958877", "#b3aa9f"],
  pale: ["#8c765c", "#ada290", "#c6bfb3"],
  silver: ["#8d5b3a", "#77706a", "#4e4a45"],
  wing: ["#b0703e", "#c3a780", "#d8c8aa"],
} as const;
type Tone = keyof typeof TONES;

/** One side's blades (the left; the right is mirrored): steel struts behind light blades, and
 * copper glints where the blades meet the oval. Each blade runs from its root at the oval to its
 * tip; `from` is the corner of its box its root is in, where its tone starts. */
const BLADES: Record<
  "you" | "opponent",
  {
    light: { d: string; tone: Tone }[];
    dark: string[];
    from: [number, number];
    glints: Point[];
    arch: string;
    ring: string;
  }
> = {
  you: {
    light: [
      { d: blade([-46, -5], [-96, 1], [-124, 37], 9.5), tone: "tan" },
      { d: blade([-42, 13], [-78, 14], [-109, 48], 9), tone: "pale" },
    ],
    dark: [
      blade([-45, 8], [-84, 14], [-114, 47], 5),
      blade([-46, 25], [-71, 33], [-95, 63], 5),
    ],
    from: [1, 0],
    glints: [],
    arch: "M-98 70 Q-64 30 0 30 Q64 30 98 70",
    ring: "M-44 16 A49 41 0 0 0 44 16",
  },
  opponent: {
    light: [
      { d: blade([-46, 19], [-78, 10], [-103, -14], 12, 0.75), tone: "wing" },
      {
        d: blade([-48, -8], [-70, -12], [-96, -35], 11, 0.3),
        tone: "silver",
      },
    ],
    dark: [
      blade([-50, -6], [-67, -7], [-84, -8], 5),
      blade([-47, 4], [-60, 2], [-74, -1], 5),
    ],
    from: [1, 1],
    glints: [
      [-48, -9],
      [-47, 17],
    ],
    arch: "",
    ring: "M-46 19 A52.5 40.5 0 0 0 46 19",
  },
};

/** The core: a cushion, a rounded oval with flattened sides, `rx` by `ry` about (0, `cy`) to
 * the middle of its rim; the total centers `total` below the orb's center. */
const CORE = {
  you: { rx: 49, ry: 37, cy: -3, total: -6 },
  opponent: { rx: 48.5, ry: 36, cy: 0, total: 0 },
};

/** A superellipse (|x/rx|^n + |y/ry|^n = 1) about (0, cy), as a path. */
function cushion({ rx, ry, cy }: { rx: number; ry: number; cy: number }) {
  const n = 2.25;
  const points = Array.from({ length: 72 }, (_, i) => {
    const t = (i / 72) * 2 * Math.PI;
    const [c, s] = [Math.cos(t), Math.sin(t)];
    const x = rx * Math.sign(c) * Math.abs(c) ** (2 / n);
    const y = cy + ry * Math.sign(s) * Math.abs(s) ** (2 / n);
    return `${x.toFixed(1)} ${y.toFixed(1)}`;
  });
  return `M${points.join(" L")} Z`;
}

/** The active player's rays, by where each tip lies (the left half; mirrored on the right):
 * each widens from the rim of the oval to its tip. */
const RAYS = [
  [-44, -50],
  [-66, -44],
  [-84, -32],
  [-100, -16],
  [-70, -6],
]
  .flatMap(([x, y]) => [
    [x!, y!],
    [-x!, y!],
  ])
  .map(([x, y]) => {
    const a = Math.atan2(y!, x! / 1.2);
    const rim = (da: number) =>
      `${(Math.cos(a + da) * 36 * 1.2).toFixed(1)} ${(Math.sin(a + da) * 36).toFixed(1)}`;
    return `M${rim(-0.06)} L${x} ${y} L${rim(0.06)} Z`;
  })
  .join(" ");

/** A life total in a dark oval on silver blades; the active player's flares gold. */
export function LifeOrb({
  life,
  active = false,
  seat = "you",
  wings = 1,
  anchor,
}: {
  life: number;
  active?: boolean;
  /** Which way the blades sweep: down toward you, up toward the opponent. */
  seat?: "you" | "opponent";
  /** How far the blades reach, as a fraction of their full span: shorter where space is
   * tight. */
  wings?: number;
  /** The element id of the oval, which arrows meet. */
  anchor?: string;
}) {
  const blades = BLADES[seat];
  const core = CORE[seat];
  const [fx, fy] = blades.from;
  const side = (
    <>
      {blades.dark.map((d) => (
        <path key={d} className="life-orb-blade-dark" d={d} />
      ))}
      {blades.glints.map(([x, y]) => (
        <ellipse
          key={`${x} ${y}`}
          className="life-orb-glint"
          cx={x}
          cy={y}
          rx="7"
          ry="5"
          filter="url(#orb-blur)"
        />
      ))}
      {[...blades.light].reverse().map(({ d, tone }) => (
        <path
          key={d}
          className="life-orb-blade"
          d={d}
          fill={`url(#orb-blade-${seat}-${tone})`}
          filter={seat === "you" ? "url(#orb-bevel)" : undefined}
        />
      ))}
    </>
  );
  return (
    <div
      className={`life-orb life-orb-${seat}${active ? " life-orb-active" : ""}`}
    >
      <svg className="life-orb-art" viewBox="-136 -57 272 116" aria-hidden>
        <defs>
          {Object.entries(TONES).map(([tone, [root, mid, tip]]) => (
            <linearGradient
              key={tone}
              id={`orb-blade-${seat}-${tone}`}
              x1={fx}
              y1={fy}
              x2={1 - fx}
              y2={1 - fy}
            >
              <stop offset="0" stopColor={root} />
              <stop offset="0.45" stopColor={mid} />
              <stop offset="1" stopColor={tip} />
            </linearGradient>
          ))}
          <linearGradient id="orb-rim" x1="0" y1="0" x2="0" y2="1">
            <stop offset="0" stopColor="#dcd8d2" />
            <stop offset="0.2" stopColor="#8e8984" />
            <stop offset="0.6" stopColor="#4a4643" />
            <stop offset="1" stopColor="#3a3633" />
          </linearGradient>
          <radialGradient id="orb-core" cx="0.5" cy="0.32" r="0.7">
            <stop offset="0" stopColor="#3d3936" />
            <stop offset="1" stopColor="#1d1a18" />
          </radialGradient>
          <radialGradient
            id="orb-ray"
            cx="0"
            cy="0"
            r="120"
            gradientUnits="userSpaceOnUse"
          >
            <stop offset="0.3" stopColor="#fff3c0" />
            <stop offset="0.42" stopColor="#ffb030" />
            <stop offset="0.6" stopColor="#f05a10" />
            <stop offset="1" stopColor="#c83008" stopOpacity="0" />
          </radialGradient>
          <filter id="orb-blur" x="-50%" y="-50%" width="200%" height="200%">
            <feGaussianBlur stdDeviation="3" />
          </filter>
          {/* Metal catching the light along its upper edges. */}
          <filter id="orb-bevel" x="-10%" y="-10%" width="120%" height="120%">
            <feGaussianBlur in="SourceAlpha" stdDeviation="1.2" result="b" />
            <feSpecularLighting
              in="b"
              surfaceScale="3"
              specularConstant="0.9"
              specularExponent="16"
              lightingColor="#ffffff"
              result="s"
            >
              <feDistantLight azimuth="270" elevation="40" />
            </feSpecularLighting>
            <feComposite in="s" in2="SourceAlpha" operator="in" result="lit" />
            <feComposite
              in="SourceGraphic"
              in2="lit"
              operator="arithmetic"
              k2="1"
              k3="0.45"
            />
          </filter>
          <filter id="orb-shadow" x="-20%" y="-20%" width="140%" height="160%">
            <feDropShadow dx="0" dy="2" stdDeviation="1.5" floodOpacity="0.6" />
          </filter>
        </defs>
        {active && (
          <g className="life-orb-flare">
            <ellipse cx="0" cy="-2" rx="62" ry="50" filter="url(#orb-blur)" />
            <path d={RAYS} filter="url(#orb-blur)" />
            <path d={RAYS} fill="url(#orb-ray)" />
            {/* White-hot flares where the rim meets the blades, in a golden haze. */}
            {[-1, 1].map((side) => (
              <g key={side} filter="url(#orb-blur)">
                <ellipse
                  cx={side * 66}
                  cy="-12"
                  rx="26"
                  ry="7"
                  fill="#ffb028"
                />
                <ellipse
                  cx={side * 60}
                  cy="-12"
                  rx="16"
                  ry="2.5"
                  fill="#fff6d0"
                />
              </g>
            ))}
          </g>
        )}
        {blades.arch && <path className="life-orb-arch" d={blades.arch} />}
        <g filter="url(#orb-shadow)" transform={`scale(${wings})`}>
          {side}
          <g transform="scale(-1 1)">{side}</g>
        </g>

        {active && (
          <path
            className="life-orb-halo"
            d="M-42 -17 A47 37 0 0 1 42 -17"
            filter="url(#orb-blur)"
          />
        )}
        <path
          id={anchor}
          className="life-orb-core"
          d={cushion(core)}
          filter="url(#orb-shadow)"
        />
        <path
          className="life-orb-ring life-orb-ring-glow"
          d={blades.ring}
          filter="url(#orb-blur)"
        />
        <path className="life-orb-ring" d={blades.ring} />
        {active && (
          <ellipse
            className="life-orb-bloom"
            cx="0"
            cy={core.cy - core.ry}
            rx="30"
            ry="8"
            filter="url(#orb-blur)"
          />
        )}
        {active && (
          <path
            className="life-orb-crescent"
            d="M-40 -24 A47 37 0 0 1 40 -24"
          />
        )}
      </svg>
      <span
        className="life-orb-total"
        style={{ marginTop: `calc(${core.total} * var(--orb))` }}
      >
        {life}
      </span>
    </div>
  );
}

/** A player's name on a dark band behind a nested brass diamond. */
export function PlayerBadge({ name }: { name: string }) {
  return (
    <div className="player-badge">
      <svg className="player-badge-emblem" viewBox="-24 -24 48 48" aria-hidden>
        <defs>
          <linearGradient id="emblem-brass" x1="0" y1="0" x2="1" y2="1">
            <stop offset="0" stopColor="#e4cd9c" />
            <stop offset="0.5" stopColor="#ad9168" />
            <stop offset="1" stopColor="#7a6244" />
          </linearGradient>
          <linearGradient id="emblem-stone" x1="0" y1="0" x2="1" y2="1">
            <stop offset="0" stopColor="#857b72" />
            <stop offset="1" stopColor="#35302b" />
          </linearGradient>
        </defs>
        <path className="player-badge-ring" d="M0 -23 L23 0 L0 23 L-23 0 Z" />
        <path className="player-badge-ring" d="M0 -16 L16 0 L0 16 L-16 0 Z" />
        <path
          d="M0 -9 L9 0 L0 9 L-9 0 Z"
          fill="url(#emblem-stone)"
          stroke="url(#emblem-brass)"
          strokeWidth="1.3"
        />
      </svg>
      <span>{name}</span>
    </div>
  );
}

/** Where a pile's bottom card lies relative to its top card, in pixels: the edges of the cards
 * between show along that side. Seen in perspective, the bottom card may also look `flare`
 * pixels wider (growing to the right) and turned `twist` degrees. */
export interface PileDepth {
  x: number;
  y: number;
  flare?: number;
  twist?: number;
}

/** A pile of cards: the top one over the edges of those beneath, one edge per few cards. */
function Pile({
  count,
  depth,
  width,
  children,
  label,
  className,
}: {
  count: number;
  depth: PileDepth;
  width: number;
  children: ReactNode;
  label: string;
  className?: string;
}) {
  const edges = Math.min(12, Math.ceil(count / 3));
  const left = Math.max(0, -depth.x);
  return (
    <div
      className={["pile", className].filter(Boolean).join(" ")}
      style={{
        paddingLeft: left,
        paddingRight: Math.max(0, depth.x),
        paddingBottom: Math.max(0, depth.y),
      }}
      aria-label={label}
    >
      {Array.from({ length: edges }, (_, i) => {
        const k = (edges - i) / edges; // deepest first
        return (
          <div
            key={i}
            className="pile-edge"
            style={{
              left: left + depth.x * k,
              top: depth.y * k,
              width: width + (depth.flare ?? 0) * k,
              height: width / ASPECT.full,
              rotate: `${(depth.twist ?? 0) * k}deg`,
            }}
          />
        );
      })}
      {children}
    </div>
  );
}

/** A face-down library, as thick as it is. */
export function Library({
  count,
  width = 110,
  depth = { x: -10, y: 24 },
}: {
  count: number;
  width?: number;
  depth?: PileDepth;
}) {
  return (
    <Pile
      count={count}
      depth={depth}
      width={width}
      label={`Library, ${count} cards`}
    >
      {count > 0 && <CardBack width={width} />}
    </Pile>
  );
}

/** Where a graveyard's bottom card lies relative to its top card. */
export const GRAVEYARD_DEPTH: PileDepth = { x: -6, y: 8 };

/** A graveyard, its top card face up. */
export function Graveyard({
  cards,
  width = 110,
  height,
  depth = GRAVEYARD_DEPTH,
}: {
  cards: CardFace[];
  width?: number;
  /** The top card's height, when not the full frame's usual aspect. */
  height?: number;
  depth?: PileDepth;
}) {
  const top = cards[cards.length - 1];
  return (
    <Pile
      count={Math.max(0, cards.length - 1) * 3}
      depth={depth}
      width={width}
      label={`Graveyard, ${cards.length} cards`}
      className="graveyard"
    >
      {top ? (
        <Card card={top} width={width} height={height} />
      ) : (
        <div className="pile-empty" style={{ width }} />
      )}
    </Pile>
  );
}

/** The edges beneath the top card of a graveyard of `count` cards, for a top card drawn by
 * someone else: the game screen draws every card itself, so it can move. The top card's place
 * is `-depth.x` pixels in from the left. */
export function GraveyardEdges({
  count,
  width,
  depth = GRAVEYARD_DEPTH,
}: {
  count: number;
  width: number;
  depth?: PileDepth;
}) {
  return (
    <Pile
      count={Math.max(0, count - 1) * 3}
      depth={depth}
      width={width}
      label={`Graveyard, ${count} cards`}
      className="graveyard"
    >
      <div style={{ width, height: width / ASPECT.full }} />
    </Pile>
  );
}

/** Identical lands, each copy beneath peeking out by `step` (a fraction of the card's width),
 * with a ×N count once there are several. */
export function LandStack({
  card,
  count,
  width = 150,
  height,
  tapped = 0,
  step = { x: 0.117, y: 0.034 },
  showCount = true,
}: {
  card: CardFace;
  count: number;
  width?: number;
  /** The cards' height, when not the field frame's usual aspect. */
  height?: number;
  tapped?: number;
  step?: { x: number; y: number };
  /** The ×N badge; on the table the edges of the copies beneath show the count instead. */
  showCount?: boolean;
}) {
  const [dx, dy] = [width * step.x, width * step.y];
  return (
    <div
      className="land-stack"
      style={{
        paddingLeft: (count - 1) * dx,
        paddingBottom: (count - 1) * dy,
      }}
    >
      {Array.from({ length: count }, (_, i) => {
        const depth = count - 1 - i;
        return (
          <Card
            key={i}
            card={card}
            variant="field"
            width={width}
            height={height}
            tapped={i >= count - tapped}
            className={depth > 0 ? "card-under" : undefined}
            style={
              depth > 0
                ? {
                    position: "absolute",
                    left: i * dx,
                    top: depth * dy,
                  }
                : undefined
            }
          />
        );
      })}
      {showCount && count > 1 && <span className="land-count">×{count}</span>}
    </div>
  );
}
