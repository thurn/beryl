// The stack-item frame (a spell or ability on the stack) and the marks of how it was cast.
import {
  AbilityPlate,
  CardArt,
  CARD_ASPECT,
  frameKey,
  type CardFace,
} from "./Card";
import { useTextFit } from "./fit";
import { RulesText } from "./Mana";
import type { PaidCost } from "../generated/protocol";

export interface StackItemProps {
  source: CardFace;
  /** What the item is, such as "Triggered Ability"; omitted for a spell. */
  label?: string;
  text: string;
  showArt?: boolean;
  width?: number;
  /** The item's height, when it is not the frame's usual aspect (as without art). */
  height?: number;
  /** The frame's unit in pixels, when not width / 12. */
  unit?: number;
}

/** A stack item: with art, its source's scan wearing a plate that says the item; without it,
 * or until the scan loads, the source's name bar over a shield-shaped rules box. */
export function StackItem({
  source,
  label,
  text,
  showArt = false,
  width = 170,
  height,
  unit,
}: StackItemProps) {
  const nameRef = useTextFit(source.name);
  if (showArt && source.scan) {
    return (
      <div
        className="stack-item stack-item-scanned"
        style={{ width, height: width / CARD_ASPECT, fontSize: width / 12 }}
      >
        <img className="card-scan" src={source.scan} alt={source.name} />
        <AbilityPlate label={label} text={text} />
      </div>
    );
  }
  return (
    <div
      className={`stack-item frame-${frameKey(source)}${showArt ? "" : " stack-item-compact"}`}
      style={{ width, height, fontSize: unit ?? width / 12 }}
    >
      <div className="card-rim">
        <div className="card-name-bar">
          <span className="card-name" ref={nameRef}>
            {source.name}
          </span>
        </div>
        {showArt && <CardArt card={source} />}
        {label && <div className="card-type-bar">{label}</div>}
        <div className="card-rules">
          <p>
            <RulesText text={text} />
          </p>
        </div>
      </div>
    </div>
  );
}

/** The keyword of each cost a spell can be cast with, as its card prints it. */
const PAID: Record<PaidCost, string> = {
  kicker: "Kicker",
  offspring: "Offspring",
  impending: "Impending",
  overload: "Overload",
  evoke: "Evoke",
  flashback: "Flashback",
};

/** What a stack item's frame cannot say, on its edges: whose it is, named for assistive
 * technology only (a corner mark in each player's colour said nothing a player could read), the
 * X it was announced with, spell or ability alike, and
 * the alternative or additional costs a spell was cast with, as Arena tags them, since its
 * printed text does not say what they change (an overloaded spell targets nothing). An X of 0
 * goes unmarked, as for an item without X. */
export function StackMarks({
  controller,
  x,
  paid = [],
}: {
  controller: "you" | "opponent";
  x: number;
  paid?: PaidCost[];
}) {
  return (
    <>
      <span
        className={`stack-controller stack-controller-${controller}`}
        data-controller={controller}
        role="img"
        aria-label={controller === "you" ? "Yours" : "Opponent's"}
      />
      {x > 0 && (
        <span className="stack-x" aria-label={`X is ${x}`}>
          X={x}
        </span>
      )}
      {paid.length > 0 && (
        <span className="stack-paid" data-paid={paid.join(" ")}>
          {paid.map((p) => PAID[p]).join(" · ")}
        </span>
      )}
    </>
  );
}
