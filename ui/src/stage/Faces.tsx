// The faces of the stage's nodes: React draws each object's card and each ability's stack
// item, the skin as it is (components/Card.tsx, components/Stack.tsx), into the node the stage
// made for it, and nothing else of the node. A face is drawn again only when what it is
// drawn from or its dress changes; motion never reaches React. A layout brings the faces it
// is drawn with (`Skin`): the desktop's are here, the phone's in mobile/faces.tsx.
import { memo, useSyncExternalStore, type ComponentType } from "react";
import { createPortal } from "react-dom";
import { MotionConfig } from "motion/react";
import type { ObjectView } from "../generated/protocol";
import { CARD_ASPECT, CardBack } from "../components/Card";
import { StackItem, StackMarks } from "../components/Stack";
import { Look, type CardState } from "../game/CardLayer";
import { abilityText, BLANK, useAbilityFace } from "../game/faces";
import type { DomSurface } from "./dom.ts";
import type { Dress, StackAbility } from "./layout.ts";
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

/** An ability on the stack: its source's frame, wearing a plate that says the ability (the old
 * board's AbilityOnStack). */
const AbilityFace = memo(function AbilityFace({
  ability,
  source,
  dress,
}: {
  ability: StackAbility;
  source: ObjectView | undefined;
  dress: Dress;
}) {
  const face = useAbilityFace(ability.name, source, BLANK);
  return (
    <>
      <StackItem
        source={face}
        label={ability.triggered ? "Triggered Ability" : "Activated Ability"}
        text={abilityText(face.text ?? "", ability.triggered, ability.nth)}
        width={dress.frame}
        showArt
      />
      <StackMarks controller={ability.controller} x={ability.x} />
    </>
  );
});

/** How a layout's nodes are drawn: a card's face from its record and its dress, and an
 * ability's from its stack item and its source's record. Each is memoised, so that motion
 * draws none again. */
export interface Skin {
  Card: ComponentType<{ object: ObjectView; dress: Dress }>;
  Ability: ComponentType<{
    ability: StackAbility;
    source: ObjectView | undefined;
    dress: Dress;
  }>;
}

/** The desktop's faces: the old board's cards and stack items. */
export const DESKTOP: Skin = { Card: Face, Ability: AbilityFace };

/** Every node's face. The skin turns a tapped frame with a spring of its own (Card.tsx's
 * TURN), on the browser's time; on the stage a record's change shows at once, and what moves
 * is a step's track, so the skin's motion is switched off here. */
export function Faces({
  stage,
  surface,
  skin,
}: {
  stage: ScriptStage;
  surface: DomSurface;
  skin: Skin;
}) {
  const { resting } = useSyncExternalStore(stage.subscribe, () => stage.shown);
  return (
    <MotionConfig reducedMotion="always">
      {[...resting].map(([name, { object, ability, source, dress }]) => {
        const node = surface.node(name);
        if (!node) return null;
        const face = object ? (
          <skin.Card object={object} dress={dress} />
        ) : (
          <skin.Ability ability={ability} source={source} dress={dress} />
        );
        return createPortal(face, node, `${name}`);
      })}
    </MotionConfig>
  );
}
