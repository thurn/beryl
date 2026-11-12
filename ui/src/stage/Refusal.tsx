// Why a click is refused, on the desktop's stage (UI Conventions, "Why a click is refused"). The
// session says with each decision which cards it will not take and why (`DecisionView.refused`);
// a click on one changes nothing of the prompt, so the prompt hands it on (prompt.ts `listen`),
// and it is answered here: a sound, the reason under the card's inspector while the pointer
// stays on it, and for a timing an opposing permanent imposes, a word at that permanent for a
// moment. Nothing here works out what is allowed.
import { useEffect, useRef, useState } from "react";
import { play } from "../audio/sound";
import type { DecisionView, Refused } from "../generated/protocol";
import { desktop } from "./desktop.ts";
import type { Name, Resting } from "./layout.ts";
import type { Asked, Prompts } from "./prompt.ts";

/** How long the word at the permanent that forbids a click stays, in ms: the old board's. */
const NOTICE_MS = 1800;

interface Said {
  refused: Refused;
  /** The decision that refused it: a later one says its own. */
  decision: DecisionView;
}

/** The click the open decision last refused: `refused` while the pointer stays on its card,
 * for the inspector, and `notice`, the opposing permanent on the battlefield whose timing rule
 * forbade it, for a moment after the click wherever the pointer goes. */
export function useRefusal(
  prompts: Prompts,
  { decision }: Asked,
  resting: ReadonlyMap<Name, Resting>,
) {
  const [said, setSaid] = useState<Said>();
  const [noticed, setNoticed] = useState<Said & { id: number }>();
  const open = useRef(decision);
  open.current = decision;
  const ids = useRef(0);
  useEffect(
    () =>
      prompts.listen({
        click(name) {
          const decision = open.current;
          const refused = decision?.refused.find((r) => r.slot === name);
          if (!decision || !refused) return;
          play("no");
          setSaid({ refused, decision });
          setNoticed({ refused, decision, id: ++ids.current });
        },
        hover(name) {
          setSaid((s) => (s?.refused.slot === name ? s : undefined));
        },
      }),
    [prompts],
  );
  useEffect(() => {
    if (!noticed) return;
    const timer = setTimeout(() => setNoticed(undefined), NOTICE_MS);
    return () => clearTimeout(timer);
  }, [noticed]);
  const reason =
    noticed?.decision === decision ? noticed?.refused.reason : undefined;
  const by = reason?.kind === "timing" ? reason.by : null;
  const source = by === null ? undefined : resting.get(by);
  const forbids =
    source?.object?.name != null &&
    source.dress.zone === "battlefield" &&
    source.object.controller === "opponent";
  return {
    refused: said?.decision === decision ? said?.refused : undefined,
    notice:
      source && forbids && noticed ? { source, id: noticed.id } : undefined,
  };
}

/** Where the word stands: centred under the permanent `source`, and the room it takes there,
 * which the inspector keeps clear of. */
export function noticePlace(source: Resting) {
  const middle = desktop.middle(source, source.pose);
  const half = desktop.half(source, source.pose);
  const left = middle.x;
  const top = middle.y + half.y + 6;
  return {
    left,
    top,
    box: { left: left - 84, right: left + 84, top, bottom: top + 36 },
  };
}

/** The word at the permanent that holds its controller's opponents to sorcery speed. */
export function TimingRefusal({ source }: { source: Resting }) {
  const { left, top } = noticePlace(source);
  return (
    <div
      className="timing-refusal stage-timing-refusal"
      role="status"
      style={{ left, top }}
    >
      Sorcery speed only
    </div>
  );
}
