// Waterbend is edited locally. Only Confirm sends the tap choices to the engine; previews
// run the real payment rules on a copy and stale replies never replace a newer selection.
import { useEffect, useRef, useState } from "react";
import { previewWaterbend } from "../ipc";
import type {
  DecisionView,
  WaterbendPreview,
  ObjectView,
} from "../generated/protocol";
import { twins, type Intent } from "./decisions";

export function useWaterbend(
  decision: DecisionView | undefined,
  objects: Map<number, ObjectView>,
) {
  const active = decision?.kind === "waterbend" ? decision : undefined;
  const key = active ? `${active.id.session}:${active.id.seq}` : undefined;
  const revision = useRef(0);
  const [edit, setEdit] = useState<{
    key: string;
    selected: number[];
    preview?: WaterbendPreview | null;
  }>();
  useEffect(() => {
    revision.current += 1;
  }, [key]);
  const current = edit?.key === key ? edit : undefined;
  const preview = current ? current.preview : active?.waterbend;
  const selected = current?.selected ?? active?.waterbend?.benders ?? [];
  const toggle = (slot: number) => {
    if (!active || !key) return false;
    if (
      !active.candidates.some(
        (c) =>
          c.option.kind === "object" &&
          (c.option.slot === slot ||
            (!!objects.get(slot) &&
              !!objects.get(c.option.slot) &&
              twins(objects.get(slot)!, objects.get(c.option.slot)!))),
      )
    )
      return false;
    const next = selected.includes(slot)
      ? selected.filter((s) => s !== slot)
      : [...selected, slot];
    const request = ++revision.current;
    setEdit({ key, selected: next });
    void previewWaterbend(active.id, next).then(
      (result) => {
        if (request === revision.current)
          setEdit({ key, selected: next, preview: result });
      },
      () => {
        if (request === revision.current)
          setEdit({ key, selected: next, preview: null });
      },
    );
    return true;
  };
  const plan = (): Intent[] =>
    preview
      ? [
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
    taps: preview
      ? [
          ...selected,
          ...preview.taps.filter((s) => !preview.benders.includes(s)),
        ]
      : selected,
    ready: !!preview,
    toggle,
    plan,
  };
}
