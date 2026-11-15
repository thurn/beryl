import { useEffect, useState } from "react";
import { Button } from "../../components/Controls";
import {
  appInfo,
  CommandError,
  isNativeApp,
  tableHost,
  tableJoin,
  tableLobby,
  tableResume,
  tableSaved,
  type SavedTable,
} from "../../ipc";
import { Bindings } from "../bindings";
import { resolve } from "../shortcuts";
import "./lobby.css";

export function TableEntries({
  phone,
  disabled,
  pending,
  setPending,
}: {
  phone: boolean;
  disabled: boolean;
  pending: boolean;
  setPending(pending: boolean): void;
}) {
  const [mode, setMode] = useState<"host" | "join">();
  const [name, setName] = useState("");
  const [bindings, setBindings] = useState<Bindings>();
  const [invite, setInvite] = useState("");
  const [local, setLocal] = useState(!isNativeApp());
  const [error, setError] = useState<string>();
  const [saved, setSaved] = useState<SavedTable[]>([]);
  const [savedError, setSavedError] = useState(false);
  useEffect(() => {
    if (phone) return;
    let mounted = true;
    void tableSaved().then(
      (tables) => {
        if (mounted) setSaved(tables);
      },
      () => {
        if (mounted) setSavedError(true);
      },
    );
    return () => {
      mounted = false;
    };
  }, [phone]);
  const blocked = disabled || pending;
  useEffect(() => {
    if (phone) return;
    let active = true;
    void appInfo()
      .then(({ platform }) => {
        if (active) setBindings(new Bindings(platform));
      })
      .catch(() => {});
    return () => {
      active = false;
    };
  }, [phone]);
  useEffect(() => {
    if (phone || blocked || !bindings) return;
    const key = (event: KeyboardEvent) => {
      if (
        event.defaultPrevented ||
        event.repeat ||
        resolve(event, "lobby", bindings) !== "lobby.host"
      )
        return;
      event.preventDefault();
      setMode("host");
    };
    window.addEventListener("keydown", key);
    return () => window.removeEventListener("keydown", key);
  }, [bindings, blocked, phone]);
  const open = async (operation: () => Promise<unknown>) => {
    setError(undefined);
    setPending(true);
    try {
      await operation();
      window.location.hash = "#/lobby";
    } catch (failure) {
      const detail =
        failure instanceof CommandError ? failure.error : undefined;
      setError(
        detail && typeof detail === "object" && "reason" in detail
          ? String(detail.reason)
          : "Could not open the table.",
      );
      setPending(false);
    }
  };
  return (
    <div className="lobby-entry">
      <nav className="lobby-actions" aria-label="Manual tables">
        {!phone && (
          <Button
            variant="secondary"
            disabled={blocked}
            onClick={() => setMode("host")}
          >
            Host a table
          </Button>
        )}
        <Button
          variant="secondary"
          disabled={blocked}
          onClick={() => setMode("join")}
        >
          Join a table
        </Button>
        <Button
          variant="secondary"
          disabled={blocked}
          onClick={() => void open(() => tableLobby({ kind: "solo" }))}
        >
          Solo table
        </Button>
      </nav>
      {mode && (
        <form
          className="lobby-actions"
          onSubmit={(event) => {
            event.preventDefault();
            if (blocked || !name.trim() || (mode === "join" && !invite.trim()))
              return;
            void open(() =>
              mode === "host"
                ? tableHost(local ? "local" : "iroh", { name: name.trim() })
                : tableJoin(invite.trim(), name.trim()),
            );
          }}
        >
          <label>
            Name
            <input
              required
              value={name}
              onChange={(event) => setName(event.target.value)}
            />
          </label>
          {mode === "join" && (
            <label>
              Invite code
              <input
                required
                autoCapitalize="none"
                spellCheck={false}
                value={invite}
                onChange={(event) => setInvite(event.target.value)}
              />
            </label>
          )}
          {mode === "host" && isNativeApp() && (
            <label>
              Connection
              <select
                value={local ? "local" : "internet"}
                onChange={(event) => setLocal(event.target.value === "local")}
              >
                <option value="internet">Internet</option>
                <option value="local">Local</option>
              </select>
            </label>
          )}
          <Button type="submit" disabled={blocked}>
            {mode === "host" ? "Host" : "Join"}
          </Button>
          <Button
            variant="secondary"
            disabled={blocked}
            onClick={() => {
              setMode(undefined);
              setError(undefined);
            }}
          >
            Cancel
          </Button>
        </form>
      )}
      {!phone && saved.length > 0 && (
        <ul className="lobby-saved" aria-label="Saved tables">
          {saved.map(({ id, host, guest }) => (
            <li key={id}>
              <span>{guest ? `${host} · ${guest}` : host}</span>
              <Button
                variant="secondary"
                disabled={blocked}
                onClick={() => void open(() => tableResume(id))}
              >
                Resume table
              </Button>
            </li>
          ))}
        </ul>
      )}
      {!phone && savedError && (
        <p role="status">Could not load saved tables.</p>
      )}
      {error && (
        <p className="game-error" role="alert">
          {error}
        </p>
      )}
    </div>
  );
}
