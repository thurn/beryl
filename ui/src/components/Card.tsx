// The card frame: a name bar colored by the card's colors, art, a P/T badge and, on the full
// frame, the type line and rules box. Battlefield permanents use the compact `field` frame.
import {
  createContext,
  useContext,
  type CSSProperties,
  type ReactNode,
} from "react";
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
  /** Art fills the frame below the name bar, without a type line (full-art basic lands). */
  fullArt?: boolean;
  ability?: AbilityIcon;
}

/** Art for named card slots (`you-mountain`, `04-titan`): replica mode (src/replica/) supplies
 * the art cut from a mockup; elsewhere there is none and cards keep their own art. */
export const SlotArt = createContext<(slot: string) => string | undefined>(
  () => undefined,
);

/** Gives a card the art of its slot, when there is any. */
export function useSlotArt() {
  const art = useContext(SlotArt);
  return (card: CardFace, slot: string | undefined): CardFace => {
    const url = slot && art(slot);
    return url ? { ...card, art: url } : card;
  };
}

export type Highlight = "source" | "target";
export type CombatRole = "attacker" | "blocker";

export interface CardProps {
  card: CardFace;
  variant?: "full" | "field";
  /** The card's width in pixels; everything else scales with it. */
  width?: number;
  /** The card's height in pixels, when it is not the frame's usual aspect. */
  height?: number;
  /** The frame's unit in pixels (bars, borders and type scale with it), when not width / 12. */
  unit?: number;
  tapped?: boolean;
  highlight?: Highlight;
  combat?: CombatRole;
  className?: string;
  /** Placement on the slot: Motion style, so `rotate` and `y` compose with layout animation. */
  style?: MotionStyle;
  id?: string;
}

/** Width over height of each frame. */
export const ASPECT = { full: 63 / 88, field: 1.3 } as const;

const BASIC_TYPES: [string, Color][] = [
  ["Plains", "W"],
  ["Island", "U"],
  ["Swamp", "B"],
  ["Mountain", "R"],
  ["Forest", "G"],
];

/** The frame's palette key: a color, `m` for multicolored, `c` for colorless. A colorless land
 * with one basic land type takes that type's color; a dual land keeps the pale land frame, as on
 * the mockups' Plateau. */
export function frameKey(card: CardFace): string {
  let colors = card.colors;
  if (colors.length === 0 && isLand(card)) {
    const basics = BASIC_TYPES.filter(([type]) => card.typeLine.includes(type));
    colors = basics.length === 1 ? basics.map(([, c]) => c) : [];
  }
  if (colors.length === 0) return "c";
  if (colors.length > 1) return "m";
  return (colors[0] as Color).toLowerCase();
}

const isLand = (card: CardFace) => card.typeLine.includes("Land");

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

/** Rules text up to this long is centered in its box. */
const SHORT_TEXT = 30;

/** Shrinks a long name to fit its bar rather than cutting it off: components.css caps the name's
 * size by the bar's `--room` (frame units inside the frame's borders, beside the cost) over its
 * `--chars`. `units` is
 * the frame's width in frame units: 12 unless overridden or turned. */
function nameFit(name: string, units: number, cost = 0): CSSProperties {
  return {
    "--chars": name.length,
    "--room": units - 2.4 - 0.85 * cost,
  } as CSSProperties;
}

/** Power and toughness (or loyalty), numerals inked blue and the slash black as on the mockups. */
function stats(card: CardFace): ReactNode {
  if (card.power !== undefined && card.toughness !== undefined) {
    return (
      <span>
        <b>{card.power}</b>/<b>{card.toughness}</b>
      </span>
    );
  }
  return card.loyalty !== undefined ? (
    <span>
      <b>{card.loyalty}</b>
    </span>
  ) : undefined;
}

export function Card({
  card,
  variant = "full",
  width = 160,
  height: givenHeight,
  unit,
  tapped = false,
  highlight,
  combat,
  className,
  style,
  id,
}: CardProps) {
  const height = givenHeight ?? width / ASPECT[variant];
  const pt = stats(card);
  const classes = [
    "card",
    `card-${variant}`,
    variant === "full" && card.fullArt && "card-full-art",
    `frame-${frameKey(card)}`,
    isLand(card) && "card-land",
    tapped && "card-tapped",
    card.colors.length > 1 && `accent-${card.colors[0]?.toLowerCase()}`,
    highlight && `card-${highlight}`,
    combat && `card-${combat}`,
    className,
  ]
    .filter(Boolean)
    .join(" ");
  // Turned a quarter left when tapped. A battlefield card keeps its footprint, as in the mockups:
  // its frame turns portrait as it rotates, so the name bar runs up the left side. A full card's
  // slot takes its turned footprint instead, so neighbors make room.
  const keep = variant === "field";
  const slot =
    tapped && !keep ? { width: height, height: width } : { width, height };
  const frame =
    tapped && keep ? { width: height, height: width } : { width, height };
  return (
    <motion.div
      layout
      className="card-slot"
      style={{ ...slot, fontSize: unit ?? width / 12, ...style }}
      id={id}
    >
      {combat === "attacker" && <AttackChevron />}
      {combat === "blocker" && <BlockArc />}
      <motion.div
        className={classes}
        initial={false}
        animate={{ rotate: tapped ? -90 : 0, ...frame }}
        transition={{ type: "spring", stiffness: 260, damping: 26 }}
      >
        <div className="card-rim">
          <div className="card-name-bar">
            <span
              className="card-name"
              style={nameFit(
                card.name,
                frame.width / (unit ?? width / 12),
                variant === "full"
                  ? (card.manaCost?.split("{").length ?? 1) - 1
                  : 0,
              )}
            >
              {card.name}
            </span>
            {variant === "full" && card.manaCost && (
              <ManaCost cost={card.manaCost} />
            )}
          </div>
          <CardArt card={card} />
          {variant === "full" && !card.fullArt && (
            <>
              <div className="card-type-bar">{card.typeLine}</div>
              <div
                className={
                  (card.text?.length ?? 0) <= SHORT_TEXT
                    ? "card-rules card-rules-short"
                    : "card-rules"
                }
              >
                {card.text && (
                  <p>
                    <RulesText text={card.text} />
                  </p>
                )}
              </div>
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
    <svg
      className="combat-marker attack-chevron"
      viewBox="0 0 100 18"
      aria-hidden
    >
      <path d="M3 16 L50 2 L97 16" />
      <path d="M22 17 L50 9 L78 17" strokeWidth="1.4" />
    </svg>
  );
}

function BlockArc() {
  return (
    <svg className="combat-marker block-arc" viewBox="0 0 100 16" aria-hidden>
      <path d="M3 2 Q50 26 97 2" />
    </svg>
  );
}

/** The back of a card, for hidden cards and libraries: the Project Beryl back of scry.png, tooled
 * leather with corner studs around an oval panel bearing the crest, the wordmark and five dots. */
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
      <div className="card-back-field">
        {["r", "w", "g", "u"].map((c) => (
          <i key={c} className={`card-back-stud dot-${c}`} />
        ))}
        <div className="card-back-oval">
          <Crest className="card-back-crest" />
          <span className="card-back-title">Project Beryl</span>
          <span className="card-back-dots">
            {["w", "r", "g", "b", "u"].map((c) => (
              <i key={c} className={`dot-${c}`} />
            ))}
          </span>
        </div>
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
