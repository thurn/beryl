// Link events stop input as soon as a transport reports a drop. A connected
// event alone cannot unlock the board. Wait for the replacement view and its
// fresh action references.
import { useCallback, useEffect, useRef, useState } from "react";
import {
  canStartTable,
  onTableLink,
  tableCurrent,
  type LinkState,
  type PlayerView,
} from "../ipc";

export function linkBlocks(kind?: LinkState["kind"]) {
  return kind === "reconnecting" || kind === "left";
}

export function useTableLink(setFrozen?: (frozen: boolean) => void) {
  const [kind, setKind] = useState<LinkState["kind"]>("alone");
  const [peer, setPeer] = useState<string>();
  const [error, setError] = useState<string>();
  const drops = useRef(0);
  const change = useCallback(
    (next: LinkState["kind"]) => {
      setFrozen?.(linkBlocks(next));
      setKind(next);
      if (next === "connected") setError(undefined);
    },
    [setFrozen],
  );
  const read = useCallback(
    (view: PlayerView, freshLink = true) => {
      if (!view.table) return;
      setPeer(view.table.peerName);
      if (freshLink) change(view.table.link?.kind ?? "alone");
    },
    [change],
  );

  useEffect(() => {
    if (!canStartTable()) return;
    let mounted = true;
    const listening = onTableLink((event) => {
      if (!mounted) return;
      if (event.kind === "error") setError(event.reason);
      else if (linkBlocks(event.kind)) {
        drops.current++;
        change(event.kind);
      }
    });
    return () => {
      mounted = false;
      void listening.then(
        (stop) => stop(),
        () => {},
      );
    };
  }, [change]);

  const current = useCallback(async () => {
    const before = drops.current;
    const update = await tableCurrent();
    // Loading the initial view's card records can outlast a drop. Its older
    // link state must not unlock the table when that snapshot finally arrives.
    return { update, freshLink: before === drops.current };
  }, []);
  return { kind, peer, error, frozen: linkBlocks(kind), read, current };
}
