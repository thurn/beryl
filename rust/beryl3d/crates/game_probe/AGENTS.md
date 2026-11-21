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
