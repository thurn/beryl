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
  const [link, setLink] = useState<LinkState>({ kind: "alone" });
  const { kind } = link;
  const [peer, setPeer] = useState<string>();
  const [error, setError] = useState<string>();
  const drops = useRef(0);
  const change = useCallback(
    (next: LinkState) => {
      setFrozen?.(linkBlocks(next.kind));
      setLink(next);
      if (next.kind === "connected") setError(undefined);
    },
    [setFrozen],
  );
  const read = useCallback(
    (view: PlayerView, freshLink = true) => {
      if (!view.table) return;
      setPeer(view.table.peerName);
      if (freshLink) change(view.table.link ?? { kind: "alone" });
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
        change(event);
      } else if (event.kind === "connected") {
        setLink((previous) => (linkBlocks(previous.kind) ? previous : event));
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
  return { link, kind, peer, error, frozen: linkBlocks(kind), read, current };
}

/** Heartbeats update the snapshot even if neither player has made an action. */
export function useLobbyLinkDetails(
  enabled: boolean,
  { current, read }: Pick<ReturnType<typeof useTableLink>, "current" | "read">,
) {
  useEffect(() => {
    if (!enabled) return;
    let active = true;
    let pending = false;
    const timer = window.setInterval(() => {
      if (pending) return;
      pending = true;
      void current()
        .then(({ update, freshLink }) => {
          if (active && freshLink && update) read(update.view);
        })
        .catch(() => {})
        .finally(() => (pending = false));
    }, 2000);
    return () => {
      active = false;
      window.clearInterval(timer);
    };
  }, [enabled, current, read]);
}
