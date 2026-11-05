// Player and zones: life orbs, player badges, the library, the graveyard and stacked lands.
import { ASPECT, Card, CardBack, type CardFace } from "./Card";

// Two metallic blades per side, tucked under the orb and sweeping out to an upturned tip
// (left side; the right is mirrored). Units: the orb's center is the origin.
const BLADES =
  "M-18 24 C-42 32 -74 22 -94 -14 C-80 4 -54 12 -26 12 Z " +
  "M-12 34 C-46 44 -82 34 -99 0 C-88 24 -52 30 -16 24 Z";

// The active player's flare: rays fanning up from behind the orb.
const RAYS = Array.from({ length: 11 }, (_, i) => {
  const angle = ((-165 + i * 15) * Math.PI) / 180;
  const length = i % 2 ? 48 : 60;
  const spread = 0.07;
  const point = (a: number, r: number) =>
    `${(Math.cos(a) * r).toFixed(1)} ${(Math.sin(a) * r).toFixed(1)}`;
  return `M${point(angle - spread, 18)} L${point(angle, length)} L${point(angle + spread, 18)} Z`;
}).join(" ");

/** A life total in a dark orb on metallic blades; the active player's flares ember. */
export function LifeOrb({
  life,
  active = false,
}: {
  life: number;
  active?: boolean;
}) {
  return (
    <div className={active ? "life-orb life-orb-active" : "life-orb"}>
      <svg className="life-orb-wings" viewBox="-100 -44 200 88" aria-hidden>
        <defs>
          <linearGradient id="blade-metal" x1="0" y1="0" x2="0" y2="1">
            <stop offset="0" stopColor="#fbf9f5" />
            <stop offset="0.45" stopColor="#b9b3ab" />
            <stop offset="1" stopColor="#4d4843" />
          </linearGradient>
          <radialGradient
            id="orb-ray"
            cx="0"
            cy="0"
            r="62"
            gradientUnits="userSpaceOnUse"
          >
            <stop offset="0.25" stopColor="#fff1c2" />
            <stop offset="0.55" stopColor="#ffae3a" stopOpacity="0.95" />
            <stop offset="1" stopColor="#f0701c" stopOpacity="0" />
          </radialGradient>
          <radialGradient id="orb-glow" cx="0.5" cy="0.5" r="0.5">
            <stop offset="0" stopColor="#ffb54a" stopOpacity="0.9" />
            <stop offset="1" stopColor="#f07a1c" stopOpacity="0" />
          </radialGradient>
        </defs>
        {active && (
          <>
            <ellipse
              cx="0"
              cy="-4"
              rx="62"
              ry="40"
              fill="url(#orb-glow)"
              opacity="0.55"
            />
            <path d={RAYS} fill="url(#orb-ray)" />
          </>
        )}
        <ellipse
          className="life-orb-underglow"
          cx="0"
          cy="28"
          rx={active ? 40 : 28}
          ry={active ? 9 : 4}
          fill="url(#orb-glow)"
        />
        <path className="life-orb-blades" d={BLADES} />
        <path className="life-orb-blades" d={BLADES} transform="scale(-1 1)" />
      </svg>
      <div className="life-orb-core">
        <span>{life}</span>
      </div>
    </div>
  );
}

export function PlayerBadge({ name }: { name: string }) {
  return (
    <div className="player-badge">
      <svg className="player-badge-emblem" viewBox="0 0 40 40" aria-hidden>
        <defs>
          <linearGradient id="emblem-gold" x1="0" y1="0" x2="1" y2="1">
            <stop offset="0" stopColor="#fbe2a6" />
            <stop offset="0.5" stopColor="#d99a3c" />
            <stop offset="1" stopColor="#7c4f18" />
          </linearGradient>
        </defs>
        <path d="M20 1 L39 20 L20 39 L1 20 Z" fill="url(#emblem-gold)" />
        <path d="M20 6 L34 20 L20 34 L6 20 Z" fill="#2a2420" />
        <path
          d="M20 11 L29 20 L20 29 L11 20 Z"
          fill="none"
          stroke="url(#emblem-gold)"
          strokeWidth="2.2"
        />
        <path d="M20 16 L24 20 L20 24 L16 20 Z" fill="#f6d690" />
      </svg>
      <span>{name}</span>
    </div>
  );
}

/** A card back over one edge per few cards beneath it, so a thick library looks thick. */
export function Library({
  count,
  width = 110,
}: {
  count: number;
  width?: number;
}) {
  const edges = Math.min(8, Math.ceil(count / 5));
  const step = 1.6;
  const depth = edges * step;
  return (
    <div
      className="pile"
      style={{ paddingLeft: depth, paddingBottom: depth }}
      aria-label={`Library, ${count} cards`}
    >
      {Array.from({ length: edges }, (_, i) => {
        const k = edges - i; // deepest first
        return (
          <div
            key={i}
            className="pile-edge"
            style={{
              left: depth - k * step,
              top: k * step,
              width,
              height: width / ASPECT.full,
            }}
          />
        );
      })}
      {count > 0 && <CardBack width={width} />}
    </div>
  );
}

export function Graveyard({
  cards,
  width = 110,
}: {
  cards: CardFace[];
  width?: number;
}) {
  const top = cards[cards.length - 1];
  return (
    <div
      className="pile graveyard"
      aria-label={`Graveyard, ${cards.length} cards`}
    >
      {Array.from({ length: Math.min(4, cards.length - 1) }, (_, i) => (
        <div
          key={i}
          className="pile-edge"
          style={{ left: (4 - i) * 2, top: (4 - i) * 2 }}
        />
      ))}
      {top ? (
        <Card card={top} width={width} />
      ) : (
        <div className="pile-empty" style={{ width }} />
      )}
    </div>
  );
}

/** Identical lands fanned to the left, with a ×N count once there are several. */
export function LandStack({
  card,
  count,
  width = 150,
  tapped = 0,
  showCount = true,
}: {
  card: CardFace;
  count: number;
  width?: number;
  tapped?: number;
  /** The ×N badge; on the table the edges of the copies beneath show the count instead. */
  showCount?: boolean;
}) {
  const offset = width * 0.075;
  return (
    <div className="land-stack" style={{ paddingLeft: (count - 1) * offset }}>
      {Array.from({ length: count }, (_, i) => (
        <Card
          key={i}
          card={card}
          variant="field"
          width={width}
          tapped={i >= count - tapped}
          className={i < count - 1 ? "card-under" : undefined}
          style={
            i < count - 1
              ? { position: "absolute", left: i * offset, top: 0 }
              : undefined
          }
        />
      ))}
      {showCount && count > 1 && <span className="land-count">×{count}</span>}
    </div>
  );
}
