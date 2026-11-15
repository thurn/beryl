import { useEffectEvent, useLayoutEffect, useRef, useState } from "react";
import { tableAnswer, type Answer, type PlayerView } from "../ipc";
import { OpeningPresentation } from "../screens/Opening";
import type { TableClient } from "./client";

export function TableOpening({
  client,
  view,
}: {
  client: TableClient;
  view: PlayerView;
}) {
  const prompt = view.table?.prompt;
  const choosingFirst = view.table?.choosingFirst && prompt?.kind === "choose";
  const revision =
    prompt?.kind === "opening"
      ? `${prompt.mulligans}:${prompt.toBottom}:${prompt.bottom.join(",")}`
      : prompt?.kind;
  const [answered, setAnswered] = useState<string>();
  const [pendingReply, setPendingReply] = useState(false);
  const [error, setError] = useState<string>();
  const pending = useRef(false);
  // The reply can arrive before the hand has finished animating. Disable
  // the old prompt until the stage draws the answer's effect.
  const sending =
    pendingReply || (answered !== undefined && answered === revision);

  function send(...answers: Answer[]) {
    if (client.frozen || pending.current || sending) return;
    pending.current = true;
    setPendingReply(true);
    setAnswered(revision);
    setError(undefined);
    void (async () => {
      try {
        for (const answer of answers) {
          const result = await tableAnswer(answer);
          if ("refused" in result) {
            setAnswered(undefined);
            setError("Could not apply that choice.");
            return;
          }
        }
      } catch {
        setAnswered(undefined);
        setError("Could not reach the table.");
      } finally {
        pending.current = false;
        setPendingReply(false);
      }
    })();
  }

  const cancel = useEffectEvent(() => send({ kind: "cancel" }));
  useLayoutEffect(() => client.pushOpen(() => cancel()), [client]);

  if (choosingFirst) {
    return (
      <OpeningPresentation
        view={view}
        starting={null}
        choosingFirst
        showCoin={false}
        title={prompt.title}
        inHand={[]}
        bottom={[]}
        buttons={[
          ...prompt.options.map((label, option) => ({
            label,
            run: () => send({ kind: "pick", option }, { kind: "done" }),
          })),
          { label: "Skip", run: () => send({ kind: "cancel" }) },
        ]}
        {...{ sending, error }}
      />
    );
  }

  if (prompt?.kind !== "opening") return null;
  const objects = new Map(view.objects.map((object) => [object.slot, object]));
  const cards = (slots: number[]) =>
    slots.flatMap((slot) => {
      const card = objects.get(slot);
      return card ? [card] : [];
    });
  const hand = cards(prompt.hand);
  const bottom = cards(prompt.bottom);
  const picking = prompt.toBottom != null;
  const toBottom = prompt.toBottom ?? 0;
  return (
    <OpeningPresentation
      view={view}
      starting={view.turn.active}
      inHand={hand}
      showCoin={false}
      title={
        picking
          ? "Put on the bottom"
          : prompt.mulligans === 0
            ? "Opening hand"
            : `Mulligan to ${Math.max(0, hand.length - prompt.mulligans)}`
      }
      canInspect={!picking}
      buttons={
        picking
          ? []
          : [
              { label: "Keep", run: () => send({ kind: "keep" }) },
              { label: "Mulligan", run: () => send({ kind: "mulligan" }) },
            ]
      }
      toggle={(card) => send({ kind: "bottom", card: card.slot })}
      confirm={() => send({ kind: "done" })}
      done={bottom.length === 0 || bottom.length === toBottom}
      {...{ bottom, picking, toBottom, sending, error }}
    />
  );
}
