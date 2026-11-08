// Stand-in cards for the component gallery. Art is the placeholder painting.
import type { CardFace } from "../components/Card";

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
    abilities: [{ icon: "flying" }],
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
} satisfies Record<string, CardFace>;

/** A card as the mockups draw it in the hand: without its mana cost. */
export const costless = ({ manaCost, ...card }: CardFace): CardFace => card;
