// Player and zones: life orbs, player badges, the library, the graveyard and stacked lands.
import type { ReactNode } from "react";
import { ASPECT, Card, CardBack, type CardFace } from "./Card";

// The life orb, drawn in docs/ui_mockups pixels about the orb's center (battlefield.png: the
// orbs at (836, 107) and (836, 703)). Two silver blades sweep out from each side of a dark oval,
// down toward the near edge for you and up toward the far edge for the opponent, over a darker
// blade and a dark arch; a gold ring underlines the oval. The active player's orb flares.

type Point = readonly [number, number];

/** A curved blade `width` thick, cut off square at its tip. */
function blade(root: Point, control: Point, tip: Point, width: number) {
  const [dx, dy] = [tip[0] - root[0], tip[1] - root[1]];
  const length = Math.hypot(dx, dy);
  const [nx, ny] = [(-dy / length) * (width / 2), (dx / length) * (width / 2)];
  const p = (x: number, y: number) => `${x.toFixed(1)} ${y.toFixed(1)}`;
  return (
    `M${p(root[0] + nx, root[1] + ny)} Q${p(control[0] + nx, control[1] + ny)} ` +
    `${p(tip[0] + nx * 0.6, tip[1] + ny * 0.6)} L${p(tip[0] - nx * 0.5, tip[1] - ny * 0.5)} ` +
    `Q${p(control[0] - nx, control[1] - ny)} ${p(root[0] - nx, root[1] - ny)} Z`
  );
}

/** One side's blades (the left; the right is mirrored), front to back. */
const BLADES = {
  you: {
    light: [
      blade([-44, -3], [-86, 3], [-125, 41], 11),
      blade([-46, 14], [-80, 19], [-107, 54], 11),
    ],
    dark: [blade([-46, 27], [-71, 35], [-95, 63], 9)],
    arch: "M-98 70 Q-64 30 0 30 Q64 30 98 70",
  },
  opponent: {
    light: [
      blade([-48, 17], [-82, 10], [-103, -14], 10),
      blade([-50, -9], [-90, -13], [-106, -48], 9),
    ],
    dark: [blade([-48, 4], [-80, -2], [-98, -24], 7)],
    arch: "",
  },
} as const;

/** The active player's rays, by angle from straight up and length from the center. */
const RAYS = [
  [-80, 94],
  [-62, 84],
  [-38, 64],
  [0, 58],
  [38, 64],
  [62, 84],
  [80, 94],
]
  .map(([deg, length]) => {
    const a = ((deg! - 90) * Math.PI) / 180;
    const point = (r: number, da = 0) =>
      `${(Math.cos(a + da) * r * 1.2).toFixed(1)} ${(Math.sin(a + da) * r).toFixed(1)}`;
    return `M${point(36, -0.06)} L${point(length! / 1.1)} L${point(36, 0.06)} Z`;
  })
  .join(" ");

/** A life total in a dark oval on silver blades; the active player's flares gold. */
export function LifeOrb({
  life,
  active = false,
  seat = "you",
}: {
  life: number;
  active?: boolean;
  /** Which way the blades sweep: down toward you, up toward the opponent. */
  seat?: "you" | "opponent";
}) {
  const blades = BLADES[seat];
  const side = (
    <>
      {blades.dark.map((d) => (
        <path key={d} className="life-orb-blade-dark" d={d} />
      ))}
      {[...blades.light].reverse().map((d) => (
        <path key={d} className="life-orb-blade" d={d} />
      ))}
    </>
  );
  return (
    <div
      className={`life-orb life-orb-${seat}${active ? " life-orb-active" : ""}`}
    >
      <svg className="life-orb-art" viewBox="-136 -57 272 116" aria-hidden>
        <defs>
          <linearGradient id="orb-blade" x1="0" y1="0" x2="0.3" y2="1">
            <stop offset="0" stopColor="#fbf5ec" />
            <stop offset="0.3" stopColor="#ddd2c4" />
            <stop offset="0.7" stopColor="#a69a8c" />
            <stop offset="1" stopColor="#5b5249" />
          </linearGradient>
          <linearGradient id="orb-rim" x1="0" y1="0" x2="0" y2="1">
            <stop offset="0" stopColor="#e9e6e1" />
            <stop offset="0.5" stopColor="#77736e" />
            <stop offset="1" stopColor="#3b3835" />
          </linearGradient>
          <radialGradient id="orb-core" cx="0.5" cy="0.32" r="0.7">
            <stop offset="0" stopColor="#3d3936" />
            <stop offset="1" stopColor="#1d1a18" />
          </radialGradient>
          <linearGradient id="orb-ray" x1="0" y1="1" x2="0" y2="0">
            <stop offset="0" stopColor="#fff0b4" />
            <stop offset="0.35" stopColor="#ff8a1c" />
            <stop offset="1" stopColor="#f2560c" stopOpacity="0" />
          </linearGradient>
          <filter id="orb-blur" x="-50%" y="-50%" width="200%" height="200%">
            <feGaussianBlur stdDeviation="3" />
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
          </g>
        )}
        {blades.arch && <path className="life-orb-arch" d={blades.arch} />}
        <g filter="url(#orb-shadow)">
          {side}
          <g transform="scale(-1 1)">{side}</g>
        </g>
        <path
          className="life-orb-ring life-orb-ring-glow"
          d="M-50 12 A51 40 0 0 0 50 12"
          filter="url(#orb-blur)"
        />
        <path className="life-orb-ring" d="M-50 12 A51 40 0 0 0 50 12" />
        {active && (
          <path
            className="life-orb-halo"
            d="M-50 -2 A50 40 0 0 1 50 -2"
            filter="url(#orb-blur)"
          />
        )}
        <ellipse
          className="life-orb-core"
          cx="0"
          cy="0"
          rx="48"
          ry="38"
          filter="url(#orb-shadow)"
        />
      </svg>
      <span className="life-orb-total">{life}</span>
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
            <stop offset="0" stopColor="#a39a92" />
            <stop offset="1" stopColor="#47413c" />
          </linearGradient>
        </defs>
        <path className="player-badge-ring" d="M0 -23 L23 0 L0 23 L-23 0 Z" />
        <path className="player-badge-ring" d="M0 -16 L16 0 L0 16 L-16 0 Z" />
        <path
          d="M0 -11 L11 0 L0 11 L-11 0 Z"
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
 * between show along that side. */
export interface PileDepth {
  x: number;
  y: number;
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
              width,
              height: width / ASPECT.full,
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

/** A graveyard, its top card face up. */
export function Graveyard({
  cards,
  width = 110,
  depth = { x: -6, y: 8 },
}: {
  cards: CardFace[];
  width?: number;
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
        <Card card={top} width={width} />
      ) : (
        <div className="pile-empty" style={{ width }} />
      )}
    </Pile>
  );
}

/** Identical lands, each copy beneath peeking out by `step` (a fraction of the card's width),
 * with a ×N count once there are several. */
export function LandStack({
  card,
  count,
  width = 150,
  tapped = 0,
  step = { x: 0.117, y: 0.034 },
  showCount = true,
}: {
  card: CardFace;
  count: number;
  width?: number;
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
