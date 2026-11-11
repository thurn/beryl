// The game screen on a portrait phone (DESIGN.md of the approved prototype): the cards placed
// by mobile/layout.ts as full scans, only live state drawn over them in the desktop's badges,
// the desktop's life orbs, piles in each player's strip, the phase icon and the stack in the
// middle, and the dock answering the same decisions the desktop's does. No names, captions,
// counts or turn number: a card's scan says what it is, and a touch will say the rest.
import { useMemo, useState, type CSSProperties } from "react";
import type {
  CandidateIndex,
  ObjectView,
  PlayerView,
} from "../generated/protocol";
import { CardBack, type Highlight } from "../components/Card";
import { interactions, tapsOnly, type Intent } from "../game/decisions";
import { PaymentButton } from "../game/PaymentButton";
import { useWaterbend } from "../game/waterbend";
import {
  MenuButton,
  Middle,
  Orb,
  PhoneDock,
  Piles,
  useViewport,
} from "./chrome";
import { AbilityScan, badgeEm, CardScan, SpellScan, type Glow } from "./faces";
import { phoneLayout, type Spot, type StackSpot } from "./layout";
import "./phone.css";

export function PhoneBoard({
  view,
  waiting,
  choose,
  passOn,
  cancel,
  onLeave,
}: {
  view: PlayerView;
  /** An answer is on its way, or the game plays on without the player. */
  waiting: boolean;
  choose(index: CandidateIndex, then?: Intent[]): void;
  passOn(): void;
  cancel(): void;
  onLeave(): void;
}) {
  const { ref, viewport } = useViewport();
  const status = view.status;
  const decision =
    status.kind === "deciding" && !waiting ? status.decision : undefined;
  const waterbend = useWaterbend(decision, choose);
  const [paymentPreview, setPaymentPreview] = useState<PlayerView>();
  const previewingPayment = paymentPreview === view;
  const shown = useMemo(
    () =>
      waterbend.manual.length === 0
        ? view
        : {
            ...view,
            objects: view.objects.map((o) =>
              waterbend.manual.includes(o.slot) ? { ...o, tapped: true } : o,
            ),
          },
    [view, waterbend.manual],
  );
  const placed = useMemo(
    () => viewport && phoneLayout(shown, viewport),
    [shown, viewport],
  );
  const objects = useMemo(
    () => new Map(shown.objects.map((o) => [o.slot, o])),
    [shown],
  );
  const can = useMemo(() => interactions(view), [view]);
  const glows = new Map<number, Glow>();
  if (decision && !waterbend.active) {
    for (const [slot, acts] of can.bySlot) {
      const single = can.picking && acts.length === 1;
      glows.set(slot, {
        highlight: single ? "target" : undefined,
        playable: !single && !tapsOnly(acts),
      });
    }
  }
  // The top of the stack glows gold, and what it names cyan.
  const top = view.stack[view.stack.length - 1];
  const marks: [number | null | undefined, Highlight][] = [
    [top?.kind === "spell" ? top.slot : top?.source, "source"],
    [can.source, "source"],
    ...(top?.targets ?? []).flatMap((t): [number, Highlight][] =>
      t.kind === "object" ? [[t.slot, "target"]] : [],
    ),
  ];
  for (const [slot, highlight] of marks) {
    if (waterbend.active || slot == null || glows.get(slot)?.highlight)
      continue;
    glows.set(slot, { ...glows.get(slot), highlight });
  }

  const paymentSlots = previewingPayment
    ? [...waterbend.manual, ...waterbend.proposed]
    : waterbend.manual;
  for (const slot of paymentSlots) glows.set(slot, { paying: true });

  const theirHand = view.objects.filter(
    (o) => o.zone === "hand" && o.owner === "opponent",
  );
  return (
    <div className="phone-board" ref={ref}>
      {placed && (
        <>
          <div
            className="phone-strip phone-strip-opponent"
            style={placed.bands.opponentStrip}
          >
            <Piles side="opponent" view={view} tops />
            <Orb side="opponent" view={view} />
            <div className="phone-backs">
              {theirHand.map((o) => (
                <CardBack key={o.slot} width={24} />
              ))}
            </div>
            <MenuButton onLeave={onLeave} />
          </div>
          <Middle band={placed.bands.middle} view={view} />
          <div
            className="phone-strip phone-strip-you"
            style={placed.bands.yourStrip}
          >
            <Piles side="you" view={view} tops />
            {waterbend.active && decision?.cancellable && (
              <button
                type="button"
                className="icon-button phone-payment-cancel"
                aria-label="Cancel"
                onClick={cancel}
              >
                ×
              </button>
            )}
            <Orb side="you" view={view} />
            {waterbend.active ? (
              <div className="phone-dock">
                <PaymentButton
                  disabled={!waterbend.ready}
                  onPreview={(on) => setPaymentPreview(on ? view : undefined)}
                  onPay={waterbend.autoPay}
                >
                  Auto Pay
                </PaymentButton>
              </div>
            ) : (
              <PhoneDock
                view={view}
                waiting={waiting}
                can={can}
                objects={objects}
                choose={choose}
                passOn={passOn}
              />
            )}
          </div>
          <div className="card-layer phone-cards">
            {placed.cards.map((spot) => (
              <PhoneCard
                key={spot.slot}
                object={objects.get(spot.slot)!}
                spot={spot}
                glow={glows.get(spot.slot) ?? {}}
                onClick={
                  waterbend.active && spot.zone === "battlefield"
                    ? () => waterbend.toggle(spot.slot)
                    : undefined
                }
              />
            ))}
            {placed.stack.map((spot) => (
              <StackCard
                key={stackKey(spot)}
                spot={spot}
                object={
                  spot.item.kind === "spell"
                    ? objects.get(spot.item.slot)
                    : undefined
                }
                top={spot.index === view.stack.length - 1}
              />
            ))}
          </div>
        </>
      )}
    </div>
  );
}

/** A card on the table or in your hand, by its slot. */
function PhoneCard({
  object,
  spot,
  glow,
  onClick,
}: {
  object: ObjectView;
  spot: Spot;
  glow: Glow;
  onClick?(): void;
}) {
  return (
    <div
      className={`card-el phone-card card-el-${object.controller}`}
      data-slot={object.slot}
      data-zone={object.zone}
      data-card={object.name ?? undefined}
      onClick={onClick}
      style={{
        left: spot.x,
        top: spot.y,
        zIndex: spot.z,
        transform: spot.rotate ? `rotate(${spot.rotate}deg)` : undefined,
      }}
    >
      <CardScan
        object={object}
        width={spot.width}
        field={spot.zone === "battlefield"}
        glow={glow}
      />
    </div>
  );
}

/** A spell or ability on the stack, the top one in the source's gold. */
function StackCard({
  spot,
  object,
  top,
}: {
  spot: StackSpot;
  object?: ObjectView;
  top: boolean;
}) {
  const style = {
    left: spot.x,
    top: spot.y,
    zIndex: spot.z,
    "--badge": badgeEm(spot.width),
  } as CSSProperties;
  return (
    <div
      className="card-el phone-card"
      data-slot={object?.slot}
      data-zone="stack"
      data-card={object?.name ?? undefined}
      style={style}
    >
      {object ? (
        <SpellScan object={object} width={spot.width} top={top} />
      ) : (
        <AbilityScan
          name={spot.item.kind === "ability" ? spot.item.name : ""}
          width={spot.width}
          top={top}
        />
      )}
    </div>
  );
}

/** The key of a stack item's element: a spell's slot, or an ability's place. */
const stackKey = (spot: StackSpot) =>
  spot.item.kind === "spell"
    ? `spell-${spot.item.slot}`
    : `ability-${spot.index}`;
