import {
  useId,
  useLayoutEffect,
  useRef,
  useState,
  type PointerEvent,
} from "react";
import { Button } from "../components/Controls";
import type {
  Answer,
  PileName,
  PlayerView,
  TablePrompt,
} from "../generated/protocol";
import { tableAnswer } from "../ipc";
import { ChoiceList } from "../stage/Choose";
import { DRAG_THRESHOLD } from "../stage/pointer";
import { TrayPiles } from "../stage/Tray.tsx";
import { landing, laidOut, type TraySpace } from "../stage/tray";
import type { TableClient } from "./client";
import { DragFace } from "./DragFace";
import "./prompt.css";

const pileLabels: Record<PileName, string> = {
  top: "TOP",
  bottom: "BOTTOM",
  graveyard: "Graveyard",
};

export function TablePromptView({
  client,
  prompt,
  space,
  view,
}: {
  client: TableClient;
  prompt: TablePrompt;
  space: TraySpace;
  view: PlayerView;
}) {
  if (prompt.kind === "opening") return null;
  return <Dialog key={prompt.kind} {...{ client, prompt, space, view }} />;
}

function Dialog({
  client,
  prompt,
  space,
  view,
}: {
  client: TableClient;
  prompt: Exclude<TablePrompt, { kind: "opening" }>;
  space: TraySpace;
  view: PlayerView;
}) {
  const heading = useId();
  const [sending, setSending] = useState(false);
  const [error, setError] = useState<string>();
  const pending = useRef(false);
  const root = useRef<HTMLDivElement>(null);

  function send(answer: Answer) {
    if (client.frozen || pending.current) return;
    pending.current = true;
    setSending(true);
    setError(undefined);
    void tableAnswer(answer)
      .then(
        (result) => {
          if ("refused" in result) setError("Could not apply that choice.");
        },
        () => setError("Could not reach the table."),
      )
      .finally(() => {
        pending.current = false;
        setSending(false);
      });
  }

  useLayoutEffect(() => {
    root.current?.focus();
    return client.pushOpen(() => send({ kind: "cancel" }));
  }, [client]);

  const ready =
    prompt.kind === "choose"
      ? prompt.selected != null
      : prompt.placed.flat().length === prompt.cards.length;

  return (
    <div
      className="table-library-order table-prompt-dialog"
      role="dialog"
      aria-modal="true"
      aria-labelledby={heading}
      tabIndex={-1}
      ref={root}
    >
      <h2 id={heading}>{prompt.title}</h2>
      {prompt.kind === "sort" ? (
        <SortTray {...{ prompt, view, space, sending, send }} />
      ) : (
        <ChoiceList
          items={prompt.options}
          chosen={prompt.selected}
          sending={sending}
          space={space}
          pick={(option) => send({ kind: "pick", option })}
        />
      )}
      <div className="table-order-controls table-prompt-controls">
        {error && <span role="alert">{error}</span>}
        <Button
          onClick={() => send({ kind: "done" })}
          disabled={!ready || sending}
        >
          Done
        </Button>
        <Button
          variant="secondary"
          onClick={() => send({ kind: "cancel" })}
          disabled={sending}
        >
          Cancel
        </Button>
      </div>
    </div>
  );
}

function SortTray({
  prompt,
  view,
  space,
  sending,
  send,
}: {
  prompt: Extract<TablePrompt, { kind: "sort" }>;
  view: PlayerView;
  space: TraySpace;
  sending: boolean;
  send: (answer: Answer) => void;
}) {
  const root = useRef<HTMLDivElement>(null);
  const grip = useRef<{ card: number; x: number; y: number }>(undefined);
  const [carried, setCarried] = useState<{
    card: number;
    dx: number;
    dy: number;
  }>();
  const missing = prompt.cards.filter(
    (slot) => !prompt.placed.some((row) => row.includes(slot)),
  );
  // Cards without an assignment stay visible at the end of the Top row.
  const tray = {
    piles: prompt.placed.map((cards, at) => ({
      cards: prompt.piles[at] === "top" ? [...cards, ...missing] : cards,
    })),
  };
  const laid = laidOut(tray, space);
  const displacement = (event: PointerEvent) => {
    const element = root.current!;
    const scale =
      element.getBoundingClientRect().width / element.offsetWidth || 1;
    return {
      dx: (event.clientX - grip.current!.x) / scale,
      dy: (event.clientY - grip.current!.y) / scale,
    };
  };
  const placeAt = (card: number, row: number, index: number) => {
    send({ kind: "place", card, pile: prompt.piles[row]!, index });
  };
  const clickCard = (card: number, row: number) => {
    if (row < 0) {
      const top = prompt.piles.indexOf("top");
      placeAt(card, top, prompt.placed[top]!.length);
    } else if (prompt.piles.length === 1) {
      placeAt(card, row, 0);
    } else {
      const other = 1 - row;
      placeAt(card, other, prompt.placed[other]!.length);
    }
  };
  const cancelDrag = () => {
    grip.current = undefined;
    setCarried(undefined);
  };

  return (
    <div ref={root} className="table-sort-tray">
      <TrayPiles
        laid={laid}
        labels={prompt.piles.map((pile) =>
          pile === "top" && prompt.piles.includes("graveyard")
            ? "Library"
            : pileLabels[pile],
        )}
        heading={space.heading}
      />
      {[...laid.cards].map(([card, place], depth) => {
        const object = view.objects.find((item) => item.slot === card);
        if (!object) return null;
        const row = prompt.placed.findIndex((cards) => cards.includes(card));
        const held = carried?.card === card ? carried : undefined;
        return (
          <button
            type="button"
            key={card}
            className="table-order-card"
            data-sort-card={card}
            data-sort-pile={prompt.piles[row]}
            aria-label={object.name ?? "Card back"}
            aria-disabled={sending}
            style={{
              left: place.x + (held?.dx ?? 0),
              top: place.y + (held?.dy ?? 0),
              zIndex: held ? prompt.cards.length + 1 : depth + 1,
            }}
            onClick={(event) => {
              // Pointer clicks are handled by pointer-up. Enter and Space also click.
              if (event.detail === 0 && !sending) clickCard(card, row);
            }}
            onPointerDown={(event) => {
              if (event.button !== 0 || sending) return;
              event.currentTarget.setPointerCapture(event.pointerId);
              grip.current = { card, x: event.clientX, y: event.clientY };
            }}
            onPointerMove={(event) => {
              if (grip.current?.card === card)
                setCarried({ card, ...displacement(event) });
            }}
            onPointerCancel={cancelDrag}
            onPointerUp={(event) => {
              if (grip.current?.card !== card) return;
              const { dx, dy } = displacement(event);
              cancelDrag();
              if (Math.hypot(dx, dy) < DRAG_THRESHOLD) {
                clickCard(card, row);
                return;
              }
              const point = {
                x: place.x + dx + place.width / 2,
                y: place.y + dy + place.width / space.card.aspect / 2,
              };
              if (prompt.piles.length === 1) {
                const nearest = [...laid.cards].reduce(
                  (best, [slot, pose]) => {
                    const distance = Math.hypot(
                      point.x - pose.x - pose.width / 2,
                      point.y - pose.y - pose.width / space.card.aspect / 2,
                    );
                    return distance < best.distance ? { slot, distance } : best;
                  },
                  { slot: card, distance: Infinity },
                );
                const index = prompt.placed[0]!.indexOf(nearest.slot);
                if (index >= 0) placeAt(card, 0, index);
                return;
              }
              const target = landing(tray, space, point);
              if (!target) return;
              const from = prompt.placed[target.pile]!.indexOf(card);
              const length =
                prompt.placed[target.pile]!.length - Number(from >= 0);
              placeAt(
                card,
                target.pile,
                Math.min(
                  length,
                  target.at - Number(from >= 0 && from < target.at),
                ),
              );
            }}
          >
            <DragFace object={object} width={place.width} />
          </button>
        );
      })}
    </div>
  );
}
