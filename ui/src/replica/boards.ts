// The boards of the screen mockups in docs/ui_mockups, on the 1600×900 stage (mockup pixels
// divided by 1672 / 1600). Slots name the art windows of docs/ui_mockups/regions.json.
import { SAMPLE, SAMPLE_BOARD } from "../screens/samples";
import type { TableBoard } from "../screens/Table";

const { opponent, you } = SAMPLE_BOARD;

/** combat.png: Goblin Guide and Angel attack, Beast blocks Goblin Guide. */
const combat: TableBoard = {
  ...SAMPLE_BOARD,
  opponent: {
    ...opponent,
    creatures: [
      { card: SAMPLE.balefulStrix, x: 432, y: 264, slot: "opponent-strix" },
      {
        card: SAMPLE.beast,
        x: 664,
        y: 264,
        combat: "blocker",
        id: "blocker-beast",
        slot: "opponent-beast",
      },
    ],
  },
  you: {
    ...you,
    creatures: [
      {
        card: SAMPLE.goblinGuide,
        x: 655,
        y: 463,
        tapped: true,
        combat: "attacker",
        id: "attacker-goblin",
        slot: "you-goblin-guide",
      },
      {
        card: SAMPLE.angel,
        x: 877,
        y: 460,
        combat: "attacker",
        id: "attacker-angel",
        slot: "you-angel",
      },
    ],
  },
  phase: "combat",
  action: { label: "Resolve Damage", caption: "Combat damage" },
  banner: { title: "Combat", detail: "Blockers declared" },
  arrows: [
    {
      from: "attacker-goblin",
      to: "blocker-beast",
      tone: "gold",
      link: true,
      bend: 0,
      fromSide: "top",
      toSide: "bottom",
    },
    {
      from: "attacker-angel",
      to: "opponent-life",
      tone: "gold",
      bend: 0.12,
      fromSide: "top",
      toSide: "bottom",
    },
  ],
};

const titanText =
  "When Inferno Titan enters the battlefield, it deals 3 damage divided as you choose among one, two, or three targets.";

/** trigger_on_stack_with_target.png: Inferno Titan's trigger targets Baleful Strix. */
const trigger: TableBoard = {
  ...SAMPLE_BOARD,
  opponent: {
    ...opponent,
    creatures: [
      {
        card: SAMPLE.balefulStrix,
        x: 465,
        y: 268,
        highlight: "target",
        id: "target-strix",
        slot: "opponent-strix",
      },
      { card: SAMPLE.beast, x: 700, y: 268, slot: "opponent-beast" },
    ],
  },
  you: {
    ...you,
    active: false,
    hand: you.hand.slice(1),
    lands: [{ ...you.lands[0]!, tapped: 1 }, ...you.lands.slice(1)],
    creatures: [
      {
        card: SAMPLE.infernoTitan,
        x: 599,
        y: 452,
        highlight: "source",
        id: "source-titan",
        slot: "you-titan",
      },
      { card: SAMPLE.angel, x: 823, y: 462, slot: "you-angel" },
    ],
  },
  phase: undefined,
  priority: "Opponent has priority",
  action: { label: "Pass", disabled: true },
  stack: [
    {
      source: SAMPLE.infernoTitan,
      label: "Triggered Ability",
      text: titanText,
      x: 1208,
      y: 222,
      width: 295,
      id: "stack-trigger",
      slot: "stack-titan",
    },
  ],
  arrows: [
    {
      from: "source-titan",
      to: "stack-trigger",
      tone: "gold",
      bend: -0.25,
      fromSide: "right",
      toSide: "left",
    },
    {
      from: "stack-trigger",
      to: "target-strix",
      tone: "cyan",
      bend: -0.3,
      fromSide: "left",
      toSide: "top",
    },
  ],
};

/** scry.png: Preordain's scry over the dimmed battlefield (the overlay is Scry.tsx). */
const scry: TableBoard = {
  ...SAMPLE_BOARD,
  you: { ...you, hand: you.hand.slice(1) },
};

export const BOARDS: Record<string, TableBoard> = {
  battlefield: SAMPLE_BOARD,
  combat,
  trigger_on_stack_with_target: trigger,
  scry,
};
