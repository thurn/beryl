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
  const automatic = active?.waterbend;
  const preview = current
    ? current.preview
    : automatic?.benders.length === 0
      ? automatic
      : undefined;
  const selected = current?.selected ?? [];
  const eligible = (slot: number, kind: "mana" | "object") =>
    active?.candidates.some(({ option }) => {
      if (option.kind !== kind) return false;
      const clicked = objects.get(slot);
      const offered = objects.get(option.slot);
      return (
        option.slot === slot ||
        (kind === "object" && !!clicked && !!offered && twins(clicked, offered))
      );
    });
  const toggle = (slot: number) => {
    if (!active || !key) return false;
    const land = eligible(slot, "mana");
    if (!land && !eligible(slot, "object")) return false;
    const next = selected.includes(slot)
      ? selected.filter((s) => s !== slot)
      : land
        ? [slot, ...selected]
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
          ...(preview.lands ?? []).map(({ slot, color }): Intent => ({
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
    benders: selected.filter((s) => !eligible(s, "mana")),
    taps: preview
      ? [
          ...selected,
          ...preview.taps.filter(
            (s) =>
              !preview.benders.includes(s) &&
              !(preview.lands ?? []).some((land) => land.slot === s),
          ),
        ]
      : selected,
    ready: !!preview,
    toggle,
    plan,
  };
}
