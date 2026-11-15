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
import { registerClicks } from "../table/click";
import { TableClient } from "../table/client";
import { GameBoard } from "./Board";
import { Button } from "../components/Controls";
import { usePhone } from "../mobile/phone";
import { Stage } from "../game/Stage";

export function TableScreen() {
  const [error, setError] = useState<string>();
  const [ready, setReady] = useState(false);
  const [{ board, client }] = useState(() => {
    const board = createBoardStage(false);
    const client = new TableClient(board.stage.pointer);
    registerClicks(client, (failure) => setError(String(failure)));
    return { board, client };
  });
  const { stage, feed } = board;
  const { shown, log } = useSyncExternalStore(feed.subscribe, feed.fed);

  useLayoutEffect(() => {
    exposeBoardStage(board);
    return () => exposeBoardStage(pageStage());
  }, [board]);

  useEffect(() => {
    let mounted = true;
    let heard = false;
    const unlisten = onGameUpdate((update, past) => {
      if (!mounted || !update.view.table) return;
      heard = true;
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
  }, [feed]);

  useLayoutEffect(() => {
    const update = () => {
      const { view } = stage.shown;
      if (view) client.setView(view);
    };
    update();
    return stage.subscribe(update);
  }, [stage, client]);

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
      {error && (
        <p className="game-error game-toast" role="alert">
          {error}
        </p>
      )}
    </GameBoard>
  );
}
