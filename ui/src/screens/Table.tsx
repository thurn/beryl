// The pseudo-3D table, drawing a TableBoard laid out as docs/ui_mockups/battlefield.png. Permanents lie at a depth
// that grows toward the opponent, so they stay upright rectangles that shrink with distance
// (depthScale), with a slab edge and a cast shadow under each; the hand leans back toward the
// table through CSS perspective. No WebGL. The scene is drawn on a fixed 1600×900 stage scaled to
// fit the window.
import { useEffect, useState, type CSSProperties, type ReactNode } from "react";
import { motion } from "motion/react";
import {
  Card,
  CardBack,
  Crest,
  useSlotArt,
  type CardFace,
  type CombatRole,
  type Highlight,
} from "../components/Card";
import {
  Button,
  FastForwardIcon,
  PhaseTracker,
  PriorityPill,
  type Phase,
} from "../components/Controls";
import {
  Graveyard,
  LandStack,
  Library,
  LifeOrb,
  PlayerBadge,
} from "../components/Player";
import { Arrows, StackItem, type ArrowSpec } from "../components/Stack";
import { SAMPLE_BOARD } from "./samples";
import { ScryOverlay } from "./Scry";

export const STAGE = { width: 1600, height: 900 };

/** Scales a fixed-size stage to fit the window, letterboxed on the table. */
export function Stage({ children }: { children: ReactNode }) {
  const fit = () =>
    Math.min(
      window.innerWidth / STAGE.width,
      window.innerHeight / STAGE.height,
    );
  const [scale, setScale] = useState(fit);
  useEffect(() => {
    const onResize = () => setScale(fit());
    window.addEventListener("resize", onResize);
    return () => window.removeEventListener("resize", onResize);
  }, []);
  return (
    <div className="stage-frame">
      <div className="stage" style={{ ...STAGE, transform: `scale(${scale})` }}>
        {children}
      </div>
    </div>
  );
}

/** The camera's distance from the near edge of the table, in perspective's terms. */
const CAMERA_DISTANCE = 1400;
/** Depth grows linearly up the table, from zero at the near edge. */
const NEAR_Y = 760;
const DEPTH_PER_PX = 0.6;

/** How much a thing lying at screen height `y` is shrunk by distance: d / (d + z), what
 * `perspective(d)` does to something `z` behind the screen plane. */
export const depthScale = (y: number) =>
  CAMERA_DISTANCE / (CAMERA_DISTANCE + Math.max(0, NEAR_Y - y) * DEPTH_PER_PX);

/** Where the table's receding edges meet, far above the stage: in the mockups a card left of
 * center leans left at its near end, and one right of center leans right. */
const VANISHING = { x: 800, y: -4400 };

/** How far a thing lying on the table with its top-center at (x, y) leans, in degrees: its
 * sides run toward the vanishing point. */
export const lean = (x: number, y: number) =>
  (Math.atan((x - VANISHING.x) / (y - VANISHING.y)) * 180) / Math.PI;

/** Places a thing `width` wide lying on the table with its top-left corner at (x, y), shrunk by
 * its depth and leaning toward the vanishing point. Cards lie parallel to the screen, so
 * perspective only scales and shears them; applying those directly renders the same in every
 * WebKit compositing mode, headless captures included. */
export function onTable(x: number, y: number, width: number): CSSProperties {
  const scale = depthScale(y);
  return {
    left: x,
    top: y,
    transformOrigin: "0 0",
    transform: `scale(${scale}) skewX(${lean(x + (width * scale) / 2, y)}deg)`,
  };
}

function OnTable({
  x,
  y,
  width,
  children,
}: {
  x: number;
  y: number;
  width: number;
  children: ReactNode;
}) {
  return (
    <div className="on-table" style={onTable(x, y, width)}>
      {children}
    </div>
  );
}

/** A zone lying on the table at a fixed place, its top-left corner at (x, y): it leans like
 * everything on the table and is foreshortened by `squash`. */
function Lying({
  x,
  y,
  width,
  squash = 1,
  children,
}: {
  x: number;
  y: number;
  width: number;
  squash?: number;
  children: ReactNode;
}) {
  return (
    <div
      className="hud"
      style={{
        left: x,
        top: y,
        transformOrigin: "0 0",
        transform: `skewX(${lean(x + width / 2, y)}deg) scaleY(${squash})`,
      }}
    >
      {children}
    </div>
  );
}

/** A permanent on the battlefield, placed on the stage. */
export interface Permanent {
  card: CardFace;
  x: number;
  y: number;
  tapped?: boolean;
  highlight?: Highlight;
  combat?: CombatRole;
  /** For arrows. */
  id?: string;
  /** For art supplied by slot (SlotArt). */
  slot?: string;
}

/** Copies of one land, fanned as a stack. */
export interface LandPile {
  card: CardFace;
  count: number;
  x: number;
  y: number;
  /** How many of them are tapped, from the top. */
  tapped?: number;
  slot?: string;
}

export interface SideView {
  life: number;
  active?: boolean;
  library: number;
  /** Bottom first. */
  graveyard: CardFace[];
  lands: LandPile[];
  creatures: Permanent[];
}

/** An item on the stack, drawn large at the right of the table. */
export interface StackView {
  source: CardFace;
  label?: string;
  text: string;
  x: number;
  y: number;
  width: number;
  id?: string;
  slot?: string;
}

/** Everything the table draws. The life orbs carry the ids `opponent-life` and `you-life` for
 * arrows. */
export interface TableBoard {
  opponent: SideView & { hand: number };
  you: SideView & { hand: { card: CardFace; slot?: string }[] };
  /** The step tracker's current step; no tracker without one. */
  phase?: Phase;
  action: { label: string; caption?: string; disabled?: boolean };
  /** Who has priority, when it is not you. */
  priority?: string;
  /** A step announcement over the battlefield, such as "Combat" / "Blockers declared". */
  banner?: { title: string; detail: string };
  stack?: StackView[];
  arrows?: ArrowSpec[];
}

const CREATURE_WIDTH = 232;
const LAND_WIDTH = 190;
/** Each copy beneath a land stack's top card, as a fraction of the card's width. */
const LAND_STEP = { x: 0.12, y: 0.015 };

function Creature({ permanent }: { permanent: Permanent }) {
  const withArt = useSlotArt();
  const [tapped, setTapped] = useState(permanent.tapped ?? false);
  return (
    <OnTable x={permanent.x} y={permanent.y} width={CREATURE_WIDTH}>
      <div onClick={() => setTapped(!tapped)}>
        <Card
          id={permanent.id}
          card={withArt(permanent.card, permanent.slot)}
          variant="field"
          width={CREATURE_WIDTH}
          tapped={tapped}
          highlight={permanent.highlight}
          combat={permanent.combat}
        />
      </div>
    </OnTable>
  );
}

function Lands({ lands }: { lands: LandPile[] }) {
  const withArt = useSlotArt();
  return lands.map((pile, i) => (
    <OnTable
      key={i}
      x={pile.x}
      y={pile.y}
      width={LAND_WIDTH * (1 + LAND_STEP.x * (pile.count - 1))}
    >
      <LandStack
        card={withArt(pile.card, pile.slot)}
        count={pile.count}
        tapped={pile.tapped}
        width={LAND_WIDTH}
        step={LAND_STEP}
        showCount={false}
      />
    </OnTable>
  ));
}

/** The hand's cards: each card's top center lies on an arc over the near edge, and the fan
 * turns each a little more clockwise than the one on its left, as in the mockups. */
const HAND = {
  width: 173,
  spacing: 172,
  top: 737,
  sag: 6.7,
  turn: 6.3,
  bias: 0.8,
};

/** The hand: a fan held at the near edge. A hovered card straightens and rises. */
function Hand({ cards }: { cards: TableBoard["you"]["hand"] }) {
  const withArt = useSlotArt();
  const mid = (cards.length - 1) / 2;
  return cards.map(({ card, slot }, i) => (
    <motion.div
      key={i}
      className="hand-card"
      style={{
        left: STAGE.width / 2 + (i - mid) * HAND.spacing - HAND.width / 2,
        top: HAND.top + (i - mid) ** 2 * HAND.sag,
        rotate: (i - mid) * HAND.turn + HAND.bias,
        originX: 0.5,
        originY: 0,
        zIndex: 10 + i,
      }}
      whileHover={{ y: -60, rotate: 0, scale: 1.05, zIndex: 30 }}
      transition={{ type: "spring", stiffness: 380, damping: 30 }}
    >
      <Card card={withArt(card, slot)} width={HAND.width} />
    </motion.div>
  ));
}

/** The battlefield and everything around it, without an overlay; `dimmed` behind one. */
export function TableScene({
  board = SAMPLE_BOARD,
  dimmed = false,
}: {
  board?: TableBoard;
  dimmed?: boolean;
}) {
  const withArt = useSlotArt();
  const { opponent, you } = board;
  return (
    <div className={dimmed ? "table table-dimmed" : "table"}>
      <Lands lands={opponent.lands} />
      {[...opponent.creatures, ...you.creatures].map((p, i) => (
        <Creature key={i} permanent={p} />
      ))}
      <Lands lands={you.lands} />

      <div className="hud" style={{ left: 18, top: 62 }}>
        <PlayerBadge name="Opponent" />
      </div>
      <div className="opponent-hand">
        {Array.from({ length: opponent.hand }, (_, i) => {
          const mid = (opponent.hand - 1) / 2;
          return (
            <CardBack
              key={i}
              width={105}
              style={{
                transform: `rotate(${(i - mid) * -4.2}deg) translateY(${-((i - mid) ** 2) * 5.5}px)`,
              }}
            />
          );
        })}
      </div>
      <div id="opponent-life" className="hud" style={{ left: 670, top: 48 }}>
        <LifeOrb
          life={opponent.life}
          active={opponent.active}
          seat="opponent"
        />
      </div>
      <Lying x={1174} y={13} width={122} squash={0.85}>
        <Library
          count={opponent.library}
          width={93}
          depth={{ x: -29, y: 37, flare: 32 }}
        />
      </Lying>
      {opponent.graveyard.length > 0 && (
        <Lying x={1330} y={36.5} width={88} squash={0.92}>
          <Graveyard
            cards={opponent.graveyard.map((c, i, all) =>
              i === all.length - 1 ? withArt(c, "opponent-graveyard") : c,
            )}
            width={88}
            depth={{ x: -5, y: 6 }}
          />
        </Lying>
      )}
      <svg
        className="gear"
        viewBox="0 0 24 24"
        aria-label="Settings"
        role="img"
      >
        <circle className="gear-hole" cx="12" cy="12" r="4.9" />
        <path d="M19.67 9.71 L22.34 9.69 L22.34 14.31 L19.67 14.29 L19.04 15.80 L20.95 17.68 L17.68 20.95 L15.80 19.04 L14.29 19.67 L14.31 22.34 L9.69 22.34 L9.71 19.67 L8.20 19.04 L6.32 20.95 L3.05 17.68 L4.96 15.80 L4.33 14.29 L1.66 14.31 L1.66 9.69 L4.33 9.71 L4.96 8.20 L3.05 6.32 L6.32 3.05 L8.20 4.96 L9.71 4.33 L9.69 1.66 L14.31 1.66 L14.29 4.33 L15.80 4.96 L17.68 3.05 L20.95 6.32 L19.04 8.20 Z M16.9 12 A4.9 4.9 0 1 0 7.1 12 A4.9 4.9 0 1 0 16.9 12 Z" />
      </svg>

      {board.banner && (
        <div className="step-banner">
          <i className="step-banner-rule step-banner-rule-left" />
          <i className="step-banner-diamond" />
          <span className="step-banner-title">{board.banner.title}</span>
          <i className="step-banner-dot" />
          <i className="step-banner-rule step-banner-rule-right" />
          <i className="step-banner-underline" />
          <span className="step-banner-detail">{board.banner.detail}</span>
          <i className="step-banner-notch" />
        </div>
      )}
      {board.stack?.map((item, i) => (
        <div key={i} className="hud" style={{ left: item.x, top: item.y }}>
          <StackItem
            id={item.id}
            source={withArt(item.source, item.slot)}
            label={item.label}
            text={item.text}
            width={item.width}
            showArt
          />
        </div>
      ))}

      <div id="you-life" className="hud" style={{ left: 670, top: 618 }}>
        <LifeOrb life={you.life} active={you.active} />
      </div>
      <Hand cards={you.hand} />
      <div className="hud" style={{ left: 18, top: 814 }}>
        <PlayerBadge name="You" />
      </div>
      <Lying x={168} y={731} width={116}>
        <Library
          count={you.library}
          width={104}
          depth={{ x: 6, y: 12, twist: 7 }}
        />
      </Lying>
      <div
        className={
          board.priority ? "turn-controls turn-controls-wait" : "turn-controls"
        }
      >
        {board.phase && <PhaseTracker current={board.phase} compact />}
        {board.priority && <PriorityPill>{board.priority}</PriorityPill>}
        <div className="turn-action">
          <Button
            icon={<FastForwardIcon />}
            disabled={board.action.disabled}
            className={
              board.action.label.length > 8 ? "button-long" : undefined
            }
          >
            {board.action.label}
          </Button>
          {board.action.caption && (
            <span className="turn-caption">{board.action.caption}</span>
          )}
        </div>
      </div>
      {board.arrows && <Arrows arrows={board.arrows} />}
    </div>
  );
}

export function TableScreen({
  board,
  overlay,
}: {
  board?: TableBoard;
  overlay?: "scry";
}) {
  return (
    <Stage>
      <TableScene board={board} dimmed={overlay !== undefined} />
      {overlay === "scry" && <ScryOverlay />}
      {/* Above any overlay: scry.png keeps the brand crisp over the dimmed table. */}
      <div className="brand">
        <Crest className="brand-crest" />
        <span>Project Beryl</span>
      </div>
    </Stage>
  );
}
