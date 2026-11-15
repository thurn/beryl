import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { Button } from "../components/Controls";
import type { CardSearchHit, CardTokenLink } from "../generated/protocol";
import { cardSearch } from "../ipc";
import { parse } from "./ask";
import { CommandList, type CommandListEntry } from "./CommandList";
import { MAX_INT, MIN_INT, storeToken, type TokenChoice } from "./cardState";
import "./tokenSearch.css";

const CUSTOM = "custom";
const colors = [
  ["W", "White"],
  ["U", "Blue"],
  ["B", "Black"],
  ["R", "Red"],
  ["G", "Green"],
];

interface TokenSearchProps {
  related?: readonly CardTokenLink[];
  phone: boolean;
  onChoose: (token: TokenChoice, shift: boolean) => void;
  onClose: () => void;
}

export function TokenSearch({
  related = [],
  phone,
  onChoose,
  onClose,
}: TokenSearchProps) {
  const [query, setQuery] = useState("");
  const [custom, setCustom] = useState<{ shift: boolean }>();
  const [result, setResult] = useState<{
    query: string;
    hits?: CardSearchHit[];
  }>();
  const text = query.trim();
  useEffect(() => {
    if (!text) return;
    let current = true;
    void cardSearch({ text, tokens: true, limit: 36 }).then(
      (hits) => {
        if (current) setResult({ query: text, hits });
      },
      () => {
        if (current) setResult({ query: text });
      },
    );
    return () => {
      current = false;
    };
  }, [text]);

  if (custom)
    return (
      <CustomToken
        onClose={onClose}
        onChoose={(token, shift) => onChoose(token, custom.shift || shift)}
      />
    );

  const hits = result?.query === text ? (result.hits ?? []) : [];
  const matches = new Map<string, CardTokenLink>();
  const rows: CommandListEntry[] = [];
  const add = (token: CardTokenLink, group: string, detail?: string) => {
    if (matches.has(token.scryfallId)) return;
    matches.set(token.scryfallId, token);
    rows.push({ id: token.scryfallId, label: token.name, group, detail });
  };
  const hitById = new Map(hits.map((hit) => [hit.scryfallId, hit]));
  const detail = (hit?: CardSearchHit) => {
    if (!hit) return undefined;
    const stats =
      hit.power !== undefined && hit.toughness !== undefined
        ? ` · ${hit.power}/${hit.toughness}`
        : "";
    return hit.typeLine + stats;
  };
  for (const token of related) {
    if (token.name.toLowerCase().includes(text.toLowerCase()))
      add(token, "Related tokens", detail(hitById.get(token.scryfallId)));
  }
  for (const hit of hits) add(hit, "Store tokens", detail(hit));
  const notice =
    text && result?.query === text && rows.length === 0
      ? result.hits
        ? "No matching tokens"
        : "Token search unavailable"
      : undefined;
  rows.push({ id: CUSTOM, label: "Custom token", group: "" });

  return (
    <CommandList
      presentation={phone ? "fullscreen" : "panel"}
      search={{
        label: "Search tokens",
        query,
        onQuery: setQuery,
        busy: !!text && result?.query !== text,
      }}
      commands={rows}
      notice={notice}
      onClose={onClose}
      onRun={(id, shift) => {
        if (id === CUSTOM) setCustom({ shift });
        else {
          const token = matches.get(id);
          if (token) onChoose(storeToken(token), shift);
        }
      }}
    />
  );
}

function CustomToken({
  onChoose,
  onClose,
}: Omit<TokenSearchProps, "phone" | "related">) {
  const dialog = useRef<HTMLDialogElement>(null);
  const submitWithShift = useRef(false);
  const [name, setName] = useState("");
  const [power, setPower] = useState("");
  const [toughness, setToughness] = useState("");
  const parsedPower = parse(power);
  const parsedToughness = parse(toughness);
  const stat = (value: number | undefined) =>
    value !== undefined && value >= MIN_INT && value <= MAX_INT;
  const validStats =
    (!power && !toughness) || (stat(parsedPower) && stat(parsedToughness));

  useLayoutEffect(() => {
    const node = dialog.current;
    node?.showModal();
    return () => node?.close();
  }, []);

  return (
    <dialog
      ref={dialog}
      className="command-list custom-token"
      aria-label="Custom token"
      onCancel={(event) => {
        event.preventDefault();
        onClose();
      }}
    >
      <form
        onKeyDown={(event) => {
          if (event.key === "Enter") submitWithShift.current = event.shiftKey;
        }}
        onSubmit={(event) => {
          event.preventDefault();
          if (!name.trim() || !validStats) return;
          const data = new FormData(event.currentTarget);
          const types = String(data.get("types")).trim();
          const note = String(data.get("note")).trim();
          onChoose(
            {
              card: {
                custom: {
                  name: name.trim(),
                  types,
                  colors: data.getAll("color").map(String),
                  stats:
                    parsedPower === undefined || parsedToughness === undefined
                      ? null
                      : [parsedPower, parsedToughness],
                },
              },
              note: note || null,
            },
            submitWithShift.current,
          );
        }}
      >
        <h2>Custom token</h2>
        <label>
          Name
          <input
            autoFocus
            required
            value={name}
            onChange={(event) => setName(event.currentTarget.value)}
          />
        </label>
        <label>
          Type line
          <input name="types" />
        </label>
        <fieldset>
          <legend>Colors</legend>
          {colors.map(([code, label]) => (
            <label key={code}>
              <input name="color" type="checkbox" value={code} /> {label}
            </label>
          ))}
        </fieldset>
        <div className="custom-token-stats">
          <label>
            Power
            <input
              inputMode="text"
              value={power}
              onChange={(event) => setPower(event.currentTarget.value)}
            />
          </label>
          <label>
            Toughness
            <input
              inputMode="text"
              value={toughness}
              onChange={(event) => setToughness(event.currentTarget.value)}
            />
          </label>
        </div>
        <label>
          Note
          <input name="note" />
        </label>
        <footer>
          <Button variant="secondary" onClick={onClose}>
            Cancel
          </Button>
          <Button
            type="submit"
            disabled={!name.trim() || !validStats}
            onPointerDown={(event) => {
              submitWithShift.current = event.shiftKey;
            }}
          >
            Create token
          </Button>
        </footer>
      </form>
    </dialog>
  );
}
