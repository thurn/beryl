// Manual picks stay local while the engine computes how to finish the payment on a copy.
import { useEffect, useRef, useState } from "react";
import { previewWaterbend } from "../ipc";
import type {
  CandidateIndex,
  DecisionView,
  WaterbendPreview,
} from "../generated/protocol";
import { follow, type Intent } from "./decisions";

const NO_SLOTS: number[] = [];

function paymentPlan(preview: WaterbendPreview): Intent[] {
  const { mana, benders, plan } = preview;
  return [
    ...mana.map(({ slot, color }): Intent => ({
      kind: "waterbend",
      wants: (o) => o.kind === "mana" && o.slot === slot && o.color === color,
      continuation: true,
    })),
    ...benders.map((slot): Intent => ({
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
    ...(plan === null
      ? []
      : [
          {
            kind: "paymentPlan" as const,
            wants: (o: Parameters<Intent["wants"]>[0]) =>
              o.kind === "plan" && o.index === plan,
            optional: true,
            continuation: true,
          },
        ]),
  ];
}

export function useWaterbend(
  decision: DecisionView | undefined,
  onPay: (index: CandidateIndex, then: Intent[]) => void,
) {
  const active = decision?.kind === "waterbend" ? decision : undefined;
  const key = active ? `${active.id.session}:${active.id.seq}` : undefined;
  const revision = useRef(0);
  const [edit, setEdit] = useState<{
    key: string;
    preview: WaterbendPreview;
    selected: number[];
    pending?: number[];
  }>();
  useEffect(() => {
    revision.current += 1;
    return () => {
      revision.current += 1;
    };
  }, [key]);
  const current = edit?.key === key ? edit : undefined;
  const preview = current?.preview ?? active?.waterbend;
  const selected = current?.selected ?? NO_SLOTS;
  const manual = current?.pending ?? selected;
  const ready = !!preview?.ready && !current?.pending;

  const pay = (proposal: WaterbendPreview) => {
    if (!active || !proposal.ready) return;
    const { index, rest } = follow(active, paymentPlan(proposal));
    if (index !== undefined) onPay(index, rest);
  };

  const toggle = (slot: number) => {
    if (!active || !key || !preview || current?.pending) return false;
    const removing = selected.includes(slot);
    if (!removing && !preview.eligible.includes(slot)) return false;
    const next = removing
      ? selected.filter((s) => s !== slot)
      : [...selected, slot];
    const request = ++revision.current;
    setEdit({ key, preview, selected, pending: next });
    const settle = (result: WaterbendPreview | null) => {
      if (request !== revision.current) return;
      setEdit({
        key,
        preview: result ?? preview,
        selected: result ? next : selected,
      });
      if (result?.ready && result.taps.length === next.length) pay(result);
    };
    void previewWaterbend(active.id, next).then(settle, () => settle(null));
    return true;
  };

  return {
    active: !!active,
    manual,
    benders: (preview?.benders ?? NO_SLOTS).filter((s) => manual.includes(s)),
    proposed: ready ? preview.taps : NO_SLOTS,
    ready,
    toggle,
    autoPay: () => {
      if (ready) pay(preview);
    },
  };
}
