# Game probe

## Recording signatures

A signature is an ordered sequence of game updates and click replies for one
seat. The corpus compares the signatures of all seats in seat order. It keeps
the first name in each group and includes every fixture separately.

- Each update records the turn step, active side, opening and undo flags,
  status, the opponent's public handling and the count of pending attackers. For
  either player it records mulligan state, counts of emblems and named counters,
  and which mana colors are floating.
- Outside the battlefield it records the cards in each zone by side. On the
  battlefield it reads the same rows and pile policy as placement, including the
  cards tucked to the left or right of each host. For each card it records
  visibility, facing, tap, token, sickness, damage, phasing, untap lock, hidden
  pick, owner mismatch, stat and tag presence, counter and keyword counts, and
  combat state. Piles keep their visual order. The stack also records item
  types, ability state, target and paid-cost counts, and the presence of X.
- Each script records reset size, beat boundaries, verbs, cause kinds, and the
  details affecting moves, streaks, arrows, marks, lifts and notices. It also
  includes the card state carried by each verb, step timing and phases, and
  effects in order with their timing and subject kinds.
- Each prompt records its shape, body shape, part, counts and state, mark kinds,
  payment preview sizes, and button kinds and states in display order. It
  distinguishes the six purposes that select a special presentation: `act`,
  `mana`, `mulligan`, `play-or-draw`, `priority` and `put-on-bottom`. A decision
  also counts candidates by kind. An open click reply contributes its prompt,
  and an answered click contributes the reply kind.
- Text contributes its line counts from `kit_text`. Inspector names and tags use
  the desktop inspector's width steps and window heights. Prompt titles, button
  labels, captions and body labels use the desktop dock's minimum and maximum
  widths and the phone pill's full width and width beside a minimum button. The
  generated game log and recorded manual log use the desktop log and phone
  sheet's inner widths. Equal widths are measured once. An empty string
  contributes zero lines.

Names can affect row piling and text wrapping. The signature compares only their
layout effects. It also leaves out rules text and object, ability, decision and
session IDs.

## Table and feature stories

Use a story for a state the engine cannot reach or need not play. A fault in how
a card behaves belongs in a scenario or fixture, where the engine can check
which actions are legal.

`just b3d story` lists stories. Store a story beside the crate or feature that
it exercises, in its `stories/` directory. Open it by its `story/<crate>/<name>`
name or by its `.story` path.

```text
story table 1600x900 desktop
life yours 14
life theirs 9
card "Island" #8 battlefield.yours.lands tapped
card "Forest {n}" battlefield.theirs.lands x40
card #49 hand.theirs
card "Spell" stack
```

Paste a card line from `just b3d tree` or leave out its screen coordinates. A
missing ID is assigned the next free slot. `x40` repeats a card forty times. If
the name uses `{n}`, the names differ from 1 through 40. An explicit ID on a
repeat starts the run of IDs. Zones contain cards in line order, from the bottom
of a pile when the zone is ordered that way. Use `stack.theirs` for an
opponent's spell, and leave out the name for an unknown card. An `ability` line
takes an ID followed by a stack ability as protocol JSON.

An optional `prompt` line takes a protocol `DecisionView` as JSON. An optional
`script` line takes a protocol `Script` as JSON and plays it from the table
defined by the card lines. Leave its timing empty to use the session's timing.
When both are present, the prompt opens after the script.

A settings story uses key and value lines for the sheet's transient state and
the inputs its view reads. Omitted controls use the app defaults.

```text
story feature 1600x900 desktop
feature settings
screen game
confirm-concede true
music 19
muted true
```

`just b3d story from verbs-zones --at end` writes a table story to a temporary
file and prints the command to replay it. Give `--out <path.story>` to choose
the file. A capture keeps the selected seat, window, clock and the recording's
protocol messages so it can reproduce active scripts, prompts and effects
without the engine or recordings pack. Each `view` is followed by its card
lines. The JSON after `|` keeps card details the tree leaves out. Replay looks
up `start` and `end` at the captured instant. A capture made with `--profile`
keeps that profile's name.

Add `--feature settings` to capture the settings sheet as editable values. Other
registered features with standalone views can reuse the capture's recording to
rebuild their own view at that instant. The feature preview draws the feature
alone.
