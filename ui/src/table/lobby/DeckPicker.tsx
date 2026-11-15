import { useEffect, useRef, useState } from "react";
import { Button } from "../../components/Controls";
import {
  CommandError,
  deckImport,
  deckList,
  deckOpenFile,
  type DeckImportReply,
  type DeckImportRequest,
  type SavedDeck,
} from "../../ipc";

export type DeckPanel = "change" | "paste" | "file";

export function DeckPicker({
  panel,
  setPanel,
  phone,
  useDeck,
}: {
  panel: DeckPanel;
  setPanel(panel?: DeckPanel): void;
  phone: boolean;
  useDeck(deck: DeckImportReply): Promise<void>;
}) {
  const [saved, setSaved] = useState<SavedDeck[]>([]);
  const [name, setName] = useState("");
  const [text, setText] = useState("");
  const [file, setFile] = useState<Pick<DeckImportRequest, "text" | "cod">>();
  const opening = useRef<ReturnType<typeof deckOpenFile>>(undefined);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<string>();

  const report = (failure: unknown) => {
    const detail = failure instanceof CommandError ? failure.error : undefined;
    setError(
      detail && typeof detail === "object" && "reason" in detail
        ? String(detail.reason)
        : "Could not load the deck.",
    );
  };

  useEffect(() => {
    let closed = false;
    if (panel === "change") {
      void deckList().then(
        (list) => {
          if (!closed) setSaved(list);
        },
        (failure: unknown) => {
          if (!closed) report(failure);
        },
      );
    } else if (panel === "file") {
      // The native dialog should open once, including when React restarts effects in development.
      opening.current ??= deckOpenFile();
      void opening.current.then(
        (reply) => {
          if (closed) return;
          if (reply.kind === "text" || reply.kind === "cod") {
            setFile(
              reply.kind === "text"
                ? { text: reply.text }
                : { cod: reply.text },
            );
          } else {
            setPanel("change");
          }
        },
        (failure: unknown) => {
          if (!closed) report(failure);
        },
      );
    }
    return () => {
      closed = true;
    };
  }, [panel, setPanel]);

  const load = async (read: () => Promise<DeckImportReply>) => {
    setPending(true);
    setError(undefined);
    try {
      await useDeck(await read());
      setPanel();
    } catch (failure) {
      report(failure);
      setPending(false);
    }
  };

  return (
    <section className="lobby-section" aria-label="Choose a deck">
      {panel === "change" ? (
        <>
          {saved.length > 0 && (
            <ul className="lobby-decks">
              {saved.map(({ deck, mainCount, sideCount }) => (
                <li key={deck.name}>
                  <Button
                    variant="secondary"
                    disabled={pending}
                    onClick={() => void load(async () => deck)}
                  >
                    {deck.name}
                  </Button>
                  <span>
                    {mainCount} main · {sideCount} sideboard
                  </span>
                </li>
              ))}
            </ul>
          )}
          <div className="lobby-actions">
            <Button variant="secondary" onClick={() => setPanel("paste")}>
              Paste decklist
            </Button>
            {!phone && (
              <Button variant="secondary" onClick={() => setPanel("file")}>
                Open file
              </Button>
            )}
          </div>
        </>
      ) : (
        (panel === "paste" || file) && (
          <form
            className="lobby-import"
            onSubmit={(event) => {
              event.preventDefault();
              const source = panel === "paste" ? { text } : file;
              if (
                !source ||
                pending ||
                !name.trim() ||
                (panel === "paste" && !text.trim())
              )
                return;
              void load(() => deckImport({ name: name.trim(), ...source }));
            }}
          >
            <label>
              Deck name
              <input
                required
                value={name}
                onChange={(event) => setName(event.target.value)}
              />
            </label>
            {panel === "paste" && (
              <label>
                Decklist
                <textarea
                  required
                  rows={6}
                  value={text}
                  onChange={(event) => setText(event.target.value)}
                />
              </label>
            )}
            <Button type="submit" disabled={pending}>
              Import
            </Button>
          </form>
        )
      )}
      {error && <p role="alert">{error}</p>}
      <Button variant="secondary" disabled={pending} onClick={() => setPanel()}>
        Close
      </Button>
    </section>
  );
}
