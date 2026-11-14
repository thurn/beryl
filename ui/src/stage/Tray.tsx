// What the screen draws of a prompt besides its bar (PromptBar.tsx) and the stage's own nodes
// and marks: the furniture under the cards a prompt holds up (prompt.ts, tray.ts), and the
// row of an order's waiting triggers. Both are the session's state as sent: which pile is
// which and what it is called, and the triggers in the order they will resolve, between the
// words the session has for the order's two ends. Nothing here says what may be moved or
// where: a drop goes to the session, whose reply is drawn.
//
// The furniture lies in the stage's own layer, over everything at rest and under the cards
// held up: a shade over the table, and for a split a box a pile under its name, or for cards
// put back from the hand the library's backs the cards are put on. Its places are the
// layout's (tray.ts), the same the stage rests the cards at.
import { useRef, useState, type PointerEvent } from "react";
import { createPortal } from "react-dom";
import { CardBack } from "../components/Card";
import { StackItem } from "../components/Stack";
import type {
  DecisionView,
  ObjectView,
  PlayerView,
} from "../generated/protocol";
import { abilityText, BLANK, useAbilityFace } from "../game/faces";
import { DRAG_THRESHOLD } from "./pointer.ts";
import { trayOf, waysOf, type Asked, type Prompts } from "./prompt.ts";
import { laidOut, type Box, type TraySpace } from "./tray.ts";

/** The shade and the piles' boxes under the cards the open prompt holds up, or under what it
 * lists over the table (triggers in their order, the ways to use a card, a catalog), drawn in the stage's `layer` in the `space` its layout gives
 * a tray. Nothing while the open prompt holds nothing up. */
export function TrayUnder({
  asked: { prompt },
  space,
  layer,
}: {
  asked: Asked;
  space: TraySpace;
  layer: HTMLElement | null;
}) {
  const tray = trayOf(prompt);
  const state = prompt?.body;
  // An order's row lies over the same shade (`OrderRow`), as do the ways to use a card and
  // a catalog's entries (Choose.tsx).
  const listed =
    state?.shape === "order" ||
    state?.shape === "catalog" ||
    waysOf(prompt).length > 0;
  if (!(tray || listed) || !layer) return null;
  const piles = state?.shape === "split" ? state.piles : [];
  const laid = tray && laidOut(tray, space);
  const library = laid?.library;
  const boxes = laid?.boxes ?? [];
  return createPortal(
    <div className="stage-tray" data-purpose={prompt?.purpose}>
      {library && <Library box={library} />}
      {boxes.map((box, at) => (
        <div
          key={at}
          // A put-back's piles are the library and the hand as they lie, with no edge drawn
          // round them: Arena's player picks the cards out of the hand itself.
          className="stage-tray-pile"
          data-pile={at}
          style={{
            left: box.x,
            top: box.y,
            width: box.width,
            height: box.height,
            ["--heading" as string]: `${space.heading}px`,
            ...(library && { border: "none", background: "none" }),
          }}
        >
          <span className="stage-tray-label">{piles[at]?.label}</span>
        </div>
      ))}
    </div>,
    layer,
  );
}

/** How many backs the library is drawn with, and how far each lies up and left of the one
 * over it, so that its edge shows as a deck's does. */
const DECK = { backs: 4, dx: -1.5, dy: -2.5 };

/** The library a put-back's cards go onto, face down in `box`. */
function Library({ box }: { box: Box }) {
  return (
    <div
      className="stage-tray-library"
      style={{
        position: "absolute",
        left: box.x,
        top: box.y,
        width: box.width,
        height: box.height,
      }}
    >
      {Array.from({ length: DECK.backs }, (_, i) => {
        const depth = DECK.backs - 1 - i;
        return (
          <CardBack
            key={i}
            width={box.width}
            style={{
              position: "absolute",
              left: DECK.dx * depth,
              top: DECK.dy * depth,
            }}
          />
        );
      })}
    </div>
  );
}

type Candidate = DecisionView["candidates"][number];

/** A waiting trigger as the stack will show it: its source's frame, wearing the words of the
 * ability. One with no source the table has (an emblem's) is named by what the session
 * calls it. */
function Waiting({
  candidate,
  source,
  width,
}: {
  candidate: Candidate;
  source: ObjectView | undefined;
  width: number;
}) {
  const face = useAbilityFace(source?.name ?? candidate.label, source, BLANK);
  return (
    <StackItem
      source={face}
      label="Triggered Ability"
      text={
        candidate.text ?? abilityText(face.text ?? "", true, candidate.nth ?? 0)
      }
      width={width}
      showArt
    />
  );
}

/** The waiting triggers of the open prompt, where it is an order, in a row over the table as
 * Arena's browser has them (`UrGalw_f35w 56:30.5`): the first to resolve at the right, the
 * last at the left, each end under the session's word for it. One is moved by carrying it to
 * its place, and a click on one puts it first. The row lies in `space`, where a layout holds
 * a prompt's cards up. Nothing for any other prompt. */
export function OrderRow({
  asked: { prompt, decision, sending },
  prompts,
  view,
  space,
}: {
  asked: Asked;
  prompts: Prompts;
  view: PlayerView;
  space: TraySpace;
}) {
  const row = useRef<HTMLOListElement>(null);
  const [carried, setCarried] = useState<{
    item: number;
    from: number;
    by: number;
  }>();
  const state = prompt?.body;
  if (state?.shape !== "order" || !prompt || !decision) return null;
  const items = state.items.flatMap((item) => {
    const id = item.kind === "ability" ? item.id : undefined;
    const candidate = id === undefined ? undefined : decision.candidates[id];
    return candidate ? [{ id: id!, on: item, candidate }] : [];
  });
  const { gap, card } = space;
  const width = Math.max(
    card.least,
    Math.min(card.most, (space.width - gap) / items.length - gap),
  );
  /** Before which item something let go at the window's `x` lies, each where it lay before
   * `moved` was carried `by`: the first whose middle it is past, going from the first at the
   * right to the last at the left; the row's length for the end. */
  const before = (x: number, moved: number, by: number) => {
    const cells = [...(row.current?.children ?? [])];
    return cells.filter((cell, at) => {
      const box = cell.getBoundingClientRect();
      const left = items[at]?.id === moved ? box.left - by : box.left;
      return left + box.width / 2 > x;
    }).length;
  };
  const down = (e: PointerEvent<HTMLElement>, item: number) => {
    if (sending) return;
    e.currentTarget.setPointerCapture(e.pointerId);
    setCarried({ item, from: e.clientX, by: 0 });
  };
  const move = (e: PointerEvent) => {
    if (carried) setCarried({ ...carried, by: e.clientX - carried.from });
  };
  const up = (e: PointerEvent) => {
    if (!carried) return;
    setCarried(undefined);
    const on = items.find(({ id }) => id === carried.item)?.on;
    if (!on) return;
    const { item, by } = carried;
    const held = Math.abs(by) >= DRAG_THRESHOLD;
    prompts.drop(on, 0, held ? before(e.clientX, item, by) : 0);
  };
  const [last, first] = prompt.words.piles;
  return (
    <div
      className="stage-order"
      style={{
        left: space.x,
        top: space.y,
        width: space.width,
        height: space.height,
      }}
    >
      <ol className="stage-order-row" ref={row} style={{ gap }}>
        {items.map(({ id, candidate }) => {
          const { option } = candidate;
          const slot = option.kind === "trigger" ? option.source : null;
          const lifted = carried?.item === id;
          return (
            <li
              key={id}
              className={
                lifted ? "stage-order-item lifted" : "stage-order-item"
              }
              data-item={id}
              style={lifted ? { translate: `${carried.by}px 0` } : undefined}
              onPointerDown={(e) => down(e, id)}
              onPointerMove={move}
              onPointerUp={up}
              onPointerCancel={() => setCarried(undefined)}
            >
              <Waiting
                candidate={candidate}
                source={view.objects.find((o) => o.slot === slot)}
                width={width}
              />
            </li>
          );
        })}
      </ol>
      <div className="stage-order-ends">
        <span>{last}</span>
        <span>{first}</span>
      </div>
    </div>
  );
}
