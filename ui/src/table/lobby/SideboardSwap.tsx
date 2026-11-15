import { Button } from "../../components/Controls";
import type { LobbyChange, LobbyView } from "../../ipc";

export function SideboardSwap({
  view,
  pending,
  change,
  close,
}: {
  view: LobbyView;
  pending: boolean;
  change(change: LobbyChange): void;
  close(): void;
}) {
  return (
    <section className="lobby-section" aria-label="Sideboard swap">
      <div className="lobby-swap">
        {(["main", "side"] as const).map((zone) => (
          <div key={zone}>
            <h2>{zone === "main" ? "Main deck" : "Sideboard"}</h2>
            <ul className="lobby-decks">
              {view[zone].map(([name, count]) => (
                <li key={name}>
                  <span>
                    {count} × {name}
                  </span>
                  <Button
                    variant="secondary"
                    size="small"
                    disabled={pending}
                    aria-label={`Move ${name} to ${zone === "main" ? "sideboard" : "main deck"}`}
                    onClick={() =>
                      change({
                        kind: "sideboard",
                        intoMain: zone === "side" ? [[name, 1]] : [],
                        intoSide: zone === "main" ? [[name, 1]] : [],
                      })
                    }
                  >
                    {zone === "main" ? "To sideboard" : "To main"}
                  </Button>
                </li>
              ))}
            </ul>
          </div>
        ))}
      </div>
      <Button variant="secondary" onClick={close}>
        Done
      </Button>
    </section>
  );
}
