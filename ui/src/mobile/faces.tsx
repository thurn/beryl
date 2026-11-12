// How a phone draws a card (DESIGN.md of the approved prototype): its full scan, only live
// state drawn over it in the desktop's badges. The stage draws them in its nodes (`PHONE`, the
// phone's skin for stage/Faces.tsx).
import { memo } from "react";
import type { CSSProperties } from "react";
import type { ObjectView } from "../generated/protocol";
import {
  Card,
  CardBack,
  Loyalty,
  type CardFace,
  type CardProps,
} from "../components/Card";
import { faceDownFace, marksOf, SCAN, useFace } from "../game/faces";
import { useCardArt } from "../cards";
import type { Skin } from "../stage/Faces.tsx";

/** The em of the badges on a card `width` wide (phone.css `--badge`): a loyalty or a face-down
 * card's P/T numeral stays at least 12px however small the scan. */
export const badgeEm = (width: number) =>
  `${Math.min(11, Math.max(7.2, width / 34))}px`;

/** How a phone draws `face`: its scan alone, with no stats over it but the badges passed in;
 * or, until the scan loads, a battlefield frame with its name and art, all a card this small
 * has room for. */
function scanOrFrame(face: CardFace): Pick<CardProps, "card" | "variant"> {
  return face.scan
    ? { card: { ...face, loyalty: undefined }, variant: "full" }
    : {
        card: { name: face.name, typeLine: face.typeLine, colors: face.colors },
        variant: "field",
      };
}

/** A card on the table (`field`) or in your hand, `width` wide. */
export function CardScan({
  object,
  width,
  field,
}: {
  object: ObjectView;
  width: number;
  field: boolean;
}) {
  const face = useFace(object);
  const down = object.facing === "down";
  const loyalty = field
    ? object.counters.find((c) => c.kind === "loyalty")?.count
    : undefined;
  if (!down && object.name === null) return <CardBack width={width} />;
  return (
    <Card
      // A face-down card shows its back and its P/T; a face-up one its scan, its loyalty
      // drawn here over the printed shield.
      {...(down
        ? { card: faceDownFace(object), variant: "full" }
        : scanOrFrame(face))}
      back={down}
      width={width}
      tapped={field && object.tapped}
      marks={
        field ? { counters: marksOf(object).counters, sick: false } : undefined
      }
      style={{ "--badge": badgeEm(width) } as CSSProperties}
    >
      {loyalty !== undefined && !down && <Loyalty loyalty={String(loyalty)} />}
    </Card>
  );
}

/** A spell on the stack. */
function SpellScan({ object, width }: { object: ObjectView; width: number }) {
  const face = useFace(object);
  return <Card {...scanOrFrame(face)} width={width} />;
}

/** An ability on the stack: its source's scan, by name, since the source may have left. */
function AbilityScan({ name, width }: { name: string; width: number }) {
  const art = useCardArt(name, undefined, SCAN);
  return (
    <Card
      {...scanOrFrame({
        name,
        typeLine: art?.face.type_line ?? "",
        colors: [],
        scan: art?.src,
      })}
      width={width}
    />
  );
}

/** A graveyard's top card, as small as the library beside it. */
export function PileTop({
  object,
  width,
}: {
  object: ObjectView;
  width: number;
}) {
  const face = useFace(object);
  return <Card {...scanOrFrame(face)} width={width} />;
}

/** The phone's faces for the stage's nodes, by where each rests (stage/phone.ts): nothing
 * under the top of a pile, a back for a card that shows none of its face there, and the
 * phone's scans for the rest. */
export const PHONE: Skin = {
  Card: memo(function PhoneFace({ object, dress }) {
    if (dress.look === "hidden") return null;
    if (dress.back) return <CardBack width={dress.frame} />;
    if (dress.look === "pile") {
      return <PileTop object={object} width={dress.frame} />;
    }
    if (dress.look === "stack") {
      return <SpellScan object={object} width={dress.frame} />;
    }
    return (
      <CardScan
        object={object}
        width={dress.frame}
        field={dress.zone === "battlefield"}
      />
    );
  }),
  Ability: memo(function PhoneAbilityFace({ ability, dress }) {
    return <AbilityScan name={ability.name} width={dress.frame} />;
  }),
};
