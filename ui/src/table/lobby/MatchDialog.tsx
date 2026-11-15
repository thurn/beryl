import { useLayoutEffect, useRef, useState } from "react";
import { Button } from "../../components/Controls";
import type { LinkState, LobbyView, PlayerView, TableAction } from "../../ipc";
import type { TableClient } from "../client";
import { assists } from "../menuRows";
import { AssistSwitches } from "./AssistSwitches";
import { GameSettingsSummary } from "./GameSettings";
import { LobbySummary } from "./LobbySummary";
import "./lobby.css";

const titles = {
  lobby: "Lobby",
  nextGame: "Next game",
  concede: "Concede",
  assists: "Assists",
} as const;

export type MatchPanel = keyof typeof titles;

export function MatchDialog({
  panel,
  client,
  view,
  lobby,
  link,
  close,
}: {
  panel: MatchPanel;
  client: TableClient;
  view: PlayerView;
  lobby?: LobbyView;
  link: LinkState;
  close(): void;
}) {
  const dialog = useRef<HTMLDialogElement>(null);
  const busy = useRef(false);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState(false);
  useLayoutEffect(() => {
    const node = dialog.current;
    node?.showModal();
    return () => node?.close();
  }, []);

  const send = async (action: TableAction, ending: boolean) => {
    if (busy.current) return;
    busy.current = true;
    setPending(true);
    setError(false);
    try {
      const reply = await client.send(action);
      if ("refused" in reply) setError(true);
      else if (ending) close();
    } catch {
      setError(true);
    } finally {
      busy.current = false;
      setPending(false);
    }
  };
  const next = (winner: number | null) =>
    void send({ next_game: { winner } }, true);

  return (
    <dialog
      ref={dialog}
      className="table-match-dialog lobby-panel"
      aria-label={titles[panel]}
      onCancel={(event) => {
        event.preventDefault();
        if (!pending) close();
      }}
    >
      <h2>{titles[panel]}</h2>
      {panel === "lobby" &&
        (lobby ? (
          <>
            <LobbySummary view={lobby} link={link} />
            <GameSettingsSummary view={lobby} />
          </>
        ) : (
          <p role="status">Opening lobby…</p>
        ))}
      {panel === "nextGame" && (
        <div className="lobby-actions">
          {lobby?.seats.map(({ seat, name }) => (
            <Button key={seat} disabled={pending} onClick={() => next(seat)}>
              {name ?? `Seat ${seat + 1}`} wins
            </Button>
          ))}
          <Button disabled={pending} onClick={() => next(null)}>
            No result
          </Button>
        </div>
      )}
      {panel === "concede" && (
        <Button
          disabled={pending}
          onClick={() => void send({ concede: {} }, true)}
        >
          Confirm Concede
        </Button>
      )}
      {panel === "assists" && (
        <AssistSwitches
          assists={[...assists(view)]}
          disabled={pending}
          onToggle={(assist, on) =>
            void send({ set_assist: { assist, on } }, false)
          }
        />
      )}
      {error && <p role="alert">Could not update the table.</p>}
      <Button variant="secondary" autoFocus disabled={pending} onClick={close}>
        {panel === "concede" || panel === "nextGame" ? "Cancel" : "Close"}
      </Button>
    </dialog>
  );
}
