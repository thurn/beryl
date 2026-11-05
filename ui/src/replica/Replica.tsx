// Replica mode (`?replica=<mockup>`), development builds only: renders a mockup's board in
// docs/ui_mockups with the game's own screens (the game screen itself for the mockups in
// games.ts, TableScreen for the rest), each card's art cut from the mockup itself
// (`by ui-compare` and `by ui-shots` cut it into ui/replica-art/). `just ui-compare` captures
// these at the mockup's native size and compares them part by part. `?replica=fonts` is the font
// sheet `just ui-fonts` renders.
import regions from "../../../docs/ui_mockups/regions.json";
import { SlotArt } from "../components/Card";
import { Board } from "../game/Board";
import { ChoiceOverlay } from "../game/ChoiceOverlay";
import { choiceOf } from "../game/choices";
import { ObjectFaces } from "../game/faces";
import { Gallery } from "../screens/Gallery";
import { Stage, TableScreen } from "../screens/Table";
import { BOARDS } from "./boards";
import { FontSheet } from "./FontSheet";
import { GAMES, type ReplicaGame } from "./games";

const MOCKUPS: Record<string, { art: Record<string, unknown> }> =
  regions.mockups;
/** The first mockup with each art slot. */
const CUTS = new Map(
  Object.entries(MOCKUPS)
    .flatMap(([name, m]) =>
      Object.keys(m.art).map((slot) => [slot, name] as const),
    )
    .reverse(),
);

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
  // A slot this mockup lacks (a card it blurs past cutting) takes another mockup's cut.
  const art = (slot: string) => {
    const from = slot in mockup.art ? name : CUTS.get(slot);
    return from && `/replica-art/${from}/${slot}.png`;
  };
  const game = GAMES[name];
  return (
    <SlotArt.Provider value={art}>
      {game ? (
        <Game game={game} art={art} />
      ) : name === "ui_system" ? (
        <Gallery />
      ) : (
        <TableScreen board={BOARDS[name]} />
      )}
    </SlotArt.Provider>
  );
}

/** A mockup's game on the game screen, its cards' faces and art the mockup's, under the
 * choice its decision asks as the game screen shows it (screens/Game.tsx). */
function Game({
  game,
  art,
}: {
  game: ReplicaGame;
  art(slot: string): string | undefined;
}) {
  const face = (o: { slot: number }, onStack = false) => {
    const found = game.faces.get(o.slot);
    const look = onStack ? found?.stack : found;
    if (!look) return undefined;
    const url = look.slot && art(look.slot);
    return url ? { ...look.face, art: url } : look.face;
  };
  const choice = game.waiting ? undefined : choiceOf(game.view);
  return (
    <ObjectFaces.Provider value={face}>
      <Stage>
        <Board
          view={game.view}
          waiting={game.waiting ?? false}
          choose={() => {}}
          setFullControl={() => {}}
          onLeave={() => {}}
          dimmed={choice !== undefined}
          inert={choice !== undefined}
        />
        {choice && (
          <ChoiceOverlay
            view={game.view}
            choice={choice}
            busy={false}
            onPlan={() => {}}
            onViewBattlefield={() => {}}
            holding={game.holding}
          />
        )}
      </Stage>
    </ObjectFaces.Provider>
  );
}
