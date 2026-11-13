// The screen before a game: the opponent and seed to start it with, or the game in
// progress to return to.
import {
  type AgentView,
  type SessionId,
  concede,
  exportBugs,
  listAgents,
} from "../ipc";
import { Button } from "../components/Controls";
import { SoundCorner } from "../audio/SoundControls";
import { Stage } from "../game/Stage";
import { usePhone } from "../mobile/phone";
import { play } from "../audio/sound";
import { useEffect, useRef, useState } from "react";

/** The agent that plays on the true state, hidden cards included. */
const CHEATING = "mcts-omniscient";
/** The opponent offered first: strong, and quick to answer. */
const DEFAULT_AGENT = "expert";
/** Where the start screen remembers the last opponent and seed. */
const LAST_GAME = "beryl.lastGame";

/** The opponent and seed of the last game started here, if the browser kept them. */
function lastGame(): { agent?: string; seed?: string } {
  try {
    const saved: unknown = JSON.parse(localStorage.getItem(LAST_GAME) ?? "{}");
    if (typeof saved !== "object" || saved === null) return {};
    const { agent, seed } = saved as Record<string, unknown>;
    return {
      agent: typeof agent === "string" ? agent : undefined,
      seed: typeof seed === "string" ? seed : undefined,
    };
  } catch {
    return {};
  }
}

function rememberGame(agent: string, seed: string) {
  try {
    localStorage.setItem(LAST_GAME, JSON.stringify({ agent, seed }));
  } catch {
    // Storage may be blocked; the start screen then offers the defaults.
  }
}

/** Picks the opponent and an optional seed, and starts the game; or returns to the game in
 * progress, `inProgress`, which a new game concedes once the player confirms it. Enter does
 * what the primary button does. */
export function StartScreen({
  inProgress,
  resumed,
  onStart,
  exportReports,
}: {
  inProgress?: SessionId;
  /** The game in progress was resumed at launch and not yet gone back to: a new game
   * concedes it without asking. */
  resumed: boolean;
  onStart(agent: string, seed?: number): Promise<void>;
  exportReports: boolean;
}) {
  const phone = usePhone();
  const [agents, setAgents] = useState<AgentView[]>([]);
  const [agent, setAgent] = useState<string>();
  const [seed, setSeed] = useState(() => lastGame().seed ?? "");
  const [error, setError] = useState<string>();
  const [pending, setPending] = useState(false);
  // A new game was asked for with one in progress: the next click concedes it.
  const [confirming, setConfirming] = useState(false);
  useEffect(() => {
    listAgents().then((list) => {
      setAgents(list);
      const offered = (name?: string) =>
        list.find((a) => a.name === name)?.name;
      setAgent(
        (a) =>
          a ??
          offered(lastGame().agent) ??
          offered(DEFAULT_AGENT) ??
          list[0]?.name,
      );
    });
  }, []);
  const seedValue = seed.trim() === "" ? undefined : Number(seed);
  const seedOk =
    seedValue === undefined ||
    (Number.isInteger(seedValue) && seedValue >= 0 && seedValue < 2 ** 32);
  const startable = agent !== undefined && seedOk && !pending;
  const starting = useRef(false);
  const start = () => {
    if (!agent || !startable || starting.current) return;
    if (inProgress !== undefined && !resumed && !confirming) {
      setConfirming(true);
      return;
    }
    starting.current = true;
    play("start");
    rememberGame(agent, seed.trim());
    setPending(true);
    setError(undefined);
    // The new game replaces the old one whether or not the concession lands first.
    const conceded =
      inProgress === undefined
        ? Promise.resolve()
        : concede(inProgress).catch(() => undefined);
    void conceded
      .then(() => onStart(agent, seedValue))
      .then(
        () => (window.location.hash = "#/game"),
        (e: unknown) => {
          starting.current = false;
          setPending(false);
          setError(String(e));
        },
      );
  };
  // Enter takes the primary button from anywhere on the screen but a button, which Enter
  // presses: back to the game in progress, else a new one.
  const onEnter = useRef<(e: KeyboardEvent) => void>(undefined);
  onEnter.current = (e) => {
    if (e.key !== "Enter" || e.repeat) return;
    if (e.target instanceof Element && e.target.closest("button")) return;
    e.preventDefault();
    if (inProgress !== undefined) window.location.hash = "#/game";
    else start();
  };
  useEffect(() => {
    const listener = (e: KeyboardEvent) => onEnter.current?.(e);
    window.addEventListener("keydown", listener);
    return () => window.removeEventListener("keydown", listener);
  }, []);
  const agentRows = agents.map((a) => (
    <label key={a.name} className="start-agent">
      <input
        type="radio"
        name="agent"
        value={a.name}
        checked={agent === a.name}
        onChange={() => setAgent(a.name)}
      />
      <span className="start-agent-name">
        {a.title}
        {a.name === CHEATING && (
          <em className="start-cheating">cheating: sees your hand</em>
        )}
      </span>
      <span className="start-agent-about">{a.description}</span>
    </label>
  ));
  const screen = (
    <div
      className={
        phone ? "table start-screen phone-start-screen" : "table start-screen"
      }
    >
      <SoundCorner />
      <section className="start-panel">
        {!phone && (
          <>
            <h1 className="start-title">New Game</h1>
            <p className="start-subtitle">WU Control mirror</p>
          </>
        )}
        <fieldset className="start-agents">
          <legend>Opponent</legend>
          {phone ? (
            <div className="phone-start-agent-list">{agentRows}</div>
          ) : (
            agentRows
          )}
        </fieldset>
        <label className="start-seed">
          <span>Seed</span>
          <input
            inputMode="numeric"
            placeholder="random"
            value={seed}
            onChange={(e) => setSeed(e.target.value)}
            aria-invalid={!seedOk}
          />
        </label>
        {/* With a game in progress, going back to it comes first. */}
        <div className="start-buttons">
          {inProgress !== undefined && (
            <Button onClick={() => (window.location.hash = "#/game")}>
              Return to Game
            </Button>
          )}
          <Button
            variant={inProgress !== undefined ? "secondary" : "primary"}
            disabled={!startable}
            onClick={start}
          >
            {pending
              ? "Starting…"
              : inProgress === undefined
                ? "Start"
                : confirming
                  ? "Concede & Start"
                  : "New Game"}
          </Button>
        </div>
        {error && <p className="game-error">{error}</p>}
        {exportReports && <ExportReports />}
      </section>
    </div>
  );
  return phone ? screen : <Stage>{screen}</Stage>;
}

function ExportReports() {
  const [pending, setPending] = useState(false);
  const [status, setStatus] = useState("");
  const save = async () => {
    setPending(true);
    setStatus("");
    try {
      const count = await exportBugs();
      if (count === 0) setStatus("No reports to export.");
      else if (count !== null) setStatus("ZIP saved.");
    } catch {
      setStatus("Couldn’t save reports.");
    } finally {
      setPending(false);
    }
  };
  return (
    <>
      <Button
        size="small"
        variant="secondary"
        disabled={pending}
        title="Includes screenshots, replays, and logs"
        onClick={() => void save()}
      >
        {pending ? "Exporting…" : "Export Bug Reports"}
      </Button>
      {status && (
        <p className="start-working" role="status">
          {status}
        </p>
      )}
    </>
  );
}
