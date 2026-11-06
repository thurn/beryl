// The card frame: a name bar colored by the card's colors, art, a P/T badge and, on the full
// frame, the type line and rules box. Battlefield permanents use the compact `field` frame,
// with their marks: a badge per kind of counter, P/T inked up or down against the printed
// values, summoning sickness as faded art, a token's silver frame and a stolen permanent's
// violet edge; the top card of a pile of identical ones wears their count. Tapping turns
// either frame whole, never reshaping it.
import {
  useLayoutEffect,
  useRef,
  type CSSProperties,
  type ReactNode,
} from "react";
import { motion, type MotionStyle } from "motion/react";
import { useTextFit } from "./fit";
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
  /** How the power and toughness shown compare with the printed ones: raised, or lowered by
   * an effect or, for toughness, by marked damage. */
  powerTone?: StatTone;
  toughnessTone?: StatTone;
}

export type StatTone = "up" | "down";

/** A kind of counter a permanent wears as a badge; loyalty is its plaque's. */
export type CounterMark = "plusOne" | "minusOne" | "time" | "story" | "chorus";

/** What the battlefield draws on a permanent beyond its frame. */
export interface CardMarks {
  counters: { kind: CounterMark; count: number }[];
  /** Its controller can't attack or tap with it yet (CR 302.6). */
  sick: boolean;
  /** It is a token, not a card (CR 111.1). */
  token?: boolean;
  /** Its controller is not its owner. */
  stolen?: boolean;
}

export type Highlight = "source" | "target";
export type CombatRole = "attacker" | "blocker";

export interface CardProps {
  card: CardFace;
  variant?: "full" | "field";
  /** The card's width in pixels; the frame's aspect and everything else scale with it. */
  width?: number;
  tapped?: boolean;
  highlight?: Highlight;
  combat?: CombatRole;
  marks?: CardMarks;
  /** On the top card of a pile of identical permanents, how many it holds. */
  count?: number;
  className?: string;
  /** Placement on the slot: Motion style, so `rotate` and `y` compose with layout animation. */
  style?: MotionStyle;
  id?: string;
}

/** How a card turns as it taps or untaps, and its slot with it. */
const TURN = { type: "spring", stiffness: 260, damping: 26 } as const;

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

/** Shrinks long rules text to fit its box rather than cutting it off mid-line, as the cube's
 * longer cards (planeswalkers, Aang's Iceberg) would be: `--fit` scales the text's size
 * (components.css), stepped down until the text fits, measured again whenever the box resizes.
 * Text rewraps as it shrinks, so its height falls faster than its size: each step shrinks by
 * the square root of the overflow. */
function useRulesFit(text: string | undefined) {
  const ref = useRef<HTMLParagraphElement>(null);
  useLayoutEffect(() => {
    const p = ref.current;
    const box = p?.parentElement;
    if (!p || !box) return;
    const fit = () => {
      p.style.removeProperty("--fit");
      const style = getComputedStyle(box);
      const room =
        box.clientHeight -
        parseFloat(style.paddingTop) -
        parseFloat(style.paddingBottom);
      let scale = 1;
      for (let i = 0; i < 8 && room > 0 && p.scrollHeight > room + 0.5; i++) {
        scale *= Math.sqrt(room / p.scrollHeight) * 0.98;
        p.style.setProperty("--fit", scale.toFixed(3));
      }
    };
    fit();
    void document.fonts.ready.then(fit);
    const observer = new ResizeObserver(fit);
    observer.observe(box);
    return () => observer.disconnect();
  }, [text]);
  return ref;
}

/** Power and toughness (or loyalty), numerals inked blue and the slash black as on the mockups,
 * or green when raised and red when lowered, as Arena inks them. */
function stats(card: CardFace): ReactNode {
  if (card.power !== undefined && card.toughness !== undefined) {
    return (
      <span>
        <b className={toneClass(card.powerTone)}>{card.power}</b>
        <i>/</i>
        <b className={toneClass(card.toughnessTone)}>{card.toughness}</b>
      </span>
    );
  }
  return card.loyalty !== undefined ? (
    <span>
      <b>{card.loyalty}</b>
    </span>
  ) : undefined;
}

export const toneClass = (tone: StatTone | undefined) => tone && `stat-${tone}`;

/** The icons of a permanent's marks, on a 16-unit square: counters by kind, and for the
 * inspector loyalty, damage and summoning sickness. */
const MARK_ICONS = {
  plusOne: "M8 1.5 L14.5 9.5 H10.5 V14.5 H5.5 V9.5 H1.5 Z",
  minusOne: "M8 14.5 L1.5 6.5 H5.5 V1.5 H10.5 V6.5 H14.5 Z",
  time: "M3 1 H13 V3.2 C13 5.8 10.6 7 9.3 8 C10.6 9 13 10.2 13 12.8 V15 H3 V12.8 C3 10.2 5.4 9 6.7 8 C5.4 7 3 5.8 3 3.2 Z",
  story:
    "M0.8 3 C3.2 1.8 5.8 2.1 7.4 3.4 V14.2 C5.8 13 3.2 12.8 0.8 13.8 Z M15.2 3 C12.8 1.8 10.2 2.1 8.6 3.4 V14.2 C10.2 13 12.8 12.8 15.2 13.8 Z",
  chorus:
    "M5.6 2.6 L14.4 0.8 V11.6 A2.6 2.1 0 1 1 12.6 9.6 V4.4 L7.4 5.5 V13.2 A2.6 2.1 0 1 1 5.6 11.2 Z",
  loyalty:
    "M8 0.8 L14.4 2.9 V8 C14.4 11.6 11.4 14.2 8 15.3 C4.6 14.2 1.6 11.6 1.6 8 V2.9 Z",
  damage:
    "M8 0.5 L9.8 5.2 L15 4 L11.4 8 L15 12 L9.8 10.8 L8 15.5 L6.2 10.8 L1 12 L4.6 8 L1 4 L6.2 5.2 Z",
  sick: "M10.2 1.2 A7 7 0 1 0 14.8 11.4 A5.6 5.6 0 0 1 10.2 1.2 Z",
} as const;

export type MarkIcon = keyof typeof MARK_ICONS;

/** A mark's badge: its icon, with a count when it has one, named `label` for assistive
 * technology when it stands alone. */
export function MarkBadge({
  icon,
  count,
  label,
}: {
  icon: MarkIcon;
  count?: number;
  label?: string;
}) {
  return (
    <span
      className={`mark-badge mark-${icon}`}
      role={label ? "img" : undefined}
      aria-label={label}
    >
      <svg viewBox="0 0 16 16" aria-hidden>
        <path d={MARK_ICONS[icon]} />
      </svg>
      {count !== undefined && <b>{count}</b>}
    </span>
  );
}

export function Card({
  card,
  variant = "full",
  width = 160,
  tapped = false,
  highlight,
  combat,
  marks,
  count,
  className,
  style,
  id,
}: CardProps) {
  const height = width / ASPECT[variant];
  const nameRef = useTextFit(card.name);
  const typeRef = useTextFit(card.typeLine);
  const rulesRef = useRulesFit(card.text);
  const pt = stats(card);
  const classes = [
    "card",
    `card-${variant}`,
    `frame-${frameKey(card)}`,
    card.colors.length > 1 && `accent-${card.colors[0]?.toLowerCase()}`,
    pt && "card-with-pt",
    card.ability && variant === "field" && "card-with-ability",
    marks?.sick && "card-sick",
    marks?.token && "card-token",
    highlight && `card-${highlight}`,
    combat && `card-${combat}`,
    className,
  ]
    .filter(Boolean)
    .join(" ");
  // Tapped, the whole frame turns a quarter clockwise, as a player turns a card, keeping its
  // shape: its slot takes the turned footprint, so neighbors make room for it. (On the table,
  // layout.ts also draws a tapped frame smaller, to stand as tall as its row.)
  const frame = { width, height };
  const slot = tapped ? { width: height, height: width } : frame;
  return (
    <motion.div
      className="card-slot"
      style={{ fontSize: width / 12, ...style }}
      id={id}
      initial={false}
      animate={slot}
      transition={TURN}
    >
      {combat === "attacker" && <AttackChevron id={id} />}
      {combat === "blocker" && <BlockArc id={id} />}
      <motion.div
        className={classes}
        style={frame}
        initial={false}
        animate={{ rotate: tapped ? 90 : 0 }}
        transition={TURN}
      >
        <div className="card-rim">
          <div className="card-name-bar">
            <span className="card-name" ref={nameRef}>
              {card.name}
            </span>
            {variant === "full" && card.manaCost && (
              <ManaCost cost={card.manaCost} />
            )}
          </div>
          <CardArt card={card} />
          {variant === "full" && (
            <>
              <div className="card-type-bar">
                <span className="card-type-line" ref={typeRef}>
                  {card.typeLine}
                </span>
              </div>
              <div className="card-rules">
                {card.text && (
                  <p ref={rulesRef}>
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
        {marks && marks.counters.length > 0 && (
          <span className="card-counters">
            {marks.counters.map((c) => (
              <MarkBadge
                key={c.kind}
                icon={c.kind}
                count={c.count}
                label={`${c.count} ${c.kind} counters`}
              />
            ))}
          </span>
        )}
        {marks?.stolen && <span className="card-stolen-edge" />}
        {pt && <span className="card-pt">{pt}</span>}
        {count !== undefined && count > 1 && (
          <span className="card-count">×{count}</span>
        )}
      </motion.div>
    </motion.div>
  );
}

// The combat marks, as in docs/ui_mockups/combat.png: over an attacker a wide gold chevron with
// light falling from its apex; under a blocker a gold crescent hugging the card's bottom,
// notched where the line from its attacker meets it. Arrows anchor to them by `${card id}-mark`.
/** Light falling from the chevron's apex: streaks fanning down between its arms. */
const RAYS = [-34, -22, -12, 0, 12, 22, 34]
  .map((dx) => `M50 1 L${50 + dx - 1.2} 16.5 L${50 + dx + 1.2} 16.5 Z`)
  .join(" ");

function AttackChevron({ id }: { id?: string }) {
  const arms = "M0 17 Q30 9 50 0.5 Q70 9 100 17";
  return (
    <svg
      id={id && `${id}-mark`}
      className="combat-marker attack-chevron"
      viewBox="0 0 100 17"
      preserveAspectRatio="none"
      aria-hidden
    >
      <defs>
        <linearGradient id="chevron-light" x1="0" y1="0" x2="0" y2="1">
          <stop offset="0" stopColor="#fff2c8" stopOpacity="0.9" />
          <stop offset="0.6" stopColor="#ffb44a" stopOpacity="0.3" />
          <stop offset="1" stopColor="#e88a20" stopOpacity="0" />
        </linearGradient>
      </defs>
      <path d={`${arms} Z`} fill="url(#chevron-light)" opacity="0.45" />
      <path d={RAYS} fill="url(#chevron-light)" />
      <path className="combat-line" d={arms} />
    </svg>
  );
}

function BlockArc({ id }: { id?: string }) {
  return (
    <svg
      id={id && `${id}-mark`}
      className="combat-marker block-arc"
      viewBox="0 0 100 20"
      preserveAspectRatio="none"
      aria-hidden
    >
      <defs>
        <linearGradient id="block-gold" x1="0" y1="0" x2="0" y2="1">
          <stop offset="0" stopColor="#f6c450" />
          <stop offset="0.55" stopColor="#fde49a" />
          <stop offset="1" stopColor="#fff6d6" />
        </linearGradient>
      </defs>
      <path
        d="M0 0 C1.5 12 22 19.5 50 19.5 C78 19.5 98.5 12 100 0 C97.5 10 78 15 53.5 15 L50 11.5 L46.5 15 C22 15 2.5 10 0 0 Z M47.5 19 L50 24 L52.5 19 Z"
        fill="url(#block-gold)"
      />
    </svg>
  );
}

/** The back of a card, for hidden cards and libraries: the Project Beryl back of scry.png, tooled
 * leather with corner studs around an oval panel bearing the crest, the wordmark and four dots. */
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
        {["r", "w", "w", "r"].map((c, i) => (
          <i key={i} className={`card-back-stud dot-${c}`} />
        ))}
        <div className="card-back-oval">
          <Crest className="card-back-crest" />
          <span className="card-back-title">Project Beryl</span>
          <span className="card-back-dots">
            {["w", "r", "g", "b"].map((c) => (
              <i key={c} className={`dot-${c}`} />
            ))}
          </span>
        </div>
      </div>
    </div>
  );
}

/** The Beryl crest: five slender flames rising from a shield that tapers to a point, after the
 * mockups' wordmark (ui_system.png's header, the brand at the screens' top left). */
export function Crest({ className }: { className?: string }) {
  return (
    <svg className={className} viewBox="0 0 48 66" aria-hidden>
      <path
        fill="currentColor"
        d="M24 66 C23 53 17 44.5 6 39 C2 37.2 0.5 35 1 32 Q4 24 5.5 12 Q7 22 9.5 30 Q11 31 12.8 29 Q13.2 16 13.8 3.5 Q15 16 16.3 28.5 Q19 31 21.7 28.5 Q22.8 12 24 0 Q25.2 12 26.3 28.5 Q29 31 31.7 28.5 Q33 16 34.2 3.5 Q34.8 16 35.2 29 Q37 31 38.5 30 Q41 22 42.5 12 Q44 24 47 32 C47.5 35 46 37.2 42 39 C31 44.5 25 53 24 66 Z"
      />
    </svg>
  );
}
