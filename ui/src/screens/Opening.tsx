// The opening over the dimmed table: the coin flip, keeping or mulliganing a hand and
// the cards put on the bottom.
import { Button, Dock, DockButtons } from "../components/Controls";
import { type CSSProperties, useEffect, useState } from "react";
import {
  type CandidateIndex,
  type DecisionView,
  type ObjectView,
  type PlayerView,
} from "../ipc";
import { Card, CardBack } from "../components/Card";
import { Coin } from "../components/Player";
import { type HoverHandlers, useHoverInspect } from "../game/Piles";
import { type Intent, twins } from "../game/decisions";
import { useFace } from "../game/faces";

/** The starting hand size (CR 103.5). */
const STARTING_HAND = 7;
/** How long the coin spins once the opponent wins the flip, however soon they choose. */
const FLIP_MS = 1500;

/** The opening over the dimmed table, from the coin flip until both hands are kept: who plays
 * first, keep or mulligan, and the cards to put on the bottom, which fill slots and go back on
 * a click until confirmed. While the opponent decides it shows where the player stands: a hand
 * they mulliganed lies face down. */
export function Opening({
  view,
  decision: asked,
  onPick,
  onPlan,
}: {
  view: PlayerView;
  /** The decision the player may answer now; none while the game goes on without them. */
  decision?: DecisionView;
  onPick(index: CandidateIndex): void;
  onPlan(picks: Intent[]): void;
}) {
  const { you } = view.players;
  const { flipWinner } = view.turn;
  // When the opponent won the flip, the coin spins a while before it shows their choice, made
  // at once or not, as the opening first shows: before anyone has declared.
  const [spinning, setSpinning] = useState(
    () =>
      flipWinner === "opponent" &&
      you.mulligans === 0 &&
      you.mulligan === "undeclared",
  );
  useEffect(() => {
    if (!spinning) return;
    const spin = window.setTimeout(() => setSpinning(false), FLIP_MS);
    return () => window.clearTimeout(spin);
  }, [spinning]);
  const starting = spinning ? null : view.turn.starting;
  const decision = spinning ? undefined : asked;
  const choosingFirst =
    !spinning && starting === null && flipWinner === "opponent";
  const considering = view.handling?.zone === "hand";
  const picking = decision?.prompt.purpose === "put-on-bottom";
  // The cards picked for the bottom, kept as they were: those sent are gone from the hand.
  const [bottom, setBottom] = useState<ObjectView[]>([]);
  const inHand = view.objects.filter(
    (o) => o.zone === "hand" && o.owner === "you",
  );
  const hand = inHand.filter((o) => !bottom.some((b) => b.slot === o.slot));
  const unsent = bottom.filter((b) => inHand.some((o) => o.slot === b.slot));
  // One card per mulligan goes on the bottom; any already there count.
  const toBottom = picking
    ? Math.max(1, inHand.length - (STARTING_HAND - you.mulligans))
    : 0;
  const slots = Math.max(toBottom, bottom.length);
  // The cards are small to fit seven: hovering one shows it whole.
  const [inspector, inspect] = useHoverInspect();
  // A card clicked moves out from under the pointer, which never leaves it: its inspector goes.
  const toggle = (o: ObjectView) => {
    inspect(o).onPointerLeave();
    setBottom((picked) =>
      picked.some((b) => b.slot === o.slot)
        ? picked.filter((b) => b.slot !== o.slot)
        : picked.length < toBottom
          ? [...picked, o]
          : picked,
    );
  };
  const confirm = () =>
    onPlan(
      unsent.map((o) => ({
        kind: "chooseObject",
        purpose: "put-on-bottom",
        wants: (c) => c.kind === "object" && c.slot === o.slot,
        // The engine offers one of interchangeable cards (architecture §7.2): its slot fills
        // with the card that went.
        alike: (c) => {
          if (c.kind !== "object") return false;
          const other = inHand.find((h) => h.slot === c.slot);
          return other !== undefined && twins(other, o);
        },
        picked: (c) => {
          const sent =
            c.kind === "object" && inHand.find((h) => h.slot === c.slot);
          if (!sent || sent.slot === o.slot) return;
          // A twin picked too trades places with it.
          setBottom((picked) =>
            picked.map((b) =>
              b.slot === o.slot ? sent : b.slot === sent.slot ? o : b,
            ),
          );
        },
      })),
    );
  const title =
    starting === null
      ? flipWinner === "you"
        ? "You won the flip"
        : "Opponent won the flip"
      : picking
        ? "Put on the bottom"
        : you.mulligan === "willMulligan"
          ? "Mulligan"
          : you.mulligans === 0
            ? "Opening hand"
            : `Mulligan to ${STARTING_HAND - you.mulligans}`;
  const buttons =
    decision && !picking
      ? decision.candidates.map((c, i) => ({ label: c.label, index: i }))
      : [];
  const facedown = you.mulligan === "willMulligan";
  return (
    <div className="opening" role="dialog" aria-label={title}>
      <div className="overlay-backdrop" />
      <div className="opening-coin" data-starting={starting ?? undefined}>
        <Coin
          key={starting ?? "spinning"}
          spinning={starting === null && !choosingFirst}
          choosing={choosingFirst}
          first={starting === "you" || choosingFirst}
        />
        {starting && (
          <span className="opening-coin-label">
            {starting === "you" ? "On the play" : "On the draw"}
          </span>
        )}
      </div>
      <h2 className="opening-title">{title}</h2>
      {/* Fanned on a gentle arc, as the hand is held: each card's place from the middle. */}
      <div className="opening-hand">
        {starting !== null &&
          hand.map((o, i) => (
            <div
              key={o.slot}
              className="opening-fan"
              style={{ "--fan": i - (hand.length - 1) / 2 } as CSSProperties}
            >
              {facedown ? (
                <CardBack width={176} />
              ) : (
                <OpeningCard
                  object={o}
                  hover={inspect(o)}
                  onPick={
                    picking && bottom.length < toBottom
                      ? () => toggle(o)
                      : undefined
                  }
                />
              )}
            </div>
          ))}
      </div>
      {slots > 0 && (
        <div className="opening-slots" aria-label="Bottom of your library">
          {Array.from({ length: slots }, (_, i) => {
            const o = bottom[i];
            return o ? (
              <OpeningCard
                key={o.slot}
                object={o}
                hover={inspect(o)}
                width={120}
                onPick={picking ? () => toggle(o) : undefined}
              />
            ) : (
              <i key={`empty-${i}`} className="opening-slot" />
            );
          })}
        </div>
      )}
      {(picking || buttons.length > 0) && (
        <Dock>
          <DockButtons className="opening-actions">
            {picking && (
              <Button disabled={unsent.length !== toBottom} onClick={confirm}>
                Done
              </Button>
            )}
            {buttons.map((b, i) => (
              <Button
                key={b.index}
                variant={i === 0 ? "primary" : "secondary"}
                onClick={() => onPick(b.index)}
              >
                {b.label}
              </Button>
            ))}
          </DockButtons>
        </Dock>
      )}
      <div className="opening-buttons">
        {considering ? (
          <span role="status" aria-label="Opponent is considering their hand" />
        ) : (
          !decision &&
          !choosingFirst && (
            <i className="working-dots opening-waiting" role="status">
              <i />
              <i />
              <i />
            </i>
          )
        )}
      </div>
      {inspector}
    </div>
  );
}

function OpeningCard({
  object,
  width = 176,
  onPick,
  hover,
}: {
  object: ObjectView;
  width?: number;
  onPick?(): void;
  hover: HoverHandlers;
}) {
  const face = useFace(object);
  return (
    <button
      type="button"
      className="opening-card"
      disabled={!onPick}
      data-card={object.name ?? ""}
      data-slot={object.slot}
      onClick={onPick}
      {...hover}
    >
      <Card
        card={face}
        width={width}
        highlight={onPick ? "target" : undefined}
      />
    </button>
  );
}
