// A card as the table's skin draws it at a placement: its frame, its marks and how it was
// cast. The stage draws each object's node with it (stage/Faces.tsx).
import { Card, CardBack, typeUnit } from "../components/Card";
import { ManaCost } from "../components/Mana";
import { StackMarks } from "../components/Stack";
import type { ObjectView, PaidCost } from "../generated/protocol";
import { faceDownFace, marksOf, useFace } from "./faces";
import type { Placement } from "./placement";

/** How a card is shown beyond its placement. */
export interface CardState {
  /** It is attached to the card the pointer is over, or that card is attached to it. */
  linked?: boolean;
  /** On the stack, the X its spell was announced with. */
  x?: number;
  /** On the stack, the alternative and additional costs its spell was cast with. */
  paid?: PaidCost[];
}

/** The card itself, as its placement draws it. */
export function Look({
  object,
  place,
  state,
}: {
  object: ObjectView;
  place: Pick<Placement, "look" | "width" | "count">;
  state: CardState;
}) {
  const card = useFace(object);
  const under = place.look === "beneath" || place.look === "attached";
  // A face-down permanent shows its back to both players; its controller's hover still names
  // its card (CR 708.5). On the table it keeps a permanent's frame, its marks and its 2/2.
  const down = object.facing === "down";
  const field =
    under ||
    place.look === "field" ||
    place.look === "land" ||
    (down && place.look === "shown" && object.zone === "battlefield");
  if (down ? !field : object.name === null) {
    return <CardBack width={place.width} />;
  }
  const combat = field ? object.combat?.kind : undefined;
  // A card your hand fan holds because you can play it from where it lies (a graveyard, exile):
  // its own outline, apart from the hand's playable edge.
  const elsewhere = place.look === "hand" && object.zone !== "hand";
  const card_ = (
    <Card
      card={down ? faceDownFace(object) : card}
      back={down}
      combat={
        combat === "attacking"
          ? "attacker"
          : combat === "blocking"
            ? "blocker"
            : undefined
      }
      variant={field ? "field" : "full"}
      marks={field ? marksOf(object) : undefined}
      count={place.count}
      width={place.width}
      tapped={field && object.tapped}
      className={
        [
          elsewhere && "card-elsewhere",
          state.linked && "card-linked",
          under && "card-under",
        ]
          .filter(Boolean)
          .join(" ") || undefined
      }
    />
  );
  if (place.look === "stack") {
    return (
      <div className="on-stack">
        {card_}
        <StackMarks
          controller={object.controller}
          x={state.x ?? 0}
          paid={state.paid}
        />
      </div>
    );
  }
  return field ? (
    <div className="on-table">{card_}</div>
  ) : place.look === "hand" ? (
    <div className="hand-card">
      {card_}
      {/* The cost stands above the card's top right corner rather than in its name bar, where
          the next card of a crowded hand would cover it, as Arena floats each hand card's cost
          above the fan (_IBqPx0gcag 12:00, uzg_P976qhc 17:20). Above the frame, it is clear
          of the next card, which covers only the frame's right part. */}
      {card.manaCost && (
        <span className="hand-cost" style={{ fontSize: typeUnit(place.width) }}>
          <ManaCost cost={card.manaCost} />
        </span>
      )}
    </div>
  ) : (
    card_
  );
}
