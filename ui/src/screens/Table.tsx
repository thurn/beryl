// The pseudo-3D table, laid out as docs/ui_mockups/battlefield.png. Permanents lie at a depth
// that grows toward the opponent, so they stay upright rectangles that shrink with distance
// (depthScale), with a slab edge and a cast shadow under each; the hand leans back toward the
// table through CSS perspective. No WebGL. The scene is drawn on a fixed 1600×900 stage scaled to
// fit the window.
import { useEffect, useState, type CSSProperties, type ReactNode } from "react";
import { motion } from "motion/react";
import { Card, CardBack, Crest, type CardFace } from "../components/Card";
import { Button, FastForwardIcon, PhaseTracker } from "../components/Controls";
import {
  Graveyard,
  LandStack,
  Library,
  LifeOrb,
  PlayerBadge,
} from "../components/Player";
import { SAMPLE } from "./samples";
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
const DEPTH_PER_PX = 0.35;

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

function Creature({ card, x, y }: { card: CardFace; x: number; y: number }) {
  const [tapped, setTapped] = useState(false);
  return (
    <OnTable x={x} y={y}>
      <div onClick={() => setTapped(!tapped)}>
        <Card card={card} variant="field" width={215} tapped={tapped} />
      </div>
    </OnTable>
  );
}

/** The hand: a fan held at the near edge, leaning back toward the table; each card overlaps the
 * one on its left. A hovered card straightens and rises. */
function Hand({ cards }: { cards: CardFace[] }) {
  const mid = (cards.length - 1) / 2;
  return (
    <div className="hand">
      {cards.map((card, i) => (
        <motion.div
          key={card.name}
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
          <Card card={card} width={182} />
        </motion.div>
      ))}
    </div>
  );
}

/** The battlefield and everything around it, without an overlay; `dimmed` behind one. */
export function TableScene({ dimmed = false }: { dimmed?: boolean }) {
  return (
    <div className={dimmed ? "table table-dimmed" : "table"}>
      <OnTable x={150} y={127}>
        <LandStack card={SAMPLE.island} count={3} width={186} />
      </OnTable>
      <OnTable x={372} y={122}>
        <LandStack card={SAMPLE.swamp} count={3} width={186} />
      </OnTable>
      <Creature card={SAMPLE.balefulStrix} x={608} y={223} />
      <Creature card={SAMPLE.beast} x={810} y={223} />
      <Creature card={SAMPLE.goblinGuide} x={589} y={421} />
      <Creature card={SAMPLE.angel} x={813} y={421} />
      <OnTable x={104} y={566}>
        <LandStack card={SAMPLE.mountain} count={3} width={186} />
      </OnTable>
      <OnTable x={338} y={564}>
        <LandStack card={SAMPLE.plains} count={3} width={186} />
      </OnTable>

      <div className="brand">
        <Crest className="brand-crest" />
        <span>Project Beryl</span>
      </div>
      <div className="hud" style={{ left: 18, top: 62 }}>
        <PlayerBadge name="Opponent" />
      </div>
      <div className="opponent-hand">
        {Array.from({ length: 5 }, (_, i) => (
          <CardBack
            key={i}
            width={112}
            style={{
              transform: `perspective(700px) rotateX(-12deg) rotate(${180 + (i - 2) * 3}deg) translateY(${-(Math.abs(i - 2) ** 2) * 4}px)`,
            }}
          />
        ))}
      </div>
      <div className="hud" style={{ left: 705, top: 50 }}>
        <LifeOrb life={17} />
      </div>
      <div className="hud" style={{ left: 1195, top: 14 }}>
        <Library count={33} width={98} />
      </div>
      <div className="hud" style={{ left: 1330, top: 40 }}>
        <Graveyard cards={[SAMPLE.doomBlade]} width={84} />
      </div>
      <svg
        className="gear"
        viewBox="0 0 24 24"
        aria-label="Settings"
        role="img"
      >
        <path d="M10.3 2h3.4l.5 2.6 1.9.8 2.2-1.5 2.4 2.4-1.5 2.2.8 1.9 2.6.5v3.4l-2.6.5-.8 1.9 1.5 2.2-2.4 2.4-2.2-1.5-1.9.8-.5 2.6h-3.4l-.5-2.6-1.9-.8-2.2 1.5-2.4-2.4 1.5-2.2-.8-1.9L2 13.7v-3.4l2.6-.5.8-1.9-1.5-2.2 2.4-2.4 2.2 1.5 1.9-.8zM12 8.5a3.5 3.5 0 1 0 0 7 3.5 3.5 0 0 0 0-7z" />
      </svg>

      <div className="hud" style={{ left: 705, top: 622 }}>
        <LifeOrb life={22} active />
      </div>
      <Hand
        cards={[SAMPLE.bolt, SAMPLE.swords, SAMPLE.mountain, SAMPLE.plateau]}
      />
      <div className="hud" style={{ left: 160, top: 730 }}>
        <Library count={26} width={110} />
      </div>
      <div className="hud" style={{ left: 18, top: 815 }}>
        <PlayerBadge name="You" />
      </div>
      <div className="hud turn-controls">
        <PhaseTracker current="main" compact />
        <Button icon={<FastForwardIcon />}>Next</Button>
        <span className="turn-caption">To Combat</span>
      </div>
    </div>
  );
}

export function TableScreen({ overlay }: { overlay?: "scry" }) {
  return (
    <Stage>
      <TableScene dimmed={overlay !== undefined} />
      {overlay === "scry" && <ScryOverlay />}
    </Stage>
  );
}
