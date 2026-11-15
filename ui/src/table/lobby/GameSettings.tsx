import { useState } from "react";
import { Button } from "../../components/Controls";
import type { LobbyChange, LobbySettings, LobbyView } from "../../ipc";

const defaultFirst = (view: LobbyView) =>
  view.lastWinner != null ? "Loser chooses" : "Roll";

export function GameSettingsSummary({ view }: { view: LobbyView }) {
  const { startingLife, startingHandSize, first } = view.settings;
  return (
    <fieldset className="lobby-section">
      <legend>Game</legend>
      <dl className="lobby-settings-summary">
        <div>
          <dt>Starting life</dt>
          <dd>{startingLife}</dd>
        </div>
        <div>
          <dt>Starting hand size</dt>
          <dd>{startingHandSize}</dd>
        </div>
        <div>
          <dt>First</dt>
          <dd>
            {first == null
              ? defaultFirst(view)
              : (view.seats.find(({ seat }) => seat === first)?.name ??
                "Guest")}
          </dd>
        </div>
      </dl>
    </fieldset>
  );
}

export function GameSettings({
  view,
  pending,
  change,
}: {
  view: LobbyView;
  pending: boolean;
  change(change: LobbyChange): Promise<void>;
}) {
  const { startingLife, startingHandSize, first } = view.settings;
  const [draft, setDraft] = useState<LobbySettings>(view.settings);
  const dirty =
    draft.startingLife !== startingLife ||
    draft.startingHandSize !== startingHandSize ||
    draft.first !== first;
  return (
    <form
      onSubmit={(event) => {
        event.preventDefault();
        if (dirty)
          void change({ kind: "settings", settings: draft }).catch(() => {});
      }}
    >
      <fieldset className="lobby-section lobby-actions" disabled={pending}>
        <legend>Game</legend>
        <label>
          Starting life
          <input
            type="number"
            required
            min={-(2 ** 31)}
            max={2 ** 31 - 1}
            value={Number.isNaN(draft.startingLife) ? "" : draft.startingLife}
            onChange={(event) =>
              setDraft({ ...draft, startingLife: event.target.valueAsNumber })
            }
          />
        </label>
        <label>
          Starting hand size
          <input
            type="number"
            required
            min={0}
            max={65535}
            value={
              Number.isNaN(draft.startingHandSize) ? "" : draft.startingHandSize
            }
            onChange={(event) =>
              setDraft({
                ...draft,
                startingHandSize: event.target.valueAsNumber,
              })
            }
          />
        </label>
        <label>
          First
          <select
            value={draft.first ?? ""}
            onChange={(event) => {
              const { value } = event.target;
              setDraft({
                ...draft,
                first: value === "" ? null : Number(value),
              });
            }}
          >
            <option value="">{defaultFirst(view)}</option>
            {view.seats.map(({ seat, name }) => (
              <option key={seat} value={seat}>
                {name ?? "Guest"}
              </option>
            ))}
          </select>
        </label>
        <Button type="submit" variant="secondary" disabled={!dirty}>
          Save settings
        </Button>
      </fieldset>
    </form>
  );
}
