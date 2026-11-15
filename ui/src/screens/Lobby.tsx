import { useEffect, useRef, useState } from "react";
import { Button } from "../components/Controls";
import { Stage } from "../game/Stage";
import {
  appInfo,
  isNativeApp,
  onGameUpdate,
  onTableLobby,
  tableLeave,
  tableLobby,
  tableQuit,
  type DeckImportReply,
  type LobbyChange,
  type LobbyUpdate,
} from "../ipc";
import { useKeyboardInset } from "../mobile/keyboardInset";
import { usePhone } from "../mobile/phone";
import { LinkBanner } from "../table/LinkBanner";
import { useLobbyLinkDetails, useTableLink } from "../table/link";
import { Bindings } from "../table/bindings";
import { AssistSwitches } from "../table/lobby/AssistSwitches";
import { DeckPicker, type DeckPanel } from "../table/lobby/DeckPicker";
import { GameSettings, GameSettingsSummary } from "../table/lobby/GameSettings";
import { SideboardSwap } from "../table/lobby/SideboardSwap";
import { resolve } from "../table/shortcuts";
import { LobbySummary } from "../table/lobby/LobbySummary";
import { openNativeTableKeys } from "../table/keyboard";
import "../table/lobby/lobby.css";

export function Lobby() {
  const phone = usePhone();
  const keyboardStyle = useKeyboardInset(phone);
  const [update, setUpdate] = useState<LobbyUpdate | null>();
  const [picker, setPicker] = useState<DeckPanel>();
  const [deckName, setDeckName] = useState<string>();
  const [sideboard, setSideboard] = useState(false);
  const [pending, setPending] = useState(false);
  const busy = useRef(false);
  const [error, setError] = useState<string>();
  const {
    link,
    kind,
    peer,
    frozen,
    read,
    current,
    error: linkError,
  } = useTableLink();
  const view = update?.view;
  const own = view?.seats.find((seat) => seat.seat === view.you);
  useLobbyLinkDetails(link.kind === "connected", { current, read });
  useEffect(
    () =>
      phone
        ? undefined
        : openNativeTableKeys(() => setError("Could not set table shortcuts.")),
    [phone],
  );
  useEffect(() => {
    let mounted = true;
    let heard = false;
    const receive = (next: LobbyUpdate | null) => {
      if (mounted) setUpdate(next);
    };
    const listening = onTableLobby((next) => {
      heard = true;
      receive(next);
    });
    void listening
      .then(() => (heard ? undefined : tableLobby({ kind: "view" })))
      .then((next) => {
        if (!heard && next !== undefined) receive(next);
      })
      .catch(() => {
        if (mounted) setError("Could not open the lobby.");
      });
    return () => {
      mounted = false;
      void listening.then(
        (stop) => stop(),
        () => {},
      );
    };
  }, []);
  useEffect(() => {
    let mounted = true;
    let heard = false;
    const listening = onGameUpdate(({ view }) => {
      if (!mounted || !view.table) return;
      heard = true;
      read(view);
    });
    void listening
      .then(() => (heard ? undefined : current()))
      .then((snapshot) => {
        if (mounted && !heard && snapshot?.update)
          read(snapshot.update.view, snapshot.freshLink);
      })
      .catch(() => {});
    return () => {
      mounted = false;
      void listening.then(
        (stop) => stop(),
        () => {},
      );
    };
  }, [current, read]);
  useEffect(() => {
    if (update && !update.view) window.location.hash = "#/table";
  }, [update]);

  const send = async (action: LobbyChange) => {
    if (frozen || busy.current) throw new Error("The lobby is unavailable.");
    busy.current = true;
    setPending(true);
    setError(undefined);
    try {
      await tableLobby(action);
    } finally {
      busy.current = false;
      setPending(false);
    }
  };
  const change = async (action: LobbyChange) => {
    try {
      await send(action);
    } catch (failure) {
      setError(
        action.kind === "start"
          ? "Could not start the game."
          : "Could not update the lobby.",
      );
      throw failure;
    }
  };
  const run = (action: LobbyChange) => void change(action).catch(() => {});
  const pick = async ({ name, main, side }: DeckImportReply) => {
    await send({ kind: "load_deck", main, side });
    setDeckName(name);
  };
  const unload = () => {
    void change({ kind: "load_deck", main: [], side: [] }).then(
      () => {
        setDeckName(undefined);
        setSideboard(false);
      },
      () => {},
    );
  };
  const leave = () => {
    void tableLeave().then(
      () => {
        window.location.hash = "#/";
      },
      () => setError("Could not leave the lobby."),
    );
  };

  const keyboard =
    useRef<(event: KeyboardEvent, bindings: Bindings) => void>(undefined);
  keyboard.current = (event, bindings) => {
    if (event.defaultPrevented || event.repeat || event.isComposing) return;
    if (resolve(event, "table", bindings) === "table.leave") {
      event.preventDefault();
      if (isNativeApp())
        void tableQuit().catch(() => setError("Could not quit."));
      else if (window.confirm("Leave table?")) leave();
      return;
    }
    if (frozen || pending) return;
    const id = resolve(event, "lobby", bindings);
    if (id === "lobby.paste_deck") setPicker("paste");
    else if (id === "lobby.change_deck") setPicker("file");
    else if (id === "lobby.unload_deck" && own?.loaded) unload();
    else if (id === "lobby.sideboard" && own?.loaded)
      setSideboard((open) => !open);
    else if (id === "lobby.ready" && view?.you !== 0 && own?.loaded)
      run({ kind: "ready", ready: !own.ready });
    else return;
    event.preventDefault();
  };
  useEffect(() => {
    if (phone) return;
    let mounted = true;
    let bindings: Bindings | undefined;
    void appInfo().then(
      ({ platform }) => {
        if (mounted) bindings = new Bindings(platform);
      },
      () => {},
    );
    const key = (event: KeyboardEvent) => {
      if (bindings) keyboard.current?.(event, bindings);
    };
    window.addEventListener("keydown", key);
    return () => {
      mounted = false;
      window.removeEventListener("keydown", key);
    };
  }, [phone]);

  const screen = (
    <div className="table start-screen lobby-screen" style={keyboardStyle}>
      <main
        className="start-panel lobby-panel"
        aria-label="Table lobby"
        inert={frozen}
      >
        <header className="lobby-heading">
          <h1 className="start-title">Table</h1>
          <Button variant="secondary" disabled={pending} onClick={leave}>
            Leave table
          </Button>
        </header>
        {view && own ? (
          <>
            <LobbySummary view={view} link={link} />
            <section className="lobby-section" aria-label="Deck">
              <div className="lobby-deck">
                <div>
                  <h2>
                    {own.loaded
                      ? (deckName ?? "Deck loaded")
                      : "No deck loaded"}
                  </h2>
                  {own.loaded && (
                    <p>
                      {own.mainCount} main · {own.sideCount} sideboard
                    </p>
                  )}
                </div>
                <div className="lobby-actions">
                  <Button
                    variant="secondary"
                    disabled={pending}
                    onClick={() => setPicker("change")}
                  >
                    Change
                  </Button>
                  <Button
                    variant="secondary"
                    disabled={pending || !own.loaded}
                    onClick={() => setSideboard((open) => !open)}
                  >
                    Sideboard
                  </Button>
                  {own.loaded && (
                    <Button
                      variant="secondary"
                      disabled={pending}
                      onClick={unload}
                    >
                      Unload deck
                    </Button>
                  )}
                </div>
              </div>
              {view.unknown.length > 0 && (
                <div role="status">
                  <h3>Unknown cards</h3>
                  <ul>
                    {view.unknown.map((name) => (
                      <li key={name}>{name}</li>
                    ))}
                  </ul>
                </div>
              )}
            </section>
            {picker && (
              <DeckPicker
                key={picker}
                panel={picker}
                setPanel={setPicker}
                phone={phone}
                useDeck={pick}
              />
            )}
            {sideboard && own.loaded && (
              <SideboardSwap
                view={view}
                pending={pending}
                change={run}
                close={() => setSideboard(false)}
              />
            )}
            <AssistSwitches
              assists={view.assists}
              disabled={pending}
              onToggle={(assist, on) => run({ kind: "set_assist", assist, on })}
            />
            {view.you === 0 ? (
              <GameSettings view={view} pending={pending} change={change} />
            ) : (
              <GameSettingsSummary view={view} />
            )}
            {view.you === 0 ? (
              <Button
                className="lobby-start"
                disabled={!view.canStart || pending}
                onClick={() => run({ kind: "start" })}
              >
                Start
              </Button>
            ) : (
              <Button
                className="lobby-start"
                disabled={!own.loaded || pending}
                aria-pressed={own.ready}
                onClick={() => run({ kind: "ready", ready: !own.ready })}
              >
                Ready
              </Button>
            )}
          </>
        ) : update === null ? (
          <p role="status">Waiting for the table…</p>
        ) : null}
        {error && (
          <p className="game-error" role="alert">
            {error}
          </p>
        )}
      </main>
      <LinkBanner kind={kind} peer={peer} onLobby={leave} />
      {linkError && (
        <p className="game-error game-toast" role="alert">
          {linkError}
        </p>
      )}
    </div>
  );
  return phone ? screen : <Stage>{screen}</Stage>;
}
