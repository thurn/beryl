import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useState,
  useSyncExternalStore,
} from "react";
import {
  appInfo,
  canStartTable,
  isNativeApp,
  onGameUpdate,
  onTableLobby,
  tableLeave,
  tableLobby,
  tableQuit,
  type LobbyView,
} from "../ipc";
import {
  createBoardStage,
  exposeBoardStage,
  pageStage,
} from "../stage/playback";
import type { Name } from "../stage/layout";
import type { Under } from "../stage/pointer";
import { Bindings } from "../table/bindings";
import {
  TableCommands,
  TableMenu,
  type TableOverlay,
} from "../table/TableMenu";
import { TableArrows } from "../table/arrows";
import { registerClicks } from "../table/click";
import { TableClient } from "../table/client";
import { listenTableKeys } from "../table/keyboard";
import { registerPlayers } from "../table/players";
import { registerTurn } from "../table/turn";
import type { AppInfo } from "../generated/protocol";
import { LinkBanner } from "../table/LinkBanner";
import { linkBlocks, useLobbyLinkDetails, useTableLink } from "../table/link";
import { TableDrag } from "../table/drag";
import { TableOpening } from "../table/TableOpening";
import { TableViewers } from "../table/viewers";
import { registerSelection } from "../table/select";
import { GameBoard } from "./Board";
import { Button } from "../components/Controls";
import { usePhone } from "../mobile/phone";
import { Stage } from "../game/Stage";
import { MatchDialog, type MatchPanel } from "../table/lobby/MatchDialog";

export function TableScreen() {
  const [platform, setPlatform] = useState<AppInfo["platform"]>();
  useEffect(() => {
    void appInfo()
      .then(({ platform }) => setPlatform(platform))
      .catch(() => {
        setPlatform(/Mac/.test(navigator.platform) ? "macos" : "linux");
      });
  }, []);
  return platform && <ManualTable platform={platform} />;
}

function ManualTable({ platform }: { platform: AppInfo["platform"] }) {
  const [error, setError] = useState<string>();
  const [ready, setReady] = useState(false);
  const [overlay, setOverlay] = useState<TableOverlay>();
  const [panel, setPanel] = useState<MatchPanel>();
  const [lobby, setLobby] = useState<LobbyView>();
  const [blocked, showBlockers] = useState<readonly number[]>();
  const [{ board, client, commands, leave, openMatch }] = useState(() => {
    const board = createBoardStage(false);
    const client = new TableClient(board.stage.pointer);
    const bindings = new Bindings(platform);
    const report = (failure: unknown) => setError(String(failure));
    client.drag = new TableDrag(client, board.stage, report);
    client.arrows = new TableArrows(client, board.stage, report);
    client.viewers = new TableViewers(client, report);
    registerSelection(client);
    registerTurn(client, report);
    const leave = () => {
      void tableLeave().then(() => {
        window.location.hash = "#/";
      }, report);
    };
    const quit = () => {
      if (isNativeApp()) void tableQuit().catch(report);
      else if (window.confirm("Leave table?")) leave();
    };
    const commands = new TableCommands(
      client,
      bindings,
      setOverlay,
      report,
      leave,
      quit,
      showBlockers,
    );
    registerClicks(client, commands);
    registerPlayers(client, commands);
    client.viewers.register(commands);
    const openMatch = (panel: MatchPanel) => {
      commands.close();
      setPanel(panel);
    };
    commands.clients.set("open_lobby", () => openMatch("lobby"));
    commands.clients.set("open_next_game", () => openMatch("nextGame"));
    commands.clients.set("confirm_concede", () => openMatch("concede"));
    commands.clients.set("open_assists", () => openMatch("assists"));
    return { board, client, commands, leave, openMatch };
  });
  const setFrozen = useCallback(
    (frozen: boolean) => {
      client.setFrozen(frozen);
      if (frozen) {
        commands.close();
        setPanel(undefined);
        board.feed.skip();
      }
    },
    [board, client, commands],
  );
  const {
    read,
    current,
    link,
    kind,
    peer,
    frozen,
    error: linkError,
  } = useTableLink(setFrozen);
  useLobbyLinkDetails(panel === "lobby" && link.kind === "connected", {
    current,
    read,
  });
  const { stage, feed } = board;
  const { shown, log } = useSyncExternalStore(feed.subscribe, feed.fed);
  const playing = useSyncExternalStore(stage.subscribe, () => stage.shown.busy);

  useLayoutEffect(() => {
    exposeBoardStage(board);
    // A driven browser can open the target picker through the same API as a menu.
    const driven = window as Window & {
      berylPickTableTarget?: (from: Name) => Promise<Under | undefined>;
    };
    if (navigator.webdriver)
      driven.berylPickTableTarget = (from) => client.arrows!.pickTarget(from);
    return () => {
      delete driven.berylPickTableTarget;
      exposeBoardStage(pageStage());
    };
  }, [board, client]);

  useEffect(() => {
    let mounted = true;
    let heard = false;
    const unlisten = onGameUpdate((update, past) => {
      if (!mounted || !update.view.table) return;
      heard = true;
      client.drag?.arriving();
      const link = update.view.table.link?.kind;
      if (linkBlocks(link)) read(update.view);
      const skip =
        (past && !update.view.table.choosingFirst) ||
        linkBlocks(link) ||
        client.frozen;
      feed.arrive(update, skip);
      if (!linkBlocks(link)) read(update.view);
    });
    void unlisten
      .then(() => (heard ? undefined : current()))
      .then((snapshot) => {
        if (!mounted) return;
        if (snapshot?.update && !heard) {
          read(snapshot.update.view, snapshot.freshLink);
          feed.arrive(
            snapshot.update,
            !snapshot.update.view.table?.choosingFirst || client.frozen,
          );
        }
        setReady(true);
      })
      .catch((failure: unknown) => {
        if (mounted) setError(String(failure));
      });
    return () => {
      mounted = false;
      void unlisten.then(
        (stop) => stop(),
        () => {},
      );
    };
  }, [client, current, feed, read]);

  useEffect(() => {
    if (!canStartTable()) return;
    let mounted = true;
    let heard = false;
    let played = false;
    const listening = onTableLobby(({ view, game }) => {
      if (!mounted) return;
      heard = true;
      if (view === null) {
        played = true;
        setLobby(game);
      } else if (played) window.location.hash = "#/lobby";
    });
    void listening
      .then(async () => {
        if (heard) return;
        const update = await tableLobby({ kind: "view" });
        if (mounted && !heard) {
          played = update?.view === null;
          setLobby(update?.game ?? update?.view ?? undefined);
        }
      })
      .catch(() => {});
    return () => {
      mounted = false;
      void listening.then(
        (stop) => stop(),
        () => {},
      );
    };
  }, []);

  useLayoutEffect(() => {
    const update = () => {
      const { view } = stage.shown;
      if (view) client.setView(view);
    };
    update();
    return stage.subscribe(update);
  }, [stage, client]);

  useEffect(() => listenTableKeys(commands), [commands]);

  const phone = usePhone();

  if (!shown) {
    const status = (
      <div className="table start-screen">
        <section className="start-panel" role="status">
          <p className="start-subtitle">
            {error ?? (ready ? "No table in progress." : "Opening table…")}
          </p>
          {(ready || error) && (
            <Button
              onClick={() => {
                window.location.hash = "#/";
              }}
            >
              Back
            </Button>
          )}
        </section>
      </div>
    );
    return phone ? status : <Stage>{status}</Stage>;
  }
  const prompt = shown.view.table?.prompt;
  const opening =
    prompt?.kind === "opening" ||
    (prompt?.kind === "choose" && shown.view.table?.choosingFirst && !playing);
  return (
    <GameBoard
      board={board}
      client={client}
      view={shown.view}
      opening={opening}
      log={log}
      tableLog={{
        blocked,
        act: commands.history,
        context: commands.openLogLine,
      }}
      undo={() =>
        void commands.shortcut("table.undo").catch(commands.reportError)
      }
      freeze={frozen}
      notice={
        <>
          <LinkBanner kind={kind} peer={peer} onLobby={leave} />
          {(linkError ?? error) && (
            <p className="game-error game-toast" role="alert">
              {linkError ?? error}
            </p>
          )}
        </>
      }
      waiting={false}
      stops={shown.view.stops}
      onLeave={leave}
      onShortcuts={commands.openShortcuts}
      onMatchPanel={canStartTable() ? openMatch : undefined}
      onNextPhase={() => {
        if (!commands.overlayOpen && !client.overlayOpen)
          void commands.shortcut("phase.next").catch(commands.reportError);
      }}
    >
      {opening && <TableOpening client={client} view={shown.view} />}
      {panel && (
        <MatchDialog
          key={panel}
          panel={panel}
          client={client}
          view={shown.view}
          lobby={lobby}
          link={link}
          close={() => setPanel(undefined)}
        />
      )}
      <TableMenu
        commands={commands}
        overlay={overlay}
        view={shown.view}
        phone={phone}
      />
    </GameBoard>
  );
}
