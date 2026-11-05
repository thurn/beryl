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

/** Places a thing lying on the table with its top-left corner at (x, y), shrunk by its depth.
 * Cards lie parallel to the screen, so perspective only scales them; applying that scale
 * directly renders the same in every WebKit compositing mode, headless captures included. */
export function onTable(x: number, y: number): CSSProperties {
  return {
    left: x,
    top: y,
    transformOrigin: "0 0",
    transform: `scale(${depthScale(y)})`,
  };
}

function OnTable({
  x,
  y,
  children,
}: {
  x: number;
  y: number;
  children: ReactNode;
}) {
  return (
    <div className="on-table" style={onTable(x, y)}>
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

function Creature({ permanent }: { permanent: Permanent }) {
  const withArt = useSlotArt();
  const [tapped, setTapped] = useState(permanent.tapped ?? false);
  return (
    <OnTable x={permanent.x} y={permanent.y}>
      <div onClick={() => setTapped(!tapped)}>
        <Card
          id={permanent.id}
          card={withArt(permanent.card, permanent.slot)}
          variant="field"
          width={227}
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
    <OnTable key={i} x={pile.x} y={pile.y}>
      <LandStack
        card={withArt(pile.card, pile.slot)}
        count={pile.count}
        tapped={pile.tapped}
        width={192}
        showCount={false}
      />
    </OnTable>
  ));
}

/** The hand: a fan held at the near edge, leaning back toward the table; each card overlaps the
 * one on its left. A hovered card straightens and rises. */
function Hand({ cards }: { cards: TableBoard["you"]["hand"] }) {
  const withArt = useSlotArt();
  const mid = (cards.length - 1) / 2;
  return (
    <div className="hand">
      {cards.map(({ card, slot }, i) => (
        <motion.div
          key={i}
          className="hand-card"
          style={{
            rotate: (i - mid) * 3.5,
            y: Math.abs(i - mid) ** 2 * 6,
            rotateX: 14,
            transformPerspective: 900,
            originY: 1,
            zIndex: i,
          }}
          whileHover={{ y: -60, rotateX: 0, scale: 1.05, zIndex: 20 }}
          transition={{ type: "spring", stiffness: 380, damping: 30 }}
        >
          <Card card={withArt(card, slot)} width={182} />
        </motion.div>
      ))}
    </div>
  );
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

      <div className="brand">
        <Crest className="brand-crest" />
        <span>Project Beryl</span>
      </div>
      <div className="hud" style={{ left: 18, top: 62 }}>
        <PlayerBadge name="Opponent" />
      </div>
      <div className="opponent-hand">
        {Array.from({ length: opponent.hand }, (_, i) => {
          const mid = (opponent.hand - 1) / 2;
          return (
            <CardBack
              key={i}
              width={112}
              style={{
                transform: `perspective(700px) rotateX(-12deg) rotate(${180 + (i - mid) * 3}deg) translateY(${-((i - mid) ** 2) * 4}px)`,
              }}
            />
          );
        })}
      </div>
      <div id="opponent-life" className="hud" style={{ left: 670, top: 40 }}>
        <LifeOrb life={opponent.life} active={opponent.active} />
      </div>
      <div className="hud" style={{ left: 1195, top: 14 }}>
        <Library count={opponent.library} width={98} />
      </div>
      {opponent.graveyard.length > 0 && (
        <div className="hud" style={{ left: 1330, top: 40 }}>
          <Graveyard
            cards={opponent.graveyard.map((c, i, all) =>
              i === all.length - 1 ? withArt(c, "opponent-graveyard") : c,
            )}
            width={96}
          />
        </div>
      )}
      <svg
        className="gear"
        viewBox="0 0 24 24"
        aria-label="Settings"
        role="img"
      >
        <path d="M10.3 2h3.4l.5 2.6 1.9.8 2.2-1.5 2.4 2.4-1.5 2.2.8 1.9 2.6.5v3.4l-2.6.5-.8 1.9 1.5 2.2-2.4 2.4-2.2-1.5-1.9.8-.5 2.6h-3.4l-.5-2.6-1.9-.8-2.2 1.5-2.4-2.4 1.5-2.2-.8-1.9L2 13.7v-3.4l2.6-.5.8-1.9-1.5-2.2 2.4-2.4 2.2 1.5 1.9-.8zM12 8.5a3.5 3.5 0 1 0 0 7 3.5 3.5 0 0 0 0-7z" />
      </svg>

      {board.banner && (
        <div className="step-banner">
          <span className="step-banner-title">{board.banner.title}</span>
          <span className="step-banner-detail">{board.banner.detail}</span>
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

      <div id="you-life" className="hud" style={{ left: 670, top: 612 }}>
        <LifeOrb life={you.life} active={you.active} />
      </div>
      <Hand cards={you.hand} />
      <div className="hud" style={{ left: 160, top: 730 }}>
        <Library count={you.library} width={110} />
      </div>
      <div className="hud" style={{ left: 18, top: 815 }}>
        <PlayerBadge name="You" />
      </div>
      <div className="hud turn-controls">
        {board.phase && <PhaseTracker current={board.phase} compact />}
        {board.priority && <PriorityPill>{board.priority}</PriorityPill>}
        <Button icon={<FastForwardIcon />} disabled={board.action.disabled}>
          {board.action.label}
        </Button>
        {board.action.caption && (
          <span className="turn-caption">{board.action.caption}</span>
        )}
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
    </Stage>
  );
}
