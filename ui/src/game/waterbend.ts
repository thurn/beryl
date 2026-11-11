// A payment made by picking permanents. The session holds the prompt's state: each pick is a
// click it answers with the whole new state, and its marks say what is picked to pay, what
// Auto Pay would tap for the rest and what else could pay.
import { useEffect, useRef, useState } from "react";
import { click } from "../ipc";
import type { Click, DecisionView, PromptView } from "../generated/protocol";

const NO_SLOTS: number[] = [];

/** The permanents `prompt` marks with `mark`. */
const marked = (
  prompt: PromptView,
  mark: PromptView["marks"][number]["mark"],
) =>
  prompt.marks.flatMap((m) =>
    m.mark === mark && m.on.kind === "object" ? [m.on.slot] : [],
  );

export function useWaterbend(decision: DecisionView | undefined) {
  const active =
    decision?.prompt.purpose === "waterbend" &&
    decision.prompt.state?.shape === "pay"
      ? decision
      : undefined;
  const key = active ? `${active.id.session}:${active.id.seq}` : undefined;
  const request = useRef(0);
  const [edit, setEdit] = useState<{
    key: string;
    prompt: PromptView;
    pending?: number[];
  }>();
  useEffect(() => {
    request.current += 1;
    return () => {
      request.current += 1;
    };
  }, [key]);
  const current = edit?.key === key ? edit : undefined;
  const prompt = current?.prompt ?? active?.prompt;
  const paying = prompt ? marked(prompt, "paying") : NO_SLOTS;
  const manual = current?.pending ?? paying;
  const ready =
    !!prompt?.buttons.some((b) => b.id.kind === "auto" && b.enabled) &&
    !current?.pending;

  /** Sends `sent`, showing `pending` as picked until the session's reply says what is. A
   * click that answers leaves them shown until the game's next view arrives. */
  const send = (sent: Click, pending: number[]) => {
    if (!active || !key || !prompt || current?.pending) return false;
    const id = ++request.current;
    setEdit({ key, prompt, pending });
    void click(active.id, prompt.revision, sent).then(
      (reply) => {
        if (id === request.current && reply.kind === "open")
          setEdit({ key, prompt: reply.prompt });
      },
      () => {
        if (id === request.current) setEdit({ key, prompt });
      },
    );
    return true;
  };

  const toggle = (slot: number) => {
    if (!prompt) return false;
    const removing = paying.includes(slot);
    const pickable =
      marked(prompt, "eligible").includes(slot) ||
      marked(prompt, "suggested").includes(slot);
    if (!removing && !pickable) return false;
    return send(
      { kind: "on", on: { kind: "object", slot } },
      removing ? paying.filter((s) => s !== slot) : [...paying, slot],
    );
  };

  return {
    active: !!active,
    manual,
    benders: paying.filter((s) => manual.includes(s)),
    proposed:
      ready && prompt ? [...paying, ...marked(prompt, "suggested")] : NO_SLOTS,
    ready,
    toggle,
    autoPay: () => {
      if (ready) send({ kind: "button", button: { kind: "auto" } }, paying);
    },
  };
}
