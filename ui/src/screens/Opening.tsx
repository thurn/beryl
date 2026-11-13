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
import { CARD_ASPECT, Card, CardBack } from "../components/Card";
import { Coin, Thinking } from "../components/Player";
import { type HoverHandlers, useHoverInspect } from "../game/Piles";
import { type Intent, twins } from "../game/decisions";
import { useFace } from "../game/faces";
import { useViewport } from "../mobile/chrome";
import { usePhone } from "../mobile/phone";
import { Inspected } from "../stage/PhoneChrome";

/** The starting hand size (CR 103.5). */
const STARTING_HAND = 7;
/** How long the coin spins once the opponent wins the flip, however soon they choose. */
const FLIP_MS = 1500;
const CARDS_PER_ROW = 4;
const CARD_GAP = 8;
const SLOT_SCALE = 0.8;

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
  const phone = usePhone();
  const { ref, viewport, frameStyle } = useViewport();
  const [inspecting, setInspecting] = useState<ObjectView["slot"]>();
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
  const facedown = you.mulligan === "willMulligan" || (phone && !decision);
  const canInspect = phone && decision?.kind === "keepOrMulligan" && !facedown;
  const inspected = canInspect
    ? hand.find((o) => o.slot === inspecting)
    : undefined;
  const coinSize = viewport && viewport.width < 393 ? 56 : 64;
  const actions = picking ? 1 : buttons.length;
  const cardWidth = viewport
    ? phoneCardWidth(
        viewport.width,
        viewport.height - viewport.insetBottom,
        coinSize,
        // Keep room for both rows while cards are moved between the hand and the slots.
        Math.ceil(
          (inHand.length + bottom.length - unsent.length) / CARDS_PER_ROW,
        ),
        slots,
        actions,
      )
    : 176;
  const slotWidth =
    viewport && slots > 0
      ? Math.min(
          SLOT_SCALE * cardWidth,
          (viewport.width - 16 - (slots - 1) * CARD_GAP) / slots,
        )
      : 120;
  return (
    <div
      ref={phone ? ref : undefined}
      className={phone ? "opening phone-opening" : "opening"}
      role="dialog"
      aria-label={title}
      style={
        phone
          ? ({
              ...frameStyle,
              "--opening-coin-size": `${coinSize}px`,
              "--opening-hand-width": `${CARDS_PER_ROW * cardWidth + (CARDS_PER_ROW - 1) * CARD_GAP}px`,
              "--opening-slot-width": `${slotWidth}px`,
              "--opening-slot-height": `${slotWidth / CARD_ASPECT}px`,
            } as CSSProperties)
          : undefined
      }
      onClickCapture={
        inspected
          ? (event) => {
              if ((event.target as Element).closest(".inspect")) return;
              event.stopPropagation();
              setInspecting(undefined);
            }
          : undefined
      }
    >
      <div className="overlay-backdrop" />
      <div className="opening-coin" data-starting={starting ?? undefined}>
        <Coin
          key={starting ?? "spinning"}
          size={phone ? coinSize : undefined}
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
      <div className="opening-cards">
        <div className="opening-hand">
          {starting !== null &&
            hand.map((o, i) => (
              <div
                key={o.slot}
                className={phone ? undefined : "opening-fan"}
                style={
                  phone
                    ? undefined
                    : ({ "--fan": i - (hand.length - 1) / 2 } as CSSProperties)
                }
              >
                {facedown ? (
                  <CardBack width={cardWidth} />
                ) : (
                  <OpeningCard
                    object={o}
                    width={cardWidth}
                    hover={phone ? undefined : inspect(o)}
                    highlight={picking && bottom.length < toBottom}
                    onPick={
                      canInspect
                        ? () => setInspecting(o.slot)
                        : picking && bottom.length < toBottom
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
                  hover={phone ? undefined : inspect(o)}
                  width={slotWidth}
                  highlight={picking}
                  onPick={picking ? () => toggle(o) : undefined}
                />
              ) : (
                <i key={`empty-${i}`} className="opening-slot" />
              );
            })}
          </div>
        )}
      </div>
      {actions > 0 && (
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
      {phone ? (
        actions === 0 && (
          <div className="phone-opening-wait">
            <Thinking />
          </div>
        )
      ) : (
        <div className="opening-buttons">
          {considering ? (
            <span
              role="status"
              aria-label="Opponent is considering their hand"
            />
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
      )}
      {phone
        ? inspected &&
          viewport && (
            <Inspected object={inspected} table={view} viewport={viewport} />
          )
        : inspector}
    </div>
  );
}

/** Fit the hand and the bottom slots between the coin and the touch controls. */
function phoneCardWidth(
  width: number,
  height: number,
  coinSize: number,
  rows: number,
  slots: number,
  actions: number,
) {
  const header = 88 + coinSize / 2 + 13 + 8 + 18 + 16;
  const footer = 16 + (actions > 0 ? actions * 48 + (actions - 1) * 8 : 40);
  const gaps = Math.max(0, rows - 1) * CARD_GAP + (slots > 0 ? 16 : 0);
  const cardRows = rows + (slots > 0 ? SLOT_SCALE : 0);
  return Math.floor(
    Math.min(
      width < 393 ? 86 : 90,
      (width - (CARDS_PER_ROW - 1) * CARD_GAP) / CARDS_PER_ROW,
      ((height - header - footer - gaps) * CARD_ASPECT) / Math.max(1, cardRows),
    ),
  );
}

function OpeningCard({
  object,
  width,
  highlight,
  onPick,
  hover,
}: {
  object: ObjectView;
  width: number;
  highlight: boolean;
  onPick?(): void;
  hover?: HoverHandlers;
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
        highlight={highlight ? "target" : undefined}
      />
    </button>
  );
}
