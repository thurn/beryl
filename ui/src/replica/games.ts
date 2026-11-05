// The screen mockups in docs/ui_mockups as game views, which replica mode draws through the
// game screen itself (game/Board.tsx), so `just ui-compare` checks the layout the game uses.
// The mockups' cards need not be in data/cards.json: each object's face is a sample's
// (screens/samples.ts), with its art window named by slot as in docs/ui_mockups/regions.json.
import type { CardFace } from "../components/Card";
import type {
  CardType,
  ObjectView,
  PlayerView,
  Side,
  ZoneName,
} from "../generated/protocol";
import { costless, SAMPLE } from "../screens/samples";

/** A mockup's game: the view, and each named object's face and art slot. */
export interface ReplicaGame {
  view: PlayerView;
  faces: Map<number, { face: CardFace; slot?: string }>;
}

/** One object of a replica: where it is, whose it is and how it looks. */
interface Thing {
  zone: ZoneName;
  owner: Side;
  face?: CardFace;
  slot?: string;
  tapped?: boolean;
}

const types = (face: CardFace): CardType[] =>
  (["land", "creature", "artifact", "instant", "sorcery"] as const).filter(
    (t) => face.typeLine.toLowerCase().includes(t),
  );

/** Numbers `things` as public slots and builds the view around them. */
function game(
  things: Thing[],
  rest: Omit<PlayerView, "objects" | "battlefield" | "players"> & {
    life: Record<Side, number>;
  },
): ReplicaGame {
  const faces: ReplicaGame["faces"] = new Map();
  const objects = things.map((t, i): ObjectView => {
    const slot = i + 1;
    if (t.face) faces.set(slot, { face: t.face, slot: t.slot });
    const field = t.zone === "battlefield";
    const creature = field && t.face?.power !== undefined;
    return {
      slot,
      zone: t.zone,
      owner: t.owner,
      controller: t.owner,
      name: t.face?.name ?? null,
      token: t.face?.typeLine.startsWith("Token") ?? false,
      tapped: t.tapped ?? false,
      summoningSick: false,
      damage: 0,
      counters: [],
      types: field && t.face ? types(t.face) : [],
      pt: creature
        ? {
            power: Number(t.face!.power),
            toughness: Number(t.face!.toughness),
          }
        : null,
      attachedTo: null,
      exiledBy: null,
      combat: null,
    };
  });
  const slots = (side: Side, zone: ZoneName) =>
    objects
      .filter((o) => o.owner === side && o.zone === zone)
      .map((o) => o.slot);
  const player = (side: Side) => ({
    life: rest.life[side],
    mana: { white: 0, blue: 0, black: 0, red: 0, green: 0, colorless: 0 },
    landsPlayed: 0,
    mulligans: 0,
    library: slots(side, "library"),
    graveyard: slots(side, "graveyard"),
    emblems: [],
  });
  const { life: _, ...view } = rest;
  return {
    view: {
      ...view,
      players: { you: player("you"), opponent: player("opponent") },
      battlefield: objects
        .filter((o) => o.zone === "battlefield")
        .map((o) => o.slot),
      objects,
    },
    faces,
  };
}

const copies = (n: number, thing: Thing): Thing[] =>
  Array.from({ length: n }, () => thing);
const hidden = (n: number, owner: Side, zone: ZoneName) =>
  copies(n, { owner, zone });
const field = (owner: Side, face: CardFace, slot: string): Thing => ({
  zone: "battlefield",
  owner,
  face,
  slot,
});

/** battlefield.png: your first main phase, priority to you, and Next goes to combat. */
const battlefield = game(
  [
    ...hidden(33, "opponent", "library"),
    ...hidden(26, "you", "library"),
    ...hidden(5, "opponent", "hand"),
    {
      zone: "graveyard",
      owner: "opponent",
      face: SAMPLE.counterspell,
    },
    {
      zone: "graveyard",
      owner: "opponent",
      face: SAMPLE.doomBlade,
      slot: "opponent-graveyard",
    },
    ...copies(3, field("opponent", SAMPLE.island, "opponent-island")),
    ...copies(3, field("opponent", SAMPLE.swamp, "opponent-swamp")),
    ...copies(3, field("you", SAMPLE.mountain, "you-mountain")),
    ...copies(3, field("you", SAMPLE.plains, "you-plains")),
    field("opponent", SAMPLE.balefulStrix, "opponent-strix"),
    field("opponent", SAMPLE.beast, "opponent-beast"),
    field("you", SAMPLE.goblinGuide, "you-goblin-guide"),
    field("you", SAMPLE.angel, "you-angel"),
    {
      zone: "hand",
      owner: "you",
      face: costless(SAMPLE.bolt),
      slot: "hand-bolt",
    },
    {
      zone: "hand",
      owner: "you",
      face: costless(SAMPLE.swords),
      slot: "hand-swords",
    },
    {
      zone: "hand",
      owner: "you",
      face: SAMPLE.mountain,
      slot: "hand-mountain",
    },
    { zone: "hand", owner: "you", face: SAMPLE.plateau, slot: "hand-plateau" },
  ],
  {
    life: { you: 22, opponent: 17 },
    status: {
      kind: "deciding",
      decision: {
        id: { session: 1, seq: 1 },
        kind: "priority",
        candidates: [{ label: "Pass", option: { kind: "pass" } }],
        resolving: null,
      },
    },
    turn: { number: 7, active: "you", step: "precombatMain", priority: "you" },
    stack: [],
    stops: "smart",
    events: [],
  },
);

export const GAMES: Record<string, ReplicaGame> = { battlefield };
