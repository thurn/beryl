// Waterbend is edited locally. Only Confirm sends the tap choices to the engine; previews
// run the real payment rules on a copy and stale replies never replace a newer selection.
import { useEffect, useMemo, useRef, useState } from "react";
import { previewWaterbend } from "../ipc";
import type { DecisionView, WaterbendPreview } from "../generated/protocol";
import type { Intent } from "./decisions";

const NO_SLOTS: number[] = [];

export function useWaterbend(decision: DecisionView | undefined) {
  const active = decision?.kind === "waterbend" ? decision : undefined;
  const key = active ? `${active.id.session}:${active.id.seq}` : undefined;
  const revision = useRef(0);
  const [edit, setEdit] = useState<{
    key: string;
    preview: WaterbendPreview;
    pending?: number[];
  }>();
  useEffect(() => {
    revision.current += 1;
  }, [key]);
  const current = edit?.key === key ? edit : undefined;
  const suggestion = active?.waterbend;
  const preview = current?.preview ?? suggestion;
  const selected = current?.pending ?? preview?.taps ?? NO_SLOTS;
  const eligible = preview?.eligible ?? NO_SLOTS;
  // Suggested sources stay upright until payment. An explicit addition can preview its tap.
  const previewTaps = useMemo(
    () => selected.filter((slot) => !suggestion?.taps.includes(slot)),
    [selected, suggestion],
  );

  const toggle = (slot: number) => {
    if (!active || !key || !preview || current?.pending) return false;
    const removing = selected.includes(slot);
    if (!removing && !eligible.includes(slot)) return false;
    const next = removing
      ? selected.filter((s) => s !== slot)
      : [...selected, slot];
    const request = ++revision.current;
    setEdit({ key, preview, pending: next });
    const settle = (result: WaterbendPreview | null) => {
      if (request === revision.current)
        setEdit({ key, preview: result ?? preview });
    };
    void previewWaterbend(active.id, next).then(settle, () => settle(null));
    return true;
  };
  const plan = (): Intent[] =>
    preview?.ready && !current?.pending
      ? [
          ...(preview.mana ?? []).map(({ slot, color }): Intent => ({
            kind: "waterbend",
            wants: (o) =>
              o.kind === "mana" && o.slot === slot && o.color === color,
            continuation: true,
          })),
          ...preview.benders.map((slot): Intent => ({
            kind: "waterbend",
            wants: (o) => o.kind === "object" && o.slot === slot,
            continuation: true,
          })),
          {
            kind: "waterbend",
            wants: (o) => o.kind === "done",
            optional: true,
            continuation: true,
          },
          ...(preview.plan === null
            ? []
            : [
                {
                  kind: "paymentPlan" as const,
                  wants: (o: Parameters<Intent["wants"]>[0]) =>
                    o.kind === "plan" && o.index === preview.plan,
                  optional: true,
                  continuation: true,
                },
              ]),
        ]
      : [];
  return {
    active: !!active,
    selected,
    previewTaps,
    benders: (preview?.benders ?? []).filter((s) => selected.includes(s)),
    eligible,
    ready: !!preview?.ready && !current?.pending,
    toggle,
    plan,
  };
}
