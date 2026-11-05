// A game against an agent, drawn plainly from the latest PlayerView: each player's zones, the
// stack, and the open decision's candidates as buttons. The pseudo-3D game screen replaces it.
import { useEffect, useState, type ReactNode } from "react";
import { Button } from "../components/Controls";
import {
  choose,
  listAgents,
  onGameUpdate,
  setStops,
  startGame,
  type AgentView,
  type GameUpdate,
  type ObjectView,
  type PlayerView,
  type TargetRef,
} from "../ipc";

/** The latest view of the game in progress, from `game-update` events. */
export function useGameUpdates(): GameUpdate | null {
  const [update, setUpdate] = useState<GameUpdate | null>(null);
  useEffect(() => {
    const unlisten = onGameUpdate(setUpdate);
    return () => void unlisten.then((stop) => stop());
  }, []);
  return update;
}

/** The opponents to choose from; choosing one starts a game and shows it. */
export function AgentPicker() {
  const [agents, setAgents] = useState<AgentView[]>([]);
  const [error, setError] = useState<string>();
  useEffect(() => {
    listAgents().then(setAgents);
  }, []);
  const play = (agent: string) =>
    startGame(agent).then(
      () => (window.location.hash = "#/game"),
      (e: unknown) => setError(String(e)),
    );
  return (
    <section className="agents">
      <h2>Play the WU Control mirror against</h2>
      {agents.map((a) => (
        <Button key={a.name} variant="secondary" onClick={() => play(a.name)}>
          {a.name}: {a.description}
        </Button>
      ))}
      {error && <p className="game-error">{error}</p>}
    </section>
  );
}

const STEPS: Record<PlayerView["turn"]["step"], string> = {
  untap: "Untap",
  upkeep: "Upkeep",
  draw: "Draw",
  precombatMain: "Main 1",
  beginningOfCombat: "Beginning of combat",
  declareAttackers: "Declare attackers",
  declareBlockers: "Declare blockers",
  firstStrikeDamage: "First-strike damage",
  combatDamage: "Combat damage",
  endOfCombat: "End of combat",
  postcombatMain: "Main 2",
  end: "End step",
  cleanup: "Cleanup",
};

export function GameScreen({ update }: { update: GameUpdate | null }) {
  const [error, setError] = useState<string>();
  if (!update) {
    return (
      <main className="game">
        <p>No game in progress.</p>
        <a href="#/">Home</a>
      </main>
    );
  }
  const { session, view } = update;
  const objects = new Map(view.objects.map((o) => [o.slot, o]));
  const seat = (p: number) => (p === view.viewer ? "You" : "Opponent");
  const fail = (e: unknown) => setError(String(e));
  const outcome = view.outcome;
  return (
    <main className="game">
      <header className="game-header">
        <span>
          Turn {view.turn.number} · {seat(view.turn.active)} ·{" "}
          {STEPS[view.turn.step]}
        </span>
        <label>
          <input
            type="checkbox"
            checked={view.stops === "exact"}
            onChange={(e) =>
              setStops(session, e.target.checked ? "exact" : "smart").catch(
                fail,
              )
            }
          />{" "}
          Stop at every priority
        </label>
      </header>
      {[1 - view.viewer, view.viewer].map((p) => {
        const player = view.players[p];
        if (!player) {
          return null;
        }
        return (
          <section key={p} className="game-player">
            <h2>
              {seat(p)}: {player.life} life · library {player.library.length} ·
              hand {player.hand.length}
            </h2>
            <Zone title="Battlefield">
              {view.battlefield
                .map((s) => objects.get(s))
                .filter((o) => o?.controller === p)
                .map((o) => o && <Permanent key={o.slot} object={o} />)}
            </Zone>
            <Zone title="Hand">
              {player.hand.map((s) => (
                <span key={s} className="game-object">
                  {objects.get(s)?.name ?? "Card"}
                </span>
              ))}
            </Zone>
            <Zone title="Graveyard">
              {player.graveyard.map((s) => (
                <span key={s} className="game-object">
                  {objects.get(s)?.name}
                </span>
              ))}
            </Zone>
          </section>
        );
      })}
      {view.stack.length > 0 && (
        <Zone title="Stack">
          {view.stack.map((item, i) => (
            <span key={i} className="game-object">
              {item.name}
              {item.ability !== null && " (ability)"}
              {item.targets.length > 0 &&
                ` → ${item.targets.map((t) => target(t, objects, seat)).join(", ")}`}
            </span>
          ))}
        </Zone>
      )}
      {view.decision && (
        <section className="game-decision">
          <h2>Your decision: {view.decision.kind}</h2>
          <div className="game-candidates">
            {view.decision.candidates.map((c, i) => (
              <Button
                key={i}
                variant={i === 0 ? "primary" : "secondary"}
                size="small"
                onClick={() => choose(session, view.seq, i).catch(fail)}
              >
                {c.label}
              </Button>
            ))}
          </div>
        </section>
      )}
      {outcome && (
        <h2 className="game-outcome">
          {outcome.kind === "win"
            ? `${seat(outcome.player)} won`
            : outcome.kind === "draw"
              ? "A draw"
              : `Stalled: ${outcome.reason}`}
        </h2>
      )}
      {error && <p className="game-error">{error}</p>}
    </main>
  );
}

function Zone({ title, children }: { title: string; children: ReactNode }) {
  return (
    <div className="game-zone">
      <h3>{title}</h3>
      <div className="game-objects">{children}</div>
    </div>
  );
}

function Permanent({ object }: { object: ObjectView }) {
  const details = [
    object.power !== null && `${object.power}/${object.toughness}`,
    ...object.counters.map((c) => `${c.count} ${c.kind}`),
    object.tapped && "tapped",
    object.combat?.kind,
  ].filter(Boolean);
  return (
    <span className="game-object">
      {object.name}
      {details.length > 0 && ` (${details.join(", ")})`}
    </span>
  );
}

const target = (
  t: TargetRef,
  objects: Map<number, ObjectView>,
  seat: (p: number) => string,
) =>
  t.kind === "player"
    ? seat(t.player)
    : (objects.get(t.slot)?.name ?? "a card");
