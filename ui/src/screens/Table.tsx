import {
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
  tableCurrent,
  tableLeave,
  tableLobby,
  tableQuit,
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
import { TableDrag } from "../table/drag";
import { TableViewers } from "../table/viewers";
import { registerSelection } from "../table/select";
import { GameBoard } from "./Board";
import { Button } from "../components/Controls";
import { usePhone } from "../mobile/phone";
import { Stage } from "../game/Stage";

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
  const [blocked, showBlockers] = useState<readonly number[]>();
  const [{ board, client, commands, leave }] = useState(() => {
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
    return { board, client, commands, leave };
  });
  const { stage, feed } = board;
  const { shown, log } = useSyncExternalStore(feed.subscribe, feed.fed);

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
      feed.arrive(update, past);
    });
    void unlisten
      .then(() => (heard ? undefined : tableCurrent()))
      .then((update) => {
        if (!mounted) return;
        if (update && !heard) feed.arrive(update, true);
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
  }, [client, feed]);

  useEffect(() => {
    if (!canStartTable()) return;
    let mounted = true;
    let heard = false;
    let played = false;
    const listening = onTableLobby(({ view }) => {
      if (!mounted) return;
      heard = true;
      if (view === null) played = true;
      else if (played) window.location.hash = "#/lobby";
    });
    void listening
      .then(async () => {
        if (heard) return;
        const update = await tableLobby({ kind: "view" });
        if (mounted && !heard) played = update?.view === null;
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
  return (
    <GameBoard
      board={board}
      client={client}
      view={shown.view}
      log={log}
      tableLog={{
        blocked,
        act: commands.history,
        context: commands.openLogLine,
      }}
      undo={() =>
        void commands.shortcut("table.undo").catch(commands.reportError)
      }
      waiting={false}
      stops={shown.view.stops}
      onLeave={leave}
      onShortcuts={commands.openShortcuts}
      onNextPhase={() => {
        if (!commands.overlayOpen && !client.overlayOpen)
          void commands.shortcut("phase.next").catch(commands.reportError);
      }}
    >
      <TableMenu
        commands={commands}
        overlay={overlay}
        view={shown.view}
        phone={phone}
      />
      {error && (
        <p className="game-error game-toast" role="alert">
          {error}
        </p>
      )}
    </GameBoard>
  );
}
