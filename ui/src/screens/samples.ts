// Stand-in cards and the table demo's board, named and placed as in docs/ui_mockups. Art is the
// placeholder painting until the card data bead supplies real art.
import type { CardFace } from "../components/Card";
import type { TableBoard } from "./Table";

const land = (name: string, typeLine: string): CardFace => ({
  name,
  typeLine,
  colors: [],
});

export const SAMPLE = {
  infernoTitan: {
    name: "Inferno Titan",
    manaCost: "{4}{R}{R}",
    typeLine: "Creature — Giant",
    colors: ["R"],
    power: "6",
    toughness: "6",
    text: "Whenever Inferno Titan enters or attacks, it deals 3 damage divided as you choose among one, two, or three targets.",
  },
  balefulStrix: {
    name: "Baleful Strix",
    manaCost: "{U}{B}",
    typeLine: "Artifact Creature — Bird",
    colors: ["U", "B"],
    power: "1",
    toughness: "1",
    ability: "flying",
    text: "Flying, deathtouch\nWhen Baleful Strix enters, draw a card.",
  },
  beast: {
    name: "Beast",
    typeLine: "Token Creature — Beast",
    colors: ["G"],
    power: "3",
    toughness: "3",
  },
  angel: {
    name: "Angel",
    typeLine: "Token Creature — Angel",
    colors: ["W"],
    power: "4",
    toughness: "4",
  },
  goblinGuide: {
    name: "Goblin Guide",
    manaCost: "{R}",
    typeLine: "Creature — Goblin Scout",
    colors: ["R"],
    power: "2",
    toughness: "2",
  },
  swords: {
    name: "Swords to Plowshares",
    manaCost: "{W}",
    typeLine: "Instant",
    colors: ["W"],
    text: "Exile target creature. Its controller gains life equal to its power.",
  },
  bolt: {
    name: "Lightning Bolt",
    manaCost: "{R}",
    typeLine: "Instant",
    colors: ["R"],
    text: "Lightning Bolt deals 3 damage to any target.",
  },
  counterspell: {
    name: "Counterspell",
    manaCost: "{U}{U}",
    typeLine: "Instant",
    colors: ["U"],
    text: "Counter target spell.",
  },
  preordain: {
    name: "Preordain",
    manaCost: "{U}",
    typeLine: "Sorcery",
    colors: ["U"],
    text: "Scry 2, then draw a card.",
  },
  doomBlade: {
    name: "Doom Blade",
    manaCost: "{1}{B}",
    typeLine: "Instant",
    colors: ["B"],
    text: "Destroy target nonblack creature.",
  },
  island: land("Island", "Basic Land — Island"),
  plains: land("Plains", "Basic Land — Plains"),
  swamp: land("Swamp", "Basic Land — Swamp"),
  mountain: land("Mountain", "Basic Land — Mountain"),
  plateau: land("Plateau", "Land — Mountain Plains"),
  fullArtIsland: { ...land("Island", "Basic Land — Island"), fullArt: true },
} satisfies Record<string, CardFace>;

/** A card as the mockups draw it in the hand: without its mana cost. */
export const costless = ({ manaCost, ...card }: CardFace): CardFace => card;

/** The board of docs/ui_mockups/battlefield.png: the table demo, and the base of the replicas
 * (src/replica/). Slots name each card's art in replica mode. */
export const SAMPLE_BOARD: TableBoard = {
  opponent: {
    life: 17,
    library: 33,
    graveyard: [SAMPLE.counterspell, SAMPLE.doomBlade],
    hand: 5,
    lands: [
      {
        card: SAMPLE.island,
        count: 3,
        x: 159,
        y: 127,
        slot: "opponent-island",
      },
      { card: SAMPLE.swamp, count: 3, x: 367, y: 122, slot: "opponent-swamp" },
    ],
    creatures: [
      { card: SAMPLE.balefulStrix, x: 607, y: 221, slot: "opponent-strix" },
      { card: SAMPLE.beast, x: 808, y: 221, slot: "opponent-beast" },
    ],
  },
  you: {
    life: 22,
    active: true,
    library: 26,
    graveyard: [],
    hand: [
      { card: costless(SAMPLE.bolt), slot: "hand-bolt" },
      { card: costless(SAMPLE.swords), slot: "hand-swords" },
      { card: SAMPLE.mountain, slot: "hand-mountain" },
      { card: SAMPLE.plateau, slot: "hand-plateau" },
    ],
    lands: [
      { card: SAMPLE.mountain, count: 3, x: 101, y: 566, slot: "you-mountain" },
      { card: SAMPLE.plains, count: 3, x: 326, y: 564, slot: "you-plains" },
    ],
    creatures: [
      { card: SAMPLE.goblinGuide, x: 588, y: 418, slot: "you-goblin-guide" },
      { card: SAMPLE.angel, x: 810, y: 419, slot: "you-angel" },
    ],
  },
  phase: "main1",
  action: { label: "Next", caption: "To Combat" },
};
