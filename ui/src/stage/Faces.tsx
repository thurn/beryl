// The faces of the stage's nodes: React draws each object's card and each ability's stack
// item, the skin as it is (components/Card.tsx, components/Stack.tsx), into the node the stage
// made for it, and nothing else of the node. A face is drawn again only when what it is
// drawn from or its dress changes; motion never reaches React. A layout brings the faces it
// is drawn with (`Skin`): the desktop's are here, the phone's in mobile/faces.tsx.
import { memo, useSyncExternalStore, type ComponentType } from "react";
import { createPortal } from "react-dom";
import type { ObjectView } from "../generated/protocol";
import { CARD_ASPECT, CardBack } from "../components/Card";
import { StackItem, StackMarks } from "../components/Stack";
import { Look, type CardState } from "../game/Look";
import { abilityText, BLANK, useAbilityFace } from "../game/faces";
import type { DomSurface } from "./dom.ts";
import type { Dress, StackAbility } from "./layout.ts";
import type { ScriptStage } from "./stage.ts";

/** The skin's own highlights mark no card here: a mark of script/verb.rs is the stage's, an
 * element of its own on the node with its one look (spans.ts, stage.css). The one the skin
 * draws is the ring of what is attached to the permanent the pointer is on, or what that is
 * attached to (`linked`): the pointer's, and no script's. A spell on the stack wears how it
 * was cast, as its dress has it. */
const UNMARKED: CardState = {};
const LINKED: CardState = { linked: true };

const Face = memo(function Face({
  object,
  dress,
  linked,
}: {
  object: ObjectView;
  dress: Dress;
  linked?: boolean;
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
      state={linked ? LINKED : (dress.cast ?? UNMARKED)}
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
        text={
          ability.text ??
          abilityText(face.text ?? "", ability.triggered, ability.nth)
        }
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
  Card: ComponentType<{ object: ObjectView; dress: Dress; linked?: boolean }>;
  Ability: ComponentType<{
    ability: StackAbility;
    source: ObjectView | undefined;
    dress: Dress;
  }>;
}

/** The desktop's faces: full cards and stack items. */
export const DESKTOP: Skin = { Card: Face, Ability: AbilityFace };

/** Every node's face. A record's change shows at once: what moves is a step's track. */
export function Faces({
  stage,
  surface,
  skin,
  linked,
}: {
  stage: ScriptStage;
  surface: DomSurface;
  skin: Skin;
  /** The permanents ringed as linked to the one the pointer is on. */
  linked?: readonly number[];
}) {
  const { resting } = useSyncExternalStore(stage.subscribe, () => stage.shown);
  return (
    <>
      {[...resting].map(([name, { object, ability, source, dress }]) => {
        const node = surface.node(name);
        if (!node) return null;
        const face = object ? (
          <skin.Card
            object={object}
            dress={dress}
            linked={linked?.includes(object.slot)}
          />
        ) : (
          <skin.Ability ability={ability} source={source} dress={dress} />
        );
        return createPortal(face, node, `${name}`);
      })}
    </>
  );
}
