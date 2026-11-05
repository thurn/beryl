// The component library on one page, laid out as docs/ui_mockups/ui_system.png. `just ui-shots`
// captures it; it is also the place to look at a component while changing it.
import { useMemo, type ReactNode } from "react";
import { Card, Crest, useSlotArt } from "../components/Card";
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
import { TableScene } from "./Table";

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
    <section className={["panel", className].filter(Boolean).join(" ")}>
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

const SWATCHES = [
  ["Table", "#625C56", "var(--table)"],
  ["Surface", "#242321", "var(--surface)"],
  ["Text", "#F2ECE1", "var(--text)"],
  ["Gold", "#E9AD4A", "var(--gold)"],
  ["Cyan", "#38DCFF", "var(--cyan)"],
] as const;

function Foundations() {
  return (
    <Panel n="01" title="Foundations">
      <div className="swatches">
        {SWATCHES.map(([name, hex, color]) => (
          <figure key={name} className="swatch">
            <div className="swatch-chip" style={{ background: color }} />
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
  const a = useSlotArt();
  return (
    <Panel n="03" title="Player & zones">
      <div className="player-row">
        <figure>
          <LifeOrb life={22} active />
          <Caption>Active</Caption>
        </figure>
        <figure>
          <LifeOrb life={17} />
          <Caption>Inactive</Caption>
        </figure>
        <div className="specimen-divider" />
        <PlayerBadge name="Opponent" />
      </div>
      <div className="panel-rule" />
      <div className="zone-row">
        <figure>
          <Library count={40} width={100} />
          <Caption>Library</Caption>
        </figure>
        <figure>
          <Graveyard
            cards={[SAMPLE.counterspell, a(SAMPLE.doomBlade, "03-graveyard")]}
            width={100}
          />
          <Caption>Graveyard</Caption>
        </figure>
        <figure>
          <LandStack
            card={a(SAMPLE.island, "03-island")}
            count={3}
            width={130}
          />
          <Caption>Lands (stacked)</Caption>
        </figure>
      </div>
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
  const a = useSlotArt();
  return (
    <Panel n="04" title="Cards & states">
      <At x={24} y={55}>
        <Card
          card={a(SAMPLE.infernoTitan, "04-titan")}
          variant="field"
          width={152}
          height={135}
          unit={14.6}
        />
      </At>
      <At x={194} y={80}>
        <Card
          card={a(SAMPLE.mountain, "04-tapped")}
          variant="field"
          width={126}
          height={92}
          unit={13}
          tapped
        />
      </At>
      <At x={337} y={61}>
        <Card
          card={a(SAMPLE.plains, "04-untapped")}
          variant="field"
          width={101}
          height={109}
          unit={12}
        />
      </At>
      <CaptionAt x={263} y={176}>
        Tapped
      </CaptionAt>
      <CaptionAt x={389} y={176}>
        Untapped
      </CaptionAt>
      <At x={64} y={215}>
        <Card
          card={a(SAMPLE.balefulStrix, "04-source")}
          variant="field"
          width={148}
          height={100}
          unit={13.6}
          highlight="source"
        />
      </At>
      <At x={266} y={215}>
        <Card
          card={a(SAMPLE.balefulStrix, "04-target")}
          variant="field"
          width={146}
          height={100}
          unit={13.6}
          highlight="target"
        />
      </At>
      <CaptionAt x={139} y={322}>
        Source (gold)
      </CaptionAt>
      <CaptionAt x={344} y={322}>
        Target (cyan)
      </CaptionAt>
      <div className="mini-hand">
        <At x={48} y={368}>
          <Card
            card={a(costless(SAMPLE.swords), "04-hand-swords")}
            width={124}
            unit={13}
            style={{ rotate: -5 }}
          />
        </At>
        <At x={170} y={356}>
          <Card
            card={a(SAMPLE.mountain, "04-hand-mountain")}
            width={120}
            unit={13}
          />
        </At>
        <At x={280} y={358}>
          <Card
            card={a(SAMPLE.plateau, "04-hand-plateau")}
            width={124}
            unit={13}
            style={{ rotate: 5 }}
          />
        </At>
      </div>
    </Panel>
  );
}

function CombatAndStack() {
  const a = useSlotArt();
  const arrows = useMemo<ArrowSpec[]>(
    () => [
      {
        from: "g-attacker",
        to: "g-blocker",
        tone: "gold",
        link: true,
        bend: 0,
        fromSide: "right",
        toSide: "left",
      },
      {
        from: "g-titan",
        to: "g-trigger",
        tone: "gold",
        bend: -0.3,
        fromSide: "right",
        toSide: "left",
      },
      {
        from: "g-trigger",
        to: "g-beast",
        tone: "cyan",
        bend: 0.3,
        fromSide: "right",
        toSide: "left",
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
            card={a(SAMPLE.beast, "05-attacker")}
            variant="field"
            width={129}
            height={100}
            unit={12.6}
            combat="attacker"
          />
        </At>
        <At x={307} y={72}>
          <Card
            id="g-blocker"
            card={a(SAMPLE.angel, "05-blocker")}
            variant="field"
            width={126}
            height={101}
            unit={12.6}
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
            card={a(SAMPLE.infernoTitan, "05-source")}
            variant="field"
            width={119}
            height={104}
            unit={12.6}
          />
        </At>
        <At x={182} y={241}>
          <StackItem
            id="g-trigger"
            source={SAMPLE.infernoTitan}
            text="When Inferno Titan enters the battlefield, it deals 3 damage divided as you choose among one, two, or three targets."
            width={130}
            height={172}
            unit={17}
          />
        </At>
        <At x={356} y={267}>
          <Card
            id="g-beast"
            card={a(SAMPLE.beast, "05-target")}
            variant="field"
            width={116}
            height={105}
            unit={12.6}
          />
        </At>
        <CaptionAt x={121} y={422}>
          Gold: source → ability
        </CaptionAt>
        <CaptionAt x={367} y={422}>
          Cyan: ability → target
        </CaptionAt>
        <Arrows arrows={arrows} />
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
            <TableScene dimmed />
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
        <Caption>Dimmed battlefield backdrop</Caption>
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
      <div className="gallery-grid">
        <Foundations />
        <ActionsAndStatus />
        <PlayerAndZones />
        <CardsAndStates />
        <CombatAndStack />
        <ScryPanel />
      </div>
    </main>
  );
}
