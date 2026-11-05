// Replica mode (`?replica=<mockup>`), development builds only: renders a mockup's board in
// docs/ui_mockups with the game's own screens (the game screen itself for the mockups in
// games.ts, TableScreen for the rest), each card's art cut from the mockup itself
// (`by ui-compare` and `by ui-shots` cut it into ui/replica-art/). `just ui-compare` captures
// these at the mockup's native size and compares them part by part. `?replica=fonts` is the font
// sheet `just ui-fonts` renders.
import regions from "../../../docs/ui_mockups/regions.json";
import { SlotArt } from "../components/Card";
import { Board } from "../game/Board";
import { ObjectFaces } from "../game/faces";
import { Gallery } from "../screens/Gallery";
import { Stage, TableScreen } from "../screens/Table";
import { BOARDS } from "./boards";
import { FontSheet } from "./FontSheet";
import { GAMES, type ReplicaGame } from "./games";

const MOCKUPS: Record<string, { art: Record<string, unknown> }> =
  regions.mockups;

export default function Replica({ name }: { name: string }) {
  if (name === "fonts") {
    return <FontSheet />;
  }
  const mockup = MOCKUPS[name];
  if (!mockup) {
    throw new Error(
      `no replica of ${name}; one of ${Object.keys(MOCKUPS).join(", ")}, fonts`,
    );
  }
  const art = (slot: string) =>
    slot in mockup.art ? `/replica-art/${name}/${slot}.png` : undefined;
  const game = GAMES[name];
  return (
    <SlotArt.Provider value={art}>
      {game ? (
        <Game game={game} art={art} />
      ) : name === "ui_system" ? (
        <Gallery />
      ) : (
        <TableScreen
          board={BOARDS[name]}
          overlay={name === "scry" ? "scry" : undefined}
        />
      )}
    </SlotArt.Provider>
  );
}

/** A mockup's game on the game screen, its cards' faces and art the mockup's. */
function Game({
  game,
  art,
}: {
  game: ReplicaGame;
  art(slot: string): string | undefined;
}) {
  const face = (o: { slot: number }) => {
    const found = game.faces.get(o.slot);
    if (!found) return undefined;
    const url = found.slot && art(found.slot);
    return url ? { ...found.face, art: url } : found.face;
  };
  return (
    <ObjectFaces.Provider value={face}>
      <Stage>
        <Board
          view={game.view}
          waiting={false}
          choose={() => {}}
          setFullControl={() => {}}
          onLeave={() => {}}
        />
      </Stage>
    </ObjectFaces.Provider>
  );
}
