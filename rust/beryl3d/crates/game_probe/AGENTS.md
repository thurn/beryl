# Working in `game_probe`

This crate reads protocol recordings for replay, game-specific interface checks,
corpus grouping and standalone stories. Start in `driver` for replay data and
`agent` for script-aware checks. The app client runs the replay. `kit_probe`
owns generic frame checks and toolkit stories.

`tests/all/` covers signatures, stories, playback and effects with fixtures in
`tests/fixtures/`. A signature change can change which recordings the interface
gate sees. Inspect the corpus when changing what a signature keeps.

## Recording signatures

`signature::Signature` is the ordered sequence of updates and click replies for
one seat. The corpus compares all seats in seat order, keeps the first name of
each group and includes every fixture separately.

- An update includes the turn step, active side, opening and undo flags, status,
  public handling, pending attack count, mulligan state, emblem and counter
  counts, floating mana colors and the incoming damage badge's presence, digit
  count and lethal glow.
- Zone cards contribute visibility, facing, tap and other visible statuses, stat
  and tag presence, counter and keyword counts and combat state. Battlefield
  cards use the same rows and pile order as placement, with tucked cards on the
  correct side. Stack items add ability state, target and paid-cost counts and
  the presence of X.
- Scripts contribute resets, beat boundaries, verbs, cause kinds, details
  controlling visible geometry, step timing and phases, and effects in order
  with their timing and subjects.
- Prompts contribute shape, body, part, state, counts, marks, payment previews
  and ordered buttons. The special purposes are `act`, `mana`, `mulligan`,
  `play-or-draw`, `priority` and `put-on-bottom`. A decision counts candidates
  by kind. A click contributes its open prompt or answer kind.
- `kit_text` measures line counts for inspectors, prompts and logs across their
  desktop and phone widths. Equal widths count once. Empty strings contribute
  zero lines.

Names contribute their effects on piling and wrapping. Rules text and object,
ability, decision and session IDs are left out.

## Table and feature stories

Use a card scenario to check legal game actions. Use a story beside the
exercised crate or feature for a client state supplied directly.

```text
story table 1600x900 desktop
life yours 14
card "Island" #8 battlefield.yours.lands tapped
card "Forest {n}" battlefield.theirs.lands x40
```

Paste card lines from `tree` with optional IDs and coordinates. `x40` repeats a
card, `{n}` counts copies and `{id:03}` pads the slot. Zones keep line order.
See [the parser](src/story/table.rs) for repeat blocks and protocol JSON.

`just b3d story from <recording> --at <moment>` writes a portable example with
the selected window, seat, time and protocol messages. Use `--feature settings`
for an editable settings story. A feature story with `replay` can drive the full
client when it depends on another feature.
