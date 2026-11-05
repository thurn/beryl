// The screen mockups in docs/ui_mockups as game views, which replica mode draws through the
// game screen itself (game/Board.tsx), so `just ui-compare` checks the layout the game uses.
// The mockups' cards need not be in data/cards.json: each object's face is a sample's
// (screens/samples.ts), with its art window named by slot as in docs/ui_mockups/regions.json.
import type { CardFace } from "../components/Card";
import type {
  CardType,
  ObjectView,
  PassTo,
  PlayerView,
  Side,
  Status,
  ZoneName,
} from "../generated/protocol";
import { costless, SAMPLE } from "../screens/samples";

/** How a replica's card looks: its face and the art slot cut from the mockup. */
interface Look {
  face: CardFace;
  slot?: string;
}

/** A tray card held mid-drag, its center at (x, y) in the tray (game/ChoiceOverlay.tsx). */
export interface Holding {
  slot: number;
  x: number;
  y: number;
}

/** A mockup's game: the view, each named object's look (and its look as an ability's source
 * on the stack, which the mockup crops apart), whether you wait on the opponent, and the
 * card a choice overlay's drag holds. */
export interface ReplicaGame {
  view: PlayerView;
  faces: Map<number, Look & { stack?: Look }>;
  waiting?: boolean;
  holding?: Holding;
}

/** One object of a replica: where it is, whose it is and how it looks; `id` names it to the
 * others (`combat`'s attacker, the stack's sources and targets). */
interface Thing {
  zone: ZoneName;
  owner: Side;
  face?: CardFace;
  slot?: string;
  tapped?: boolean;
  id?: string;
  combat?:
    | { kind: "attacking"; blocked: boolean }
    | { kind: "blocking"; attacker: string };
  stack?: Look;
}

/** The rest of a replica's view, given as is or from each `id`'s slot. */
type Rest = Omit<PlayerView, "objects" | "battlefield" | "players"> & {
  life: Record<Side, number>;
};

const types = (face: CardFace): CardType[] =>
  (["land", "creature", "artifact", "instant", "sorcery"] as const).filter(
    (t) => face.typeLine.toLowerCase().includes(t),
  );

/** Numbers `things` as public slots and builds the view around them. */
function game(
  things: Thing[],
  rest: Rest | ((slotOf: (id: string) => number) => Rest),
  {
    waiting = false,
    holding,
  }: {
    waiting?: boolean;
    holding?: Omit<Holding, "slot"> & { id: string };
  } = {},
): ReplicaGame {
  const slotOf = (id: string) => {
    const i = things.findIndex((t) => t.id === id);
    if (i < 0) throw new Error(`no replica object ${id}`);
    return i + 1;
  };
  const { life, ...view } = typeof rest === "function" ? rest(slotOf) : rest;
  const faces: ReplicaGame["faces"] = new Map();
  const objects = things.map((t, i): ObjectView => {
    const slot = i + 1;
    if (t.face) faces.set(slot, { face: t.face, slot: t.slot, stack: t.stack });
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
      combat:
        t.combat?.kind === "attacking"
          ? { ...t.combat, planeswalker: null }
          : t.combat
            ? { kind: "blocking", attacker: slotOf(t.combat.attacker) }
            : null,
    };
  });
  const slots = (side: Side, zone: ZoneName) =>
    objects
      .filter((o) => o.owner === side && o.zone === zone)
      .map((o) => o.slot);
  const player = (side: Side) => ({
    life: life[side],
    mana: { white: 0, blue: 0, black: 0, red: 0, green: 0, colorless: 0 },
    landsPlayed: 0,
    mulligans: 0,
    library: slots(side, "library"),
    graveyard: slots(side, "graveyard"),
    emblems: [],
  });
  return {
    waiting,
    holding: holding && { ...holding, slot: slotOf(holding.id) },
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

/** The mockups' opponent: library, hand, graveyard and lands as battlefield.png has them. */
const opponentSide: Thing[] = [
  ...hidden(33, "opponent", "library"),
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
];

const plains = copies(3, field("you", SAMPLE.plains, "you-plains"));
const hand = {
  bolt: {
    zone: "hand",
    owner: "you",
    face: costless(SAMPLE.bolt),
    slot: "hand-bolt",
  },
  swords: {
    zone: "hand",
    owner: "you",
    face: costless(SAMPLE.swords),
    slot: "hand-swords",
  },
  mountain: {
    zone: "hand",
    owner: "you",
    face: SAMPLE.mountain,
    slot: "hand-mountain",
  },
  plateau: {
    zone: "hand",
    owner: "you",
    face: SAMPLE.plateau,
    slot: "hand-plateau",
  },
} satisfies Record<string, Thing>;

/** A priority decision whose pass goes to `passTo`. */
const priority = (passTo: PassTo): Status => ({
  kind: "deciding",
  decision: {
    id: { session: 1, seq: 1 },
    kind: "priority",
    candidates: [{ label: "Pass", option: { kind: "pass" } }],
    resolving: null,
    ask: { kind: "plain" },
    passTo,
    source: null,
  },
});

/** battlefield.png: your first main phase, priority to you, and Next goes to combat. */
const battlefield = game(
  [
    ...opponentSide,
    ...hidden(26, "you", "library"),
    ...copies(3, field("you", SAMPLE.mountain, "you-mountain")),
    ...plains,
    field("opponent", SAMPLE.balefulStrix, "opponent-strix"),
    field("opponent", SAMPLE.beast, "opponent-beast"),
    field("you", SAMPLE.goblinGuide, "you-goblin-guide"),
    field("you", SAMPLE.angel, "you-angel"),
    hand.bolt,
    hand.swords,
    hand.mountain,
    hand.plateau,
  ],
  {
    life: { you: 22, opponent: 17 },
    status: priority({ kind: "step", step: "beginningOfCombat" }),
    turn: { number: 7, active: "you", step: "precombatMain", priority: "you" },
    stack: [],
    stops: "smart",
    events: [],
  },
);

/** combat.png: Goblin Guide and Angel attack, Beast blocks Goblin Guide, and passing priority
 * deals the damage. */
const combat = game(
  [
    ...opponentSide,
    ...hidden(26, "you", "library"),
    ...copies(3, field("you", SAMPLE.mountain, "you-mountain")),
    ...plains,
    field("opponent", SAMPLE.balefulStrix, "opponent-strix"),
    {
      ...field("opponent", SAMPLE.beast, "opponent-beast"),
      combat: { kind: "blocking", attacker: "goblin" },
    },
    {
      ...field("you", SAMPLE.goblinGuide, "you-goblin-guide"),
      id: "goblin",
      tapped: true,
      combat: { kind: "attacking", blocked: true },
    },
    {
      ...field("you", SAMPLE.angel, "you-angel"),
      combat: { kind: "attacking", blocked: false },
    },
    hand.bolt,
    hand.swords,
    hand.mountain,
    hand.plateau,
  ],
  {
    life: { you: 22, opponent: 17 },
    status: priority({ kind: "combatDamage" }),
    turn: {
      number: 7,
      active: "you",
      step: "declareBlockers",
      priority: "you",
    },
    stack: [],
    stops: "smart",
    events: [],
  },
);

/** trigger_on_stack_with_target.png: Inferno Titan's enters trigger targets Baleful Strix, and
 * you wait while the opponent has priority. Its stack text is the mockup's wording. */
const trigger = game(
  [
    ...opponentSide,
    ...hidden(26, "you", "library"),
    field("you", SAMPLE.mountain, "you-mountain"),
    ...copies(2, {
      ...field("you", SAMPLE.mountain, "you-mountain"),
      tapped: true,
    }),
    ...plains,
    {
      ...field("opponent", SAMPLE.balefulStrix, "opponent-strix"),
      id: "strix",
    },
    field("opponent", SAMPLE.beast, "opponent-beast"),
    {
      ...field("you", SAMPLE.infernoTitan, "you-titan"),
      id: "titan",
      stack: {
        face: {
          ...SAMPLE.infernoTitan,
          text: "When Inferno Titan enters the battlefield, it deals 3 damage divided as you choose among one, two, or three targets.",
        },
        slot: "stack-titan",
      },
    },
    field("you", SAMPLE.angel, "you-angel"),
    hand.swords,
    hand.mountain,
    hand.plateau,
  ],
  (slot) => ({
    life: { you: 22, opponent: 17 },
    status: priority({ kind: "resolve" }),
    turn: {
      number: 7,
      active: "you",
      step: "precombatMain",
      priority: "opponent",
    },
    stack: [
      {
        kind: "ability",
        source: slot("titan"),
        ability: 0,
        granted: false,
        name: SAMPLE.infernoTitan.name,
        triggered: true,
        nth: 0,
        controller: "you",
        targets: [{ kind: "object", slot: slot("strix") }],
        x: 0,
      },
    ],
    stops: "smart",
    events: [],
  }),
  { waiting: true },
);

/** scry.png: Preordain resolves, its scry 2 shows Island and Counterspell, and Island is
 * being dragged from the top toward the bottom of your library. */
const scry = game(
  [
    ...opponentSide,
    ...hidden(24, "you", "library"),
    {
      zone: "library",
      owner: "you",
      face: SAMPLE.island,
      slot: "scry-island",
      id: "island",
    },
    {
      zone: "library",
      owner: "you",
      face: SAMPLE.counterspell,
      slot: "scry-counterspell",
      id: "counterspell",
    },
    ...copies(3, field("you", SAMPLE.mountain, "you-mountain")),
    ...plains,
    field("opponent", SAMPLE.balefulStrix, "opponent-strix"),
    field("opponent", SAMPLE.beast, "opponent-beast"),
    field("you", SAMPLE.goblinGuide, "you-goblin-guide"),
    field("you", SAMPLE.angel, "you-angel"),
    {
      zone: "stack",
      owner: "you",
      face: SAMPLE.preordain,
      slot: "scry-preordain",
      id: "preordain",
    },
    hand.swords,
    hand.mountain,
    hand.plateau,
  ],
  (slot) => ({
    life: { you: 22, opponent: 17 },
    status: {
      kind: "deciding",
      decision: {
        id: { session: 1, seq: 1 },
        kind: "chooseUpTo",
        candidates: [
          { label: "Island", option: { kind: "object", slot: slot("island") } },
          {
            label: "Counterspell",
            option: { kind: "object", slot: slot("counterspell") },
          },
          { label: "Done", option: { kind: "done" } },
        ],
        resolving: {
          kind: "spell",
          slot: slot("preordain"),
          targets: [],
          x: 0,
        },
        ask: { kind: "scry", count: 2 },
        passTo: null,
        source: slot("preordain"),
      },
    },
    turn: { number: 7, active: "you", step: "precombatMain", priority: "you" },
    stack: [],
    stops: "smart",
    events: [],
  }),
  { holding: { id: "island", x: 312, y: 266 } },
);

export const GAMES: Record<string, ReplicaGame> = {
  battlefield,
  combat,
  trigger_on_stack_with_target: trigger,
  scry,
};
