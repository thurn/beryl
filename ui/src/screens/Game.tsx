// A game against an agent, drawn plainly from the latest PlayerView: each player's zones, the
// stack, and the open decision's candidates as buttons. The pseudo-3D game screen replaces it.
import { useEffect, useState, type ReactNode } from "react";
import { Button } from "../components/Controls";
import {
  choose,
  CommandError,
  listAgents,
  onGameUpdate,
  setStops,
  startGame,
  type AgentView,
  type GameUpdate,
  type ObjectView,
  type PlayerView,
  type Side,
  type Stall,
  type StackItem,
  type TargetRef,
} from "../ipc";

/** The latest view of the game in progress, from `game-update` events. Session numbers only
 * grow, so a late view from a replaced game is dropped. */
export function useGameUpdates(): GameUpdate | null {
  const [update, setUpdate] = useState<GameUpdate | null>(null);
  useEffect(() => {
    const unlisten = onGameUpdate((next) =>
      setUpdate((last) => (last && next.session < last.session ? last : next)),
    );
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

const STALLS: Record<Stall, string> = {
  decisionsPerTurn: "too many decisions in one turn",
  turns: "too many turns",
  stateBasedIterations: "state-based actions did not settle",
  capacity: "a zone or queue was full",
};

const SIDES: Record<Side, string> = { you: "You", opponent: "Opponent" };

/** A command's failure as the player should hear of it; none for an answer to a decision
 * that was answered already (a double click) or replaced. */
function failure(e: unknown): string | undefined {
  if (!(e instanceof CommandError)) {
    return String(e);
  }
  const kind = e.error.kind;
  return kind === "notDeciding" || kind === "staleDecision"
    ? undefined
    : e.message;
}

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
  const fail = (e: unknown) => {
    const message = failure(e);
    if (message) {
      setError(message);
    }
  };
  const status = view.status;
  return (
    <main className="game">
      <header className="game-header">
        <span>
          Turn {view.turn.number} · {SIDES[view.turn.active]} ·{" "}
          {STEPS[view.turn.step]}
        </span>
        <label>
          <input
            type="checkbox"
            checked={view.stops === "exact"}
            disabled={status.kind !== "deciding"}
            onChange={(e) =>
              setStops(session, e.target.checked ? "exact" : "smart").catch(
                fail,
              )
            }
          />{" "}
          Stop at every priority
        </label>
      </header>
      {(["opponent", "you"] as const).map((side) => {
        const player = view.players[side];
        const hand = view.objects.filter(
          (o) => o.zone === "hand" && o.owner === side,
        );
        return (
          <section key={side} className="game-player">
            <h2>
              {SIDES[side]}: {player.life} life · library{" "}
              {player.library.length} · hand {hand.length}
            </h2>
            <Zone title="Battlefield">
              {view.battlefield
                .map((s) => objects.get(s))
                .filter((o) => o?.controller === side)
                .map((o) => o && <Permanent key={o.slot} object={o} />)}
            </Zone>
            <Zone title="Hand">
              {hand.map((o) => (
                <span key={o.slot} className="game-object">
                  {o.name ?? "Card"}
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
              {stackName(item, objects)}
              {item.targets.length > 0 &&
                ` → ${item.targets.map((t) => target(t, objects)).join(", ")}`}
            </span>
          ))}
        </Zone>
      )}
      {status.kind === "deciding" && (
        <section className="game-decision">
          <h2>Your decision: {status.decision.kind}</h2>
          <div className="game-candidates">
            {status.decision.candidates.map((c, i) => (
              <Button
                key={i}
                variant={i === 0 ? "primary" : "secondary"}
                size="small"
                onClick={() => choose(status.decision.id, i).catch(fail)}
              >
                {c.label}
              </Button>
            ))}
          </div>
        </section>
      )}
      {status.kind === "stopped" && <h2 className="game-outcome">Stopped</h2>}
      {status.kind === "over" && (
        <h2 className="game-outcome">
          {status.outcome.kind === "win"
            ? status.outcome.winner === "you"
              ? "You won"
              : "Your opponent won"
            : status.outcome.kind === "draw"
              ? "A draw"
              : `Stalled: ${STALLS[status.outcome.reason]}`}
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
    object.pt && `${object.pt.power}/${object.pt.toughness}`,
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

const stackName = (item: StackItem, objects: Map<number, ObjectView>) =>
  item.kind === "spell"
    ? (objects.get(item.slot)?.name ?? "a spell")
    : `${item.name} (ability)`;

const target = (t: TargetRef, objects: Map<number, ObjectView>) =>
  t.kind === "player"
    ? SIDES[t.player]
    : (objects.get(t.slot)?.name ?? "a card");
