// Player and zones: life orbs, mana pools, player badges, emblems, the library, the graveyard
// and stacked lands.
import { useEffect, useState, type ReactNode } from "react";
import { useCardArt } from "../cards";
import { CARD_ASPECT, Card, CardArt, CardBack, type CardFace } from "./Card";
import { Tip } from "./Controls";
import { ManaSymbol } from "./Mana";

// The life orb, drawn in docs/ui_mockups pixels about the orb's center: a dark oval with a gold
// ring underlining it toward its player's edge of the table. The opponent's orb is yours mirrored
// top to bottom, still lit from the top of the screen. The active player's orb flares.

const RING = "M-44 16 A49 41 0 0 0 44 16";

/** The core: a cushion, a rounded oval with flattened sides, `rx` by `ry` about (0, `cy`), a
 * little away from the blades' tips; the life total centers on it. */
const CORE = { rx: 49, ry: 37, cy: -3 };

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

/** A life total in a dark oval; the active player's flares gold. */
export function LifeOrb({
  life,
  active = false,
  seat = "you",
  anchor,
}: {
  life: number;
  active?: boolean;
  /** Whose orb: its ring underlines the side toward that player's edge. */
  seat?: "you" | "opponent";
  /** The element id of the oval, which arrows meet. */
  anchor?: string;
}) {
  // The opponent's art is drawn flipped, so the effects that depend on where the light comes
  // from flip with it, keeping it on top; their ids carry the seat so two orbs can share a page.
  const down = seat === "you" ? 1 : -1;
  const id = (name: string) => `orb-${name}-${seat}`;
  return (
    <div className={`life-orb${active ? " life-orb-active" : ""}`}>
      <svg className="life-orb-art" viewBox="-136 -57 272 116" aria-hidden>
        <defs>
          <linearGradient
            id={id("rim")}
            x1="0"
            y1={(1 - down) / 2}
            x2="0"
            y2={(1 + down) / 2}
          >
            <stop offset="0" stopColor="#dcd8d2" />
            <stop offset="0.2" stopColor="#8e8984" />
            <stop offset="0.6" stopColor="#4a4643" />
            <stop offset="1" stopColor="#3a3633" />
          </linearGradient>
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
          <filter
            id={id("shadow")}
            x="-20%"
            y="-20%"
            width="140%"
            height="160%"
          >
            <feDropShadow
              dx="0"
              dy={2 * down}
              stdDeviation="1.5"
              floodOpacity="0.6"
            />
          </filter>
        </defs>
        <g transform={`scale(1 ${down})`}>
          {active && (
            <g className="life-orb-flare">
              <ellipse cx="0" cy="-2" rx="62" ry="50" filter="url(#orb-blur)" />
              <path d={RAYS} filter="url(#orb-blur)" />
              <path d={RAYS} fill="url(#orb-ray)" />
            </g>
          )}
          {active && (
            <path
              className="life-orb-halo"
              d="M-42 -17 A47 37 0 0 1 42 -17"
              filter="url(#orb-blur)"
            />
          )}
          <path
            id={anchor}
            data-oval
            className="life-orb-core"
            d={cushion(CORE)}
            stroke={`url(#${id("rim")})`}
            filter={`url(#${id("shadow")})`}
          />
          <path
            className="life-orb-ring life-orb-ring-glow"
            d={RING}
            filter="url(#orb-blur)"
          />
          <path className="life-orb-ring" d={RING} />
          {active && (
            <ellipse
              className="life-orb-bloom"
              cx="0"
              cy={CORE.cy - CORE.ry}
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
        </g>
      </svg>
      <span
        className="life-orb-total"
        style={{ marginTop: `calc(${CORE.cy * down} * var(--orb))` }}
      >
        {life}
      </span>
    </div>
  );
}

/** Unspent mana by type, as the protocol's ManaPool counts it. */
export interface Pool {
  white: number;
  blue: number;
  black: number;
  red: number;
  green: number;
  colorless: number;
}

const POOL_SYMBOLS: [keyof Pool, string][] = [
  ["white", "W"],
  ["blue", "U"],
  ["black", "B"],
  ["red", "R"],
  ["green", "G"],
  ["colorless", "C"],
];

/** Whether any mana is unspent. */
export const floating = (pool: Pool) => POOL_SYMBOLS.some(([k]) => pool[k] > 0);

/** How long a pulse holds the pool on screen, in milliseconds, though it has emptied. */
const PULSE_MS = 900;

/** A player's unspent mana beside their life orb, one symbol per type with its amount when
 * more than one, shown only while there is any (CR 106.4). Each change of `pulse` (a pass that
 * ends the step and so loses it) makes it pulse, held on screen a moment though it empties. */
export function ManaPool({ pool, pulse = 0 }: { pool: Pool; pulse?: number }) {
  const [held, setHeld] = useState<{ pool: Pool; pulse: number }>();
  useEffect(() => {
    if (pulse === 0) return;
    // The pool as the pulse begins, which passing may empty as the step ends (CR 500.4). Only
    // a new pulse starts one, so the pool is read but not watched.
    setHeld({ pool, pulse });
    const timer = setTimeout(() => setHeld(undefined), PULSE_MS);
    return () => clearTimeout(timer);
  }, [pulse]);
  const shown = floating(pool) ? pool : held?.pool;
  if (!shown || !floating(shown)) return null;
  const symbols = POOL_SYMBOLS.filter(([k]) => shown[k] > 0);
  return (
    <div
      key={held?.pulse}
      className={held ? "mana-pool mana-pool-pulse" : "mana-pool"}
      role="img"
      aria-label={`Unspent mana: ${symbols
        .map(([k, s]) => `{${s}}`.repeat(shown[k]))
        .join("")}`}
      data-pool
    >
      {symbols.map(([k, s]) => (
        <span key={k} className="mana-pool-entry">
          <ManaSymbol symbol={s} />
          {shown[k] > 1 && <b>{shown[k]}</b>}
        </span>
      ))}
    </div>
  );
}

/** A player's name on a dark band behind a nested brass diamond, and under it what they are
 * doing, if anything: the opponent thinking or taking their turn. */
/** The coin of the flip that settles who plays first, which then marks the starting player: a
 * gold disc bearing a 1, drawn by its style as the icon it is, not text. It lands with a flip as
 * it appears, or spins while the flip's winner chooses, or once flipped rocks between the seats
 * bearing its 1 while the opponent chooses. Beside a player who does not go first, or while it
 * spins, it is blank. */
export function Coin({
  spinning = false,
  choosing = false,
  first = true,
  size = 44,
  className,
}: {
  spinning?: boolean;
  /** It rocks between the seats while the flip's winner chooses who goes first. */
  choosing?: boolean;
  /** It bears the 1: it stands beside the player who goes first. */
  first?: boolean;
  size?: number;
  className?: string;
}) {
  return (
    <span
      className={[
        "coin",
        spinning
          ? "coin-spinning"
          : choosing
            ? "coin-choosing"
            : "coin-landing",
        first && "coin-first",
        className,
      ]
        .filter(Boolean)
        .join(" ")}
      style={{ width: size, height: size, fontSize: size * 0.56 }}
      aria-hidden
    />
  );
}

/** A player's emblem beside what they are doing and their mulligans. The table names no one (whose
 * side is plain from where it is), so there it shows only while it has something to say. */
export function PlayerBadge({
  name,
  activity,
  mulligans = 0,
  seat = "you",
}: {
  name?: string;
  activity?: string;
  /** Mulligans taken, shown as a badge once there are any. */
  mulligans?: number;
  /** Whose badge: its tip opens toward the middle of the table. */
  seat?: "you" | "opponent";
}) {
  if (name === undefined && activity === undefined && mulligans === 0) {
    return null;
  }
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
      <span className="player-badge-name">
        {name}
        {activity && (
          <small className="player-badge-activity" role="status">
            {activity}
            <i className="working-dots" aria-hidden>
              <i />
              <i />
              <i />
            </i>
          </small>
        )}
      </span>
      {mulligans > 0 && (
        <span
          className="player-badge-mulligans has-tip"
          tabIndex={0}
          data-mulligans={mulligans}
        >
          <i className="player-badge-mulligans-card" aria-hidden />
          <span>{mulligans}</span>
          <Tip side={seat === "you" ? "above" : "below"}>
            {mulligans === 1 ? "1 mulligan" : `${mulligans} mulligans`}
          </Tip>
        </span>
      )}
    </div>
  );
}

/** A player's emblems ([CR 114.1](https://yawgatog.com/resources/magic-rules/#R1141): "An
 * emblem is a marker used to represent an object that has one or more abilities"), each a
 * round medallion of its art; pointing at one shows it whole. */
export function Emblems({
  names,
  onPoint,
}: {
  names: string[];
  /** The emblem pointed at, or none once the pointer leaves it. */
  onPoint(name: string | undefined): void;
}) {
  if (names.length === 0) return null;
  return (
    <div className="emblems" aria-label="Emblems">
      {names.map((name, i) => (
        <Emblem key={`${i}-${name}`} name={name} onPoint={onPoint} />
      ))}
    </div>
  );
}

function Emblem({
  name,
  onPoint,
}: {
  name: string;
  onPoint(name: string | undefined): void;
}) {
  const art = useCardArt(name);
  const on = () => onPoint(name);
  const off = () => onPoint(undefined);
  return (
    <span
      className="emblem"
      tabIndex={0}
      role="img"
      aria-label={name}
      data-emblem={name}
      onPointerEnter={on}
      onPointerLeave={off}
      onFocus={on}
      onBlur={off}
    >
      <CardArt
        card={{
          name,
          typeLine: art?.face.type_line ?? "",
          colors: [],
          art: art?.src,
        }}
      />
    </span>
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
  const edges = Math.min(20, Math.ceil(count / 2));
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
              height: width / CARD_ASPECT,
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
  depth = GRAVEYARD_DEPTH,
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
      className="pile-face-up"
    >
      {top ? (
        <Card card={top} width={width} />
      ) : (
        <div className="pile-empty" style={{ width }} />
      )}
    </Pile>
  );
}

/** The edges beneath the top card of a face-up pile of `count` cards, a graveyard or exile,
 * for a top card drawn by someone else: the game screen draws every card itself, so it can
 * move. The top card's place is `-depth.x` pixels in from the left. */
export function FaceUpEdges({
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
      label={`${count} cards`}
      className="pile-face-up"
    >
      <div style={{ width, height: width / CARD_ASPECT }} />
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
