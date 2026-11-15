import { useRef, useState } from "react";
import { Button } from "../../components/Controls";
import type { LinkState, LobbyView } from "../../ipc";

export function LobbySummary({
  view,
  link,
}: {
  view: LobbyView;
  link?: LinkState;
}) {
  const invite = useRef<HTMLInputElement>(null);
  const [copied, setCopied] = useState<boolean>();
  const { invite: code, seats } = view;
  if (seats.length < 2) return null;
  const score = seats.map(({ wins }) => wins).join("–");
  const winner = seats.find(({ seat }) => seat === view.lastWinner);
  const copy = async (code: string) => {
    try {
      await navigator.clipboard.writeText(code);
      setCopied(true);
    } catch {
      invite.current?.focus();
      invite.current?.select();
      setCopied(false);
    }
  };
  return (
    <section className="lobby-section" aria-label="Match">
      {code && (
        <div className="lobby-actions lobby-invite">
          <label>
            Invite code
            <input readOnly ref={invite} value={code} />
          </label>
          <Button variant="secondary" onClick={() => void copy(code)}>
            Copy
          </Button>
          {copied !== undefined && (
            <span role="status">
              {copied ? "Copied" : "Copy the selected code"}
            </span>
          )}
        </div>
      )}
      <ul className="lobby-seats">
        {seats.map(({ seat, name, loaded, mainCount, sideCount, ready }) => (
          <li key={seat}>
            <strong>{name ?? "Guest"}</strong>
            <span>
              {loaded
                ? `${mainCount} main · ${sideCount} sideboard`
                : "No deck loaded"}
            </span>
            {seat === 1 && ready && <span>Ready</span>}
          </li>
        ))}
      </ul>
      <p>
        Match score <strong>{score}</strong>
      </p>
      {view.gameNumber > 1 && (
        <p role="status">
          {winner
            ? `${winner.name} won game ${view.gameNumber - 1}`
            : "No result"}
        </p>
      )}
      {link?.kind === "connected" ? (
        <p>{`${link.direct ? "Direct" : "Relayed"}${link.roundTripMs === null ? "" : ` · ${link.roundTripMs} ms`}`}</p>
      ) : (
        !seats[1]?.name && <p>Waiting for guest…</p>
      )}
    </section>
  );
}
