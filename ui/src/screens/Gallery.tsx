// The component library on one page, laid out as docs/ui_mockups/ui_system.png. `just ui-shots`
// captures it; it is also the place to look at a component while changing it.
import { useMemo, type ReactNode } from "react";
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
import { SAMPLE } from "./samples";
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
            cards={[SAMPLE.counterspell, SAMPLE.doomBlade]}
            width={100}
          />
          <Caption>Graveyard</Caption>
        </figure>
        <figure>
          <LandStack card={SAMPLE.island} count={3} width={130} />
          <Caption>Lands (stacked)</Caption>
        </figure>
      </div>
    </Panel>
  );
}

function CardsAndStates() {
  return (
    <Panel n="04" title="Cards & states" className="panel-cards">
      <div className="states-grid">
        <Card card={SAMPLE.infernoTitan} variant="field" width={150} />
        <figure>
          <Card card={SAMPLE.mountain} variant="field" width={100} tapped />
          <Caption>Tapped</Caption>
        </figure>
        <figure>
          <Card card={SAMPLE.plains} variant="field" width={100} />
          <Caption>Untapped</Caption>
        </figure>
      </div>
      <div className="states-grid states-glow">
        <figure>
          <Card
            card={SAMPLE.balefulStrix}
            variant="field"
            width={140}
            highlight="source"
          />
          <Caption>Source (gold)</Caption>
        </figure>
        <figure>
          <Card
            card={SAMPLE.balefulStrix}
            variant="field"
            width={140}
            highlight="target"
          />
          <Caption>Target (cyan)</Caption>
        </figure>
      </div>
      <div className="mini-hand">
        <Card card={SAMPLE.swords} width={120} style={{ rotate: -5 }} />
        <Card card={SAMPLE.mountain} width={120} />
        <Card card={SAMPLE.plateau} width={120} style={{ rotate: 5, y: 6 }} />
      </div>
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
        <div className="combat-row">
          <figure>
            <Card
              id="g-attacker"
              card={SAMPLE.beast}
              variant="field"
              width={125}
              combat="attacker"
            />
            <Caption>Attacker · angular</Caption>
          </figure>
          <figure>
            <Card
              id="g-blocker"
              card={SAMPLE.angel}
              variant="field"
              width={125}
              combat="blocker"
            />
            <Caption>Blocker · rounded</Caption>
          </figure>
        </div>
        <div className="panel-rule" />
        <div className="stack-row">
          <Card
            id="g-titan"
            card={SAMPLE.infernoTitan}
            variant="field"
            width={112}
          />
          <StackItem
            id="g-trigger"
            source={SAMPLE.infernoTitan}
            text="When Inferno Titan enters the battlefield, it deals 3 damage divided as you choose among one, two, or three targets."
            width={128}
          />
          <Card id="g-beast" card={SAMPLE.beast} variant="field" width={104} />
        </div>
        <div className="stack-captions">
          <Caption>Gold: source → ability</Caption>
          <Caption>Cyan: ability → target</Caption>
        </div>
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
          <TableScene dimmed />
          <ScryOverlay />
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
