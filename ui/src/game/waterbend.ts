// Waterbend is edited locally. Only Confirm sends the tap choices to the engine; previews
// run the real payment rules on a copy and stale replies never replace a newer selection.
import { useEffect, useRef, useState } from "react";
import { previewWaterbend } from "../ipc";
import type {
  DecisionView,
  WaterbendPreview,
  ObjectView,
  Mana,
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
    picks: { slot: number; color?: Mana }[];
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
  const picks = current?.picks ?? [];
  const selected = picks.map((pick) => pick.slot);
  const benders = picks.flatMap((pick) =>
    pick.color === undefined ? [pick.slot] : [],
  );
  const eligible = (slot: number, color?: Mana) =>
    active?.candidates.some(({ option }) => {
      if (color !== undefined)
        return (
          option.kind === "mana" &&
          option.slot === slot &&
          option.color === color
        );
      if (option.kind !== "object") return false;
      const clicked = objects.get(slot);
      const offered = objects.get(option.slot);
      return (
        option.slot === slot ||
        (!!clicked && !!offered && twins(clicked, offered))
      );
    });
  const toggle = (slot: number, color?: Mana) => {
    if (!active || !key) return false;
    const removing = selected.includes(slot);
    if (!removing && !eligible(slot, color)) return false;
    const next = removing
      ? picks.filter((pick) => pick.slot !== slot)
      : [...picks, { slot, color }];
    const bend = next.flatMap((pick) =>
      pick.color === undefined ? [pick.slot] : [],
    );
    const mana = next.flatMap(({ slot, color }) =>
      color === undefined ? [] : [{ slot, color }],
    );
    const request = ++revision.current;
    setEdit({ key, picks: next });
    void previewWaterbend(active.id, bend, mana).then(
      (result) => {
        if (request === revision.current)
          setEdit({ key, picks: next, preview: result });
      },
      () => {
        if (request === revision.current)
          setEdit({ key, picks: next, preview: null });
      },
    );
    return true;
  };
  const plan = (): Intent[] =>
    preview
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
    benders,
    taps: preview
      ? [
          ...selected,
          ...preview.taps.filter(
            (s) => !preview.benders.includes(s) && !selected.includes(s),
          ),
        ]
      : selected,
    ready: !!preview,
    toggle,
    plan,
  };
}
