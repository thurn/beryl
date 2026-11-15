import {
  useEffect,
  useLayoutEffect,
  useState,
  useSyncExternalStore,
} from "react";
import { onGameUpdate, tableCurrent, tableLeave } from "../ipc";
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
import { TableDrag } from "../table/drag";
import { registerSelection } from "../table/select";
import { GameBoard } from "./Board";
import { Button } from "../components/Controls";
import { usePhone } from "../mobile/phone";
import { Stage } from "../game/Stage";

export function TableScreen() {
  const [error, setError] = useState<string>();
  const [ready, setReady] = useState(false);
  const [overlay, setOverlay] = useState<TableOverlay>();
  const [{ board, client, commands }] = useState(() => {
    const board = createBoardStage(false);
    const client = new TableClient(board.stage.pointer);
    const bindings = new Bindings(
      /Mac/.test(navigator.platform) ? "macos" : "linux",
    );
    const report = (failure: unknown) => setError(String(failure));
    client.drag = new TableDrag(client, board.stage, report);
    client.arrows = new TableArrows(client, board.stage, report, bindings);
    registerSelection(client, bindings);
    registerClicks(client, report);
    const commands = new TableCommands(client, bindings, setOverlay, report);
    return { board, client, commands };
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
      .then(tableCurrent)
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

  useLayoutEffect(() => {
    const update = () => {
      const { view } = stage.shown;
      if (view) client.setView(view);
    };
    update();
    return stage.subscribe(update);
  }, [stage, client]);

  useEffect(() => {
    window.addEventListener("keydown", client.key);
    return () => window.removeEventListener("keydown", client.key);
  }, [client]);

  const phone = usePhone();
  const leave = () => {
    void tableLeave().then(
      () => {
        window.location.hash = "#/";
      },
      (failure: unknown) => setError(String(failure)),
    );
  };

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
      waiting={false}
      stops={shown.view.stops}
      onLeave={leave}
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
