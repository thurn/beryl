import { Button } from "../components/Controls";
import { StepBanner } from "../game/StepBanner";
import type { LinkState } from "../ipc";
import { linkBlocks } from "./link";
import "./link.css";

export function LinkBanner({
  kind,
  peer = "Opponent",
  onLobby,
}: {
  kind: LinkState["kind"];
  peer?: string;
  onLobby(): void;
}) {
  if (!linkBlocks(kind)) return null;
  return (
    <div className="table-link">
      <div className="table-link-notice">
        <StepBanner
          title={
            kind === "left"
              ? `${peer} left the table`
              : `${peer} is reconnecting`
          }
          shown
          coin={false}
        />
      </div>
      {kind === "left" && (
        <Button variant="secondary" onClick={onLobby}>
          Lobby
        </Button>
      )}
    </div>
  );
}
