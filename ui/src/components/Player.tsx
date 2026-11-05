// Player and zones: life orbs, player badges, the library, the graveyard and stacked lands.
import { ASPECT, Card, CardBack, type CardFace } from "./Card";

const WING =
  "M0 0 C-14 -2 -30 -12 -44 -26 C-36 -10 -22 2 -4 8 Z " +
  "M-2 10 C-20 8 -40 0 -54 -10 C-44 6 -26 16 -6 18 Z " +
  "M-4 20 C-20 22 -36 18 -48 10 C-38 24 -22 30 -8 28 Z";

/** A life total between silver wings; the active player's glows ember. */
export function LifeOrb({
  life,
  active = false,
}: {
  life: number;
  active?: boolean;
}) {
  return (
    <div className={active ? "life-orb life-orb-active" : "life-orb"}>
      <svg className="life-orb-wings" viewBox="-80 -40 160 80" aria-hidden>
        <defs>
          <linearGradient id="wing-silver" x1="0" y1="0" x2="0" y2="1">
            <stop offset="0" stopColor="#f4f1ec" />
            <stop offset="0.5" stopColor="#a8a29a" />
            <stop offset="1" stopColor="#5d5852" />
          </linearGradient>
          <radialGradient id="orb-flare" cx="0.5" cy="0.62" r="0.5">
            <stop offset="0" stopColor="#ffd27a" stopOpacity="0.95" />
            <stop offset="0.45" stopColor="#f07a1c" stopOpacity="0.6" />
            <stop offset="1" stopColor="#f07a1c" stopOpacity="0" />
          </radialGradient>
        </defs>
        {active && (
          <ellipse className="life-orb-flare" cx="0" cy="12" rx="54" ry="30" />
        )}
        <path d={WING} transform="translate(-24 -6)" fill="url(#wing-silver)" />
        <path
          d={WING}
          transform="translate(24 -6) scale(-1 1)"
          fill="url(#wing-silver)"
        />
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
        <path d="M20 2 L38 20 L20 38 L2 20 Z" className="emblem-outer" />
        <path d="M20 9 L31 20 L20 31 L9 20 Z" className="emblem-inner" />
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
      {cards.length > 1 && (
        <div className="pile-edge" style={{ left: 3, top: 3 }} />
      )}
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
}: {
  card: CardFace;
  count: number;
  width?: number;
  tapped?: number;
}) {
  const offset = width * 0.06;
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
      {count > 1 && <span className="land-count">×{count}</span>}
    </div>
  );
}
