// The card frame: a name bar colored by the card's colors, art, a P/T badge and, on the full
// frame, the type line and rules box. Battlefield permanents use the compact `field` frame.
import type { CSSProperties } from "react";
import { motion, type MotionStyle } from "motion/react";
import { AbilityBadge, type AbilityIcon, ManaCost, RulesText } from "./Mana";

export type Color = "W" | "U" | "B" | "R" | "G";

export interface CardFace {
  name: string;
  typeLine: string;
  colors: Color[];
  manaCost?: string;
  text?: string;
  power?: string;
  toughness?: string;
  loyalty?: string;
  /** Art URL; a placeholder painting is drawn until there is one. */
  art?: string;
  ability?: AbilityIcon;
}

export type Highlight = "source" | "target";
export type CombatRole = "attacker" | "blocker";

export interface CardProps {
  card: CardFace;
  variant?: "full" | "field";
  /** The card's width in pixels; everything else scales with it. */
  width?: number;
  tapped?: boolean;
  highlight?: Highlight;
  combat?: CombatRole;
  className?: string;
  /** Placement on the slot: Motion style, so `rotate` and `y` compose with layout animation. */
  style?: MotionStyle;
  id?: string;
}

/** Width over height of each frame. */
export const ASPECT = { full: 63 / 88, field: 1.27 } as const;

const BASIC_TYPES: [string, Color][] = [
  ["Plains", "W"],
  ["Island", "U"],
  ["Swamp", "B"],
  ["Mountain", "R"],
  ["Forest", "G"],
];

/** The frame's palette key: a color, `m` for multicolored, `c` for colorless. Lands without a
 * color take the color of their basic land types. */
export function frameKey(card: CardFace): string {
  let colors = card.colors;
  if (colors.length === 0 && card.typeLine.includes("Land")) {
    colors = BASIC_TYPES.filter(([type]) => card.typeLine.includes(type)).map(
      ([, c]) => c,
    );
  }
  if (colors.length === 0) return "c";
  if (colors.length > 1) return "m";
  return (colors[0] as Color).toLowerCase();
}

const HUES: Record<string, number> = {
  w: 42,
  u: 205,
  b: 265,
  r: 14,
  g: 115,
  m: 40,
  c: 28,
};

function hash(text: string): number {
  let h = 2166136261;
  for (let i = 0; i < text.length; i++) {
    h = Math.imul(h ^ text.charCodeAt(i), 16777619);
  }
  return h >>> 0;
}

/** A deterministic stand-in painting in the card's colors: sky, horizon and a light source. */
function placeholderArt(card: CardFace): string {
  const h = hash(card.name);
  const hue = (HUES[frameKey(card)] ?? 30) + ((h % 40) - 20);
  const sunX = 20 + (h % 60);
  const sunY = 18 + ((h >> 8) % 25);
  const ridge = 52 + ((h >> 16) % 18);
  return [
    `radial-gradient(circle at ${sunX}% ${sunY}%, hsl(${hue + 30} 95% 85% / 0.9), transparent 28%)`,
    `radial-gradient(ellipse 80% 45% at ${100 - sunX}% ${ridge + 30}%, hsl(${hue} 45% 18% / 0.95), transparent 70%)`,
    `linear-gradient(172deg, transparent ${ridge}%, hsl(${hue - 10} 40% 24%) ${ridge + 4}%, hsl(${hue - 20} 45% 12%))`,
    `linear-gradient(180deg, hsl(${hue + 10} 55% 72%), hsl(${hue} 60% 46%) 55%, hsl(${hue - 15} 55% 30%))`,
  ].join(", ");
}

export function CardArt({ card }: { card: CardFace }) {
  return card.art ? (
    <img className="card-art" src={card.art} alt="" draggable={false} />
  ) : (
    <div
      className="card-art"
      style={{ backgroundImage: placeholderArt(card) }}
    />
  );
}

function stats(card: CardFace): string | undefined {
  if (card.power !== undefined && card.toughness !== undefined) {
    return `${card.power}/${card.toughness}`;
  }
  return card.loyalty;
}

export function Card({
  card,
  variant = "full",
  width = 160,
  tapped = false,
  highlight,
  combat,
  className,
  style,
  id,
}: CardProps) {
  const height = width / ASPECT[variant];
  const pt = stats(card);
  const classes = [
    "card",
    `card-${variant}`,
    variant === "full" && !card.text && "card-textless",
    `frame-${frameKey(card)}`,
    highlight && `card-${highlight}`,
    combat && `card-${combat}`,
    className,
  ]
    .filter(Boolean)
    .join(" ");
  // A tapped card's slot takes its turned footprint, so neighbors make room as it turns.
  const slot = tapped ? { width: height, height: width } : { width, height };
  return (
    <motion.div
      layout
      className="card-slot"
      style={{ ...slot, fontSize: width / 12, ...style }}
      id={id}
    >
      {combat === "attacker" && <AttackChevron />}
      <motion.div
        className={classes}
        style={{ width, height }}
        initial={false}
        animate={{ rotate: tapped ? -90 : 0 }}
        transition={{ type: "spring", stiffness: 260, damping: 26 }}
      >
        <div className="card-rim">
          <div className="card-name-bar">
            <span className="card-name">{card.name}</span>
            {variant === "full" && card.manaCost && (
              <ManaCost cost={card.manaCost} />
            )}
          </div>
          <CardArt card={card} />
          {variant === "full" && (
            <>
              <div className="card-type-bar">{card.typeLine}</div>
              {card.text && (
                <div className="card-rules">
                  <p>
                    <RulesText text={card.text} />
                  </p>
                </div>
              )}
            </>
          )}
        </div>
        {card.ability && variant === "field" && (
          <AbilityBadge ability={card.ability} />
        )}
        {pt && <span className="card-pt">{pt}</span>}
      </motion.div>
    </motion.div>
  );
}

function AttackChevron() {
  return (
    <svg className="attack-chevron" viewBox="0 0 60 18" aria-hidden>
      <path d="M2 16 L30 3 L58 16" />
      <path d="M12 17 L30 9 L48 17" />
    </svg>
  );
}

/** The back of a card, for hidden cards and libraries. */
export function CardBack({
  width = 160,
  style,
}: {
  width?: number;
  style?: CSSProperties;
}) {
  return (
    <div
      className="card-back"
      style={{
        width,
        height: width / ASPECT.full,
        fontSize: width / 12,
        ...style,
      }}
    >
      <div className="card-back-oval">
        <Crest className="card-back-crest" />
        <span className="card-back-title">Project Beryl</span>
        <span className="card-back-dots">
          {["w", "u", "b", "r", "g"].map((c) => (
            <i key={c} className={`dot-${c}`} />
          ))}
        </span>
      </div>
    </div>
  );
}

/** The Beryl crest: a five-pointed crown of flame over a stem, after the mockup's wordmark. */
export function Crest({ className }: { className?: string }) {
  return (
    <svg className={className} viewBox="0 0 48 56" aria-hidden>
      <path
        fill="currentColor"
        d="M24 56 C21 46 19 40 12 33 C6 27 3 20 4 9 C8 16 10 20 14 23 C13 15 13 8 16 1 C18 9 19 16 21 21 C21 13 22 7 24 0 C26 7 27 13 27 21 C29 16 30 9 32 1 C35 8 35 15 34 23 C38 20 40 16 44 9 C45 20 42 27 36 33 C29 40 27 46 24 56 Z"
      />
    </svg>
  );
}
