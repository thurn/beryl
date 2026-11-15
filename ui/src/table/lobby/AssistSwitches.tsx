import type { LobbyChange, TableAssist } from "../../ipc";

const names: Record<TableAssist, string> = {
  turn_upkeep: "Turn upkeep",
  combat: "Combat",
  state_nudges: "State nudges",
  library_flows: "Library flows",
  ability_menus: "Ability menus",
};

export function AssistSwitches({
  assists,
  disabled,
  change,
}: {
  assists: TableAssist[];
  disabled: boolean;
  change(change: LobbyChange): void;
}) {
  return (
    <fieldset className="lobby-section lobby-assists" disabled={disabled}>
      <legend>Assists</legend>
      {(Object.keys(names) as TableAssist[]).map((assist) => (
        <label key={assist}>
          <input
            role="switch"
            type="checkbox"
            checked={assists.includes(assist)}
            onChange={(event) =>
              change({ kind: "set_assist", assist, on: event.target.checked })
            }
          />
          {names[assist]}
        </label>
      ))}
    </fieldset>
  );
}
