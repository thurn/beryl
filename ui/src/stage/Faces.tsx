// The faces of the stage's nodes: React draws each object's card, the skin as it is
// (components/Card.tsx), into the node the stage made for it, and nothing else of the node. A
// face is drawn again only when its object's record or its dress changes; motion never
// reaches React.
import { memo, useSyncExternalStore } from "react";
import { createPortal } from "react-dom";
import { MotionConfig } from "motion/react";
import type { ObjectView } from "../generated/protocol";
import { CARD_ASPECT, CardBack } from "../components/Card";
import { Look, type CardState } from "../game/CardLayer";
import type { DomSurface } from "./dom.ts";
import type { Dress } from "./layout.ts";
import type { ScriptStage } from "./stage.ts";

/** No prompt marks a card yet (the marks of script/verb.rs come with the prompt shapes). */
const UNMARKED: CardState = {};

const Face = memo(function Face({
  object,
  dress,
}: {
  object: ObjectView;
  dress: Dress;
}) {
  // Under the top of a pile nothing of a card shows, or only its paper edge.
  if (dress.look === "hidden") return null;
  if (dress.rim) {
    return (
      <div
        className="pile-card-rim"
        style={{ width: dress.frame, height: dress.frame / CARD_ASPECT }}
        aria-hidden
      />
    );
  }
  if (dress.back) return <CardBack width={dress.frame} />;
  return (
    <Look
      object={object}
      place={{ look: dress.look, width: dress.frame, count: dress.count }}
      state={UNMARKED}
    />
  );
});

/** Every node's face. The skin turns a tapped frame with a spring of its own (Card.tsx's
 * TURN), on the browser's time; on the stage a record's change shows at once, and what moves
 * is a step's track, so the skin's motion is switched off here. */
export function Faces({
  stage,
  surface,
}: {
  stage: ScriptStage;
  surface: DomSurface;
}) {
  const { resting } = useSyncExternalStore(stage.subscribe, () => stage.shown);
  return (
    <MotionConfig reducedMotion="always">
      {[...resting].map(([slot, { object, dress }]) => {
        const node = surface.node(slot);
        return (
          node &&
          createPortal(<Face object={object} dress={dress} />, node, `${slot}`)
        );
      })}
    </MotionConfig>
  );
}
