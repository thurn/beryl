// The component library on one page, laid out as docs/ui_mockups/ui_system.png. `just ui-shots`
// captures it; it is also the place to look at a component while changing it.
import { useState, type CSSProperties, type ReactNode } from "react";
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
import { StackItem } from "../components/Stack";
import { AskPrompt } from "../table/AskPrompt";
import { CommandList, type CommandListEntry } from "../table/CommandList";
import { Menu, type MenuProps } from "../table/Menu";
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
        <LifeOrb life={22} active />
      </At>
      <At x={141} y={51}>
        <LifeOrb life={17} seat="opponent" />
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
      <At x={340} y={200}>
        <div style={{ transform: "skewX(-3deg)" }}>
          <LandStack
            card={SAMPLE.island}
            count={3}
            width={86}
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
      <At x={40} y={55}>
        <Card card={SAMPLE.infernoTitan} variant="field" width={82} />
      </At>
      <At x={204} y={78}>
        <Card card={SAMPLE.mountain} variant="field" width={70} tapped />
      </At>
      <At x={354} y={60}>
        <Card card={SAMPLE.plains} variant="field" width={70} />
      </At>
      <CaptionAt x={263} y={176}>
        Tapped
      </CaptionAt>
      <CaptionAt x={389} y={176}>
        Untapped
      </CaptionAt>
      <At x={98} y={200}>
        <Card
          card={SAMPLE.balefulStrix}
          variant="field"
          width={82}
          highlight="source"
        />
      </At>
      <At x={303} y={200}>
        <Card
          card={SAMPLE.balefulStrix}
          variant="field"
          width={82}
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
          style={{ rotate: "-7deg" }}
        />
      </At>
      <At x={168} y={358}>
        <Card card={SAMPLE.mountain} width={124} />
      </At>
      <At x={274} y={367}>
        <Card card={SAMPLE.plateau} width={128} style={{ rotate: "7deg" }} />
      </At>
    </Panel>
  );
}

function CombatAndStack() {
  return (
    <Panel n="05" title="Combat & stack">
      <div className="combat-stack">
        <At x={80} y={66}>
          <Card
            card={SAMPLE.beast}
            variant="field"
            width={80}
            combat="attacker"
          />
        </At>
        <At x={330} y={60}>
          <Card
            card={SAMPLE.angel}
            variant="field"
            width={80}
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
        <At x={44} y={270}>
          <Card card={SAMPLE.infernoTitan} variant="field" width={78} />
        </At>
        <At x={180} y={241}>
          <StackItem
            source={SAMPLE.infernoTitan}
            text="When Cinder Colossus enters the battlefield, it deals 3 damage divided as you choose among one, two, or three targets."
            width={136}
            height={172}
            unit={16}
          />
        </At>
        <At x={360} y={270}>
          <Card card={SAMPLE.beast} variant="field" width={78} />
        </At>
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

function AskExample({
  name,
  children,
}: {
  name: string;
  children: (close: () => void, autoFocus: boolean) => ReactNode;
}) {
  const [state, setState] = useState<"preview" | "closed" | "open">("preview");
  return (
    <div className="gallery-ask">
      {state !== "preview" && (
        <Button variant="secondary" onClick={() => setState("open")}>
          {name}
        </Button>
      )}
      {state !== "closed" &&
        children(() => setState("closed"), state === "open")}
    </div>
  );
}

function TablePrompts() {
  return (
    <Panel n="07" title="Manual table prompts">
      <div className="gallery-asks">
        <AskExample name="Draw cards">
          {(close, autoFocus) => (
            <AskPrompt
              kind="number"
              fieldLabel="Cards to draw"
              start={1}
              min={1}
              max={60}
              label={(value) => `Draw ${value}`}
              autoFocus={autoFocus}
              onConfirm={close}
              onCancel={close}
            />
          )}
        </AskExample>
        <AskExample name="Set note">
          {(close, autoFocus) => (
            <AskPrompt
              kind="text"
              fieldLabel="Note"
              start="Doesn't untap"
              label={() => "Set note"}
              autoFocus={autoFocus}
              onConfirm={close}
              onCancel={close}
            />
          )}
        </AskExample>
        <AskExample name="Set power and toughness…">
          {(close, autoFocus) => (
            <AskPrompt
              kind="stats"
              start={{ power: 3, toughness: 3 }}
              min={-99}
              max={99}
              label={({ power, toughness }) => `Set ${power}/${toughness}`}
              autoFocus={autoFocus}
              onConfirm={close}
              onCancel={close}
            />
          )}
        </AskExample>
      </div>
    </Panel>
  );
}

type MenuExample = "permanent" | "library" | "corner";

const permanentMenu: Pick<MenuProps, "header" | "groups"> = {
  header: { title: SAMPLE.beast.name, whose: "Token creature · yours" },
  groups: [
    { rows: [{ id: "tap", label: "Tap", click: true }] },
    {
      heading: "Move to",
      rows: [
        { id: "graveyard", label: "Graveyard", key: "⌘⌫" },
        { id: "exile", label: "Exile" },
        { id: "hand", label: "Hand" },
        { id: "top", label: "Top of library" },
        { id: "bottom", label: "Bottom of library", key: "⌘B" },
      ],
    },
    {
      rows: [
        { id: "counter", label: "Add counter…" },
        { id: "stats", label: "Set power and toughness…", key: "⌘P" },
        { id: "actions", label: "Every action…", key: "⌘K" },
      ],
    },
  ],
};

const libraryMenu: Pick<MenuProps, "header" | "groups"> = {
  header: { title: "Library", whose: "53 cards · yours" },
  groups: [
    {
      rows: [
        { id: "draw", label: "Draw", click: true },
        { id: "draw-cards", label: "Draw cards…", key: "⌘E" },
        { id: "look-top", label: "Look at top cards…", key: "⌘W" },
        { id: "search", label: "Search", key: "F3" },
        { id: "shuffle", label: "Shuffle", key: "⌘S" },
      ],
    },
    {
      heading: "Assists",
      rows: [
        { id: "scry", label: "Scry…" },
        { id: "surveil", label: "Surveil…" },
      ],
    },
    {
      rows: [
        {
          id: "keep-top",
          label: "Keep top card revealed",
          key: "⌘N",
          checked: true,
        },
        {
          id: "bottom",
          label: "Bottom of library",
          submenu: [
            { id: "draw-bottom", label: "Draw bottom card" },
            { id: "look-bottom", label: "Look at bottom cards…", key: "⇧⌘W" },
            { id: "mill-bottom", label: "Move bottom card to graveyard" },
          ],
        },
        { id: "actions", label: "Every action…", key: "⌘K" },
      ],
    },
  ],
};

function menuExample(example: MenuExample, x: number, y: number) {
  return {
    ...(example === "permanent" ? permanentMenu : libraryMenu),
    at:
      example === "corner"
        ? {
            x: document.documentElement.clientWidth - 1,
            y: window.innerHeight - 1,
          }
        : { x, y },
  };
}

function TableMenus() {
  const [example, setExample] = useState(() => {
    const preset = new URLSearchParams(window.location.search).get("menu");
    return preset === "permanent" || preset === "library" || preset === "corner"
      ? menuExample(preset, innerWidth / 2, innerHeight / 3)
      : undefined;
  });
  return (
    <Panel n="08" title="Context menus">
      <div className="gallery-menus">
        {(
          [
            ["permanent", "Permanent"],
            ["library", "Library"],
            ["corner", "Bottom-right corner"],
          ] as const
        ).map(([kind, label]) => (
          <Button
            key={kind}
            variant="secondary"
            onClick={(event) => {
              const { left, bottom } =
                event.currentTarget.getBoundingClientRect();
              setExample(menuExample(kind, left, bottom));
            }}
          >
            {label}
          </Button>
        ))}
      </div>
      {example && (
        <Menu
          {...example}
          presentation="menu"
          onChoose={() => {}}
          onClose={() => setExample(undefined)}
        />
      )}
    </Panel>
  );
}

const commandExamples: readonly CommandListEntry[] = [
  { id: "draw", label: "Draw cards…", group: "Library", key: "⌘E" },
  { id: "look-top", label: "Look at top cards…", group: "Library", key: "⌘W" },
  { id: "shuffle", label: "Shuffle", group: "Library", key: "⌘S" },
  { id: "move-top", label: "Move to top of library", group: "Selected card" },
  { id: "tap", label: "Tap", group: "Selected card" },
];

const commandQueries = [
  { query: "", label: "Empty query" },
  { query: "top", label: "Two groups" },
  { query: "untap", label: "No matches" },
];

function TableCommandList() {
  const [query, setQuery] = useState(() => {
    const preset = new URLSearchParams(window.location.search).get("commands");
    return commandQueries.find((example) => example.query === preset)?.query;
  });
  const [result, setResult] = useState("");
  const close = () => setQuery(undefined);
  const choose = (action: "Run" | "Bind", id: string) => {
    const command = commandExamples.find((command) => command.id === id);
    setResult(`${action} ${command?.label ?? id}`);
    close();
  };
  return (
    <Panel n="09" title="Command list">
      <div className="gallery-menus">
        {commandQueries.map(({ query, label }) => (
          <Button
            key={label}
            variant="secondary"
            onClick={() => {
              setResult("");
              setQuery(query);
            }}
          >
            {label}
          </Button>
        ))}
      </div>
      {result && <output className="gallery-command-result">{result}</output>}
      {query !== undefined && (
        <CommandList
          commands={commandExamples}
          acting="Your library"
          presentation="panel"
          initialQuery={query}
          onRun={(id) => choose("Run", id)}
          onBind={(id) => choose("Bind", id)}
          onClose={close}
        />
      )}
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
      <TablePrompts />
      <TableMenus />
      <TableCommandList />
    </main>
  );
}
