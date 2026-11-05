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
      to: "blocker-beast-mark",
      tone: "gold",
      bend: 0,
      fromSide: "top",
      toSide: { x: 0.5, y: 0.85 },
      start: "none",
      end: "none",
      fine: true,
    },
    {
      from: "attacker-angel-mark",
      to: "opponent-life",
      tone: "gold",
      fromSide: "top",
      toSide: { x: 0.673, y: 0.733 },
      leave: [-34, -138],
      arrive: [-100, -85],
      start: "none",
      fine: true,
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
    lands: [{ ...you.lands[0]!, count: 2, tapped: 2 }, ...you.lands.slice(1)],
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
      x: 1205,
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
      fromSide: "right",
      toSide: { x: 0.07, y: 0.84 },
      leave: [30, 150],
      arrive: [115, -48],
    },
    {
      from: "stack-trigger",
      to: "target-strix",
      tone: "cyan",
      fromSide: { x: 0, y: 0.055 },
      toSide: { x: 0.5, y: 0.2 },
      leave: [-150, -195],
      arrive: [-35, 150],
    },
  ],
};

/** scry.png: Preordain's scry over the dimmed battlefield (the overlay is Scry.tsx). */
const scry: TableBoard = {
  ...SAMPLE_BOARD,
  you: { ...you, hand: you.hand.slice(1) },
};

/** battlefield.png lights the tracker's first glyph while offering "To Combat". */
const battlefield: TableBoard = { ...SAMPLE_BOARD, phase: "beginning" };

export const BOARDS: Record<string, TableBoard> = {
  battlefield,
  combat,
  trigger_on_stack_with_target: trigger,
  scry,
};
