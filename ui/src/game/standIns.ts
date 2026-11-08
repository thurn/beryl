// Interchangeable cards in hand collapse to one candidate (architecture §7.2): of two copies
// of a basic land the engine offers one, the lowest slot, whichever the player drags. Without
// more, the copy the player let go of would fly back to the hand while its twin went into play. So the card
// element the player handled takes over the identity of the card the engine moved: once that
// card leaves the hand, each twin's element shows the other, for the rest of the game. Twins
// look alike, so nothing on screen jumps; the card the player handled is the one that goes.
import { useRef } from "react";
import type { PlayerView } from "../generated/protocol";
import { slotOf, type Act } from "./decisions";

interface Touched {
  /** The hand card the player handled. */
  touched: number;
  /** The interchangeable card the engine's candidates name in its place. */
  offered: number;
}

/** Which element (by key) shows each object; the player's touch of a hand card, which
 * `keys` honors once the card the engine offered for it moves. */
export interface StandIns {
  keys: ReadonlyMap<number, number>;
  /** The player handled the hand card in `slot`, which offers `acts`. */
  touch(slot: number, acts: readonly Act[] | undefined): void;
}

export function useStandIns(view: PlayerView): StandIns {
  const state = useRef<{
    keys: Map<number, number>;
    pending?: Touched;
    last?: PlayerView;
  }>({ keys: new Map() });
  const s = state.current;
  if (s.last !== view) {
    const before = s.last;
    s.last = view;
    const pending = s.pending;
    s.pending = undefined;
    const was =
      pending && before?.objects.find((o) => o.slot === pending.offered);
    const now = pending && view.objects.find((o) => o.slot === pending.offered);
    if (pending && was?.zone === "hand" && now && now.zone !== "hand") {
      const { touched, offered } = pending;
      const [a, b] = [
        s.keys.get(touched) ?? touched,
        s.keys.get(offered) ?? offered,
      ];
      s.keys.set(touched, b);
      s.keys.set(offered, a);
    }
  }
  return {
    keys: s.keys,
    touch(slot, acts) {
      const object = view.objects.find((o) => o.slot === slot);
      if (object?.zone !== "hand") return;
      const offered = acts
        ?.map((a) => slotOf(a.option))
        .find((x) => x !== undefined);
      s.pending =
        offered !== undefined && offered !== slot
          ? { touched: slot, offered }
          : undefined;
    },
  };
}
