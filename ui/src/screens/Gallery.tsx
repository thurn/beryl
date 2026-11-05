// The component library on one page, laid out as docs/ui_mockups/ui_system.png. `just ui-shots`
// captures it; it is also the place to look at a component while changing it.
import { useMemo, type CSSProperties, type ReactNode } from "react";
import { Card, Crest } from "../components/Card";
import {
  Button,
  FastForwardIcon,
  PhaseTracker,
  PriorityPill,
} from "../components/Controls";
import {
  Graveyard,
  LandStack,
  Library,
  LifeOrb,
  PlayerBadge,
} from "../components/Player";
import { Arrows, StackItem, type ArrowSpec } from "../components/Stack";
import { costless, SAMPLE } from "./samples";
import { ScryOverlay } from "./Scry";

function Panel({
  n,
  title,
  children,
  className,
}: {
  n: string;
  title: string;
  children: ReactNode;
  className?: string;
}) {
  return (
    <section
      className={["panel", `panel-${n}`, className].filter(Boolean).join(" ")}
    >
      <h2 className="panel-title">
        <span className="panel-number">{n}</span> {title}
      </h2>
      {children}
    </section>
  );
}

function Caption({ children }: { children: ReactNode }) {
  return <span className="caption">{children}</span>;
}

// The tokens' values (design/tokens.css), measured from the mockups as they render.
const SWATCHES = [
  ["Table", "#6B5F56", "var(--table)"],
  ["Surface", "#211F1C", "var(--surface)"],
  ["Text", "#F2E7D8", "var(--text)"],
  ["Gold", "#E4A242", "var(--gold)"],
  ["Cyan", "#33DCFE", "var(--cyan)"],
] as const;

function Foundations() {
  return (
    <Panel n="01" title="Foundations">
      <div className="swatches">
        {SWATCHES.map(([name, hex, color]) => (
          <figure key={name} className="swatch">
            <div
              className="swatch-chip"
              style={{ "--chip": color } as CSSProperties}
            />
            <figcaption>
              {name}
              <small>{hex}</small>
            </figcaption>
          </figure>
        ))}
      </div>
      <div className="type-specimens">
        <div className="specimen">
          <span className="specimen-serif">Scry</span>
          <Caption>Serif · titles and actions</Caption>
        </div>
        <div className="specimen-divider" />
        <div className="specimen">
          <span className="specimen-sans">Drag cards to reorder</span>
          <Caption>Sans serif · instructions</Caption>
        </div>
      </div>
      <div className="material-specimens">
        <figure>
          <div className="tabletop-chip" />
          <Caption>Tabletop (matte)</Caption>
        </figure>
        <figure>
          <div className="metal-chip">
            <i className="metal-gold" />
            <i className="metal-silver" />
          </div>
          <Caption>Thin metallic borders</Caption>
        </figure>
      </div>
    </Panel>
  );
}

function ActionsAndStatus() {
  return (
    <Panel n="02" title="Actions & status">
      <div className="button-rows">
        <Button>Done</Button>
        <Caption>Primary</Caption>
        <Button variant="secondary">View Battlefield</Button>
        <Caption>Secondary</Caption>
        <Button disabled icon={<FastForwardIcon />}>
          Pass
        </Button>
        <Caption>Disabled</Caption>
      </div>
      <div className="panel-rule" />
      <div className="centered">
        <PriorityPill>Opponent has priority</PriorityPill>
      </div>
      <div className="panel-rule" />
      <div className="centered">
        <PhaseTracker current="combat" />
      </div>
    </Panel>
  );
}

function PlayerAndZones() {
  return (
    <Panel n="03" title="Player & zones">
      <At x={0} y={50}>
        <LifeOrb life={22} active wings={0.7} />
      </At>
      <At x={141} y={51}>
        <LifeOrb life={17} seat="opponent" wings={0.7} />
      </At>
      <CaptionAt x={98} y={133}>
        Active
      </CaptionAt>
      <CaptionAt x={238} y={133}>
        Inactive
      </CaptionAt>
      <At x={306} y={60}>
        <div className="specimen-divider" style={{ height: 92 }} />
      </At>
      <At x={324} y={70}>
        <PlayerBadge name="Opponent" />
      </At>
      <div className="panel-rule" />
      <At x={22} y={186}>
        <Library count={40} width={94} depth={{ x: -25, y: 18 }} />
      </At>
      <At x={171} y={191}>
        <Graveyard
          cards={[
            SAMPLE.counterspell,
            SAMPLE.counterspell,
            SAMPLE.counterspell,
            SAMPLE.doomBlade,
          ]}
          width={98}
          depth={{ x: -14, y: 18 }}
        />
      </At>
      <At x={318} y={192}>
        <div style={{ transform: "skewX(-3deg)" }}>
          <LandStack
            card={SAMPLE.island}
            count={3}
            width={122}
            step={{ x: 0.13, y: 0.065 }}
          />
        </div>
      </At>
      <CaptionAt x={92} y={343}>
        Library
      </CaptionAt>
      <CaptionAt x={237} y={343}>
        Graveyard
      </CaptionAt>
      <CaptionAt x={407} y={343}>
        Lands (stacked)
      </CaptionAt>
    </Panel>
  );
}

/** Places a part at (x, y) in its panel's padding box, where the mockup puts it. */
function At({ x, y, children }: { x: number; y: number; children: ReactNode }) {
  return (
    <div className="at" style={{ left: x, top: y }}>
      {children}
    </div>
  );
}

/** A caption centered on `x`. */
function CaptionAt({
  x,
  y,
  children,
}: {
  x: number;
  y: number;
  children: ReactNode;
}) {
  return (
    <div className="at caption-at" style={{ left: x - 120, top: y }}>
      <Caption>{children}</Caption>
    </div>
  );
}

function CardsAndStates() {
  return (
    <Panel n="04" title="Cards & states">
      <At x={24} y={55}>
        <Card card={SAMPLE.infernoTitan} variant="field" width={152} />
      </At>
      {/* Shrunk as the table shrinks a tapped card, to stand as tall as it lay untapped. */}
      <At x={226} y={70}>
        <Card card={SAMPLE.mountain} variant="field" width={97} tapped />
      </At>
      <At x={337} y={61}>
        <Card card={SAMPLE.plains} variant="field" width={101} />
      </At>
      <CaptionAt x={263} y={176}>
        Tapped
      </CaptionAt>
      <CaptionAt x={389} y={176}>
        Untapped
      </CaptionAt>
      <At x={64} y={200}>
        <Card
          card={SAMPLE.balefulStrix}
          variant="field"
          width={148}
          highlight="source"
        />
      </At>
      <At x={266} y={200}>
        <Card
          card={SAMPLE.balefulStrix}
          variant="field"
          width={146}
          highlight="target"
        />
      </At>
      <CaptionAt x={139} y={325}>
        Source (gold)
      </CaptionAt>
      <CaptionAt x={344} y={325}>
        Target (cyan)
      </CaptionAt>
      <At x={54} y={347}>
        <Card
          card={costless(SAMPLE.swords)}
          width={128}
          style={{ rotate: -7 }}
        />
      </At>
      <At x={168} y={358}>
        <Card card={SAMPLE.mountain} width={124} />
      </At>
      <At x={274} y={367}>
        <Card card={SAMPLE.plateau} width={128} style={{ rotate: 7 }} />
      </At>
    </Panel>
  );
}

function CombatAndStack() {
  const arrows = useMemo<ArrowSpec[]>(
    () => [
      {
        from: "g-attacker",
        to: "g-blocker",
        tone: "gold",
        end: "dot",
        bend: 0,
        fromSide: "right",
        toSide: "left",
      },
      {
        from: "g-titan",
        to: "g-trigger",
        tone: "gold",
        fromSide: "right",
        toSide: { x: 0.02, y: 0.64 },
        leave: [24, 0],
        arrive: [24, 0],
      },
      {
        from: "g-trigger",
        to: "g-beast",
        tone: "cyan",
        fromSide: { x: 1, y: 0.55 },
        toSide: { x: 0, y: 0.51 },
        leave: [14, -20],
        arrive: [18, 0],
      },
    ],
    [],
  );
  return (
    <Panel n="05" title="Combat & stack">
      <div className="combat-stack">
        <At x={57} y={74}>
          <Card
            id="g-attacker"
            card={SAMPLE.beast}
            variant="field"
            width={129}
            combat="attacker"
          />
        </At>
        <At x={307} y={72}>
          <Card
            id="g-blocker"
            card={SAMPLE.angel}
            variant="field"
            width={126}
            combat="blocker"
          />
        </At>
        <CaptionAt x={122} y={192}>
          Attacker · angular
        </CaptionAt>
        <CaptionAt x={370} y={192}>
          Blocker · rounded
        </CaptionAt>
        <div className="panel-rule" />
        <At x={20} y={268}>
          <Card
            id="g-titan"
            card={SAMPLE.infernoTitan}
            variant="field"
            width={119}
          />
        </At>
        <At x={180} y={241}>
          <StackItem
            id="g-trigger"
            source={SAMPLE.infernoTitan}
            text="When Inferno Titan enters the battlefield, it deals 3 damage divided as you choose among one, two, or three targets."
            width={136}
            height={172}
            unit={16}
          />
        </At>
        <At x={356} y={267}>
          <Card id="g-beast" card={SAMPLE.beast} variant="field" width={116} />
        </At>
        <CaptionAt x={121} y={422}>
          Gold: source → ability
        </CaptionAt>
        <CaptionAt x={367} y={422}>
          Cyan: ability → target
        </CaptionAt>
        <Arrows arrows={arrows} scale={0.65} />
      </div>
    </Panel>
  );
}

function ScryPanel() {
  return (
    <Panel n="06" title="Scry overlay" className="panel-scry">
      <div className="scry-preview">
        <div className="scry-preview-stage">
          <div className="scry-preview-inner">
            <div className="table table-dimmed" />
            <ScryOverlay />
          </div>
        </div>
      </div>
      <div className="scry-captions">
        <Caption>Drag & reorder</Caption>
        <Caption>Source card</Caption>
        <Caption>Confirm</Caption>
      </div>
      <div className="centered">
        <Caption>Dimmed table backdrop</Caption>
      </div>
    </Panel>
  );
}

export function Gallery() {
  return (
    <main className="gallery">
      <header className="gallery-header">
        <div className="wordmark">
          <i className="wordmark-rule" />
          <Crest className="wordmark-crest" />
          <h1>Project Beryl</h1>
          <i className="wordmark-rule" />
        </div>
        <p className="gallery-subtitle">Game UI · Component Library</p>
      </header>
      <Foundations />
      <ActionsAndStatus />
      <PlayerAndZones />
      <CardsAndStates />
      <CombatAndStack />
      <ScryPanel />
    </main>
  );
}
