// The chrome of the Revealed Cards pile (revealed.ts; UI Conventions, Verbs, showcase): the
// pill over the faces the stage rests there, which says what they are and dismisses them, and
// the browser the pile opens into, the viewer every pile of the table opens into. Arena's
// pill reads "Revealed Cards" beside a cross that puts the pile away, and a click on the pile
// opens a browser of that name (`I3Te0m1bBFw 22:08.8`, `22:09.3`, `29:46.5`).
import { useEffect, useMemo, useState, useSyncExternalStore } from "react";
import { Button } from "../components/Controls";
import type { ObjectView } from "../generated/protocol";
import { ZoneViewer } from "../game/Piles";
import { isRevealed, type Name, type Resting } from "./layout.ts";
import type { Prompts } from "./prompt.ts";
import type { PileSpace } from "./revealed.ts";
import type { ScriptStage } from "./stage.ts";

/** What the pile is called, on its pill and over its browser: Arena's words. */
export const REVEALED = "Revealed Cards";

const UNMARKED = new Map<number, never>();

/** The faces the stage rests in the pile, the first revealed first. */
export const revealedIn = (resting: ReadonlyMap<Name, Resting>): ObjectView[] =>
  [...resting]
    .filter(([name]) => isRevealed(name))
    .sort(([, a], [, b]) => a.pose.depth - b.pose.depth)
    .flatMap(([, { object }]) => (object ? [object] : []));

/** Whether the pile's browser is open, and how to open or close it: a click on a face of
 * the pile opens it, and it closes with the pile's last face. */
export function useRevealedOpen(
  prompts: Prompts,
  cards: readonly ObjectView[],
): [open: boolean, setOpen: (open: boolean) => void] {
  const [open, setOpen] = useState(false);
  useEffect(
    () =>
      prompts.listen({
        click(name) {
          if (isRevealed(name)) setOpen(true);
        },
      }),
    [prompts],
  );
  const none = cards.length === 0;
  useEffect(() => {
    if (none) setOpen(false);
  }, [none]);
  return [open && !none, setOpen];
}

/** The desktop's pill over the pile, which lies in `space`, and the pile's browser while it
 * is open. Nothing while the pile holds nothing. */
export function RevealedCards({
  stage,
  prompts,
  space,
  onDismiss,
}: {
  stage: ScriptStage;
  prompts: Prompts;
  space: PileSpace;
  /** Puts the pile's `cards` away: they lie nowhere from here on. */
  onDismiss(cards: readonly ObjectView[]): void;
}) {
  const { resting } = useSyncExternalStore(stage.subscribe, () => stage.shown);
  const cards = useMemo(() => revealedIn(resting), [resting]);
  const [open, setOpen] = useRevealedOpen(prompts, cards);
  if (cards.length === 0) return null;
  return (
    <>
      <div
        className="hud stage-revealed"
        style={{ left: space.x - 6, top: space.y - 34 }}
      >
        <Button variant="secondary" onClick={() => setOpen(true)}>
          {REVEALED}
        </Button>
        <button
          type="button"
          className="stage-revealed-dismiss"
          aria-label="Dismiss"
          onClick={() => onDismiss(cards)}
        >
          <svg viewBox="0 0 24 24" aria-hidden>
            <path d="M5.6 3.5 12 9.9l6.4-6.4 2.1 2.1-6.4 6.4 6.4 6.4-2.1 2.1-6.4-6.4-6.4 6.4-2.1-2.1 6.4-6.4-6.4-6.4z" />
          </svg>
        </button>
      </div>
      {open && (
        <ZoneViewer
          pile={{ cards: [...cards] }}
          title={REVEALED}
          states={UNMARKED}
          offers={() => false}
          onPick={() => {}}
          onClose={() => setOpen(false)}
        />
      )}
    </>
  );
}
