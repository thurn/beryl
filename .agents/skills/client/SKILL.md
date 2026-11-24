---
name: client
description: Inspect, drive, and verify the client headlessly. Find recordings and stories, inspect element trees and placement decisions, perform input actions, check the window matrix, and render pictures and filmstrips. Use when changing or debugging the client's scene, layout, animations, or prompts.
---

# Inspecting the client

The [the client guide](../../../client/AGENTS.md) covers the shared build,
checks and the picture required for a visual change.

## Find a scene and moment

- `just client list [prefix]` lists recordings with their seats, durations and
  number of views. `just client story` lists standalone scenes. Open a story as
  `story/<crate>/<name>` or by its `.story` path.
- `just client info <scene>` prints the views, script steps and prompts. A moment
  can be `start`, `end`, `view:12`, `prompt:1`, a step such as `step:4.1.0`,
  or a duration such as `850ms` or `1.25s`. Append an offset as in
  `view:12+250ms`. Step lines give the verb and its start and end times.
- Use `--seat you|opponent`, `--layout desktop|phone` and
  `--size <width>x<height>` for the player's view and window. Pasting a test's
  name, such as `client:verbs-zones/you/1600x900@2`, sets seat, size and scale.

## Read and drive the frame

A `tree` line gives the element's role, name, ID, place, box and state. Its
prefix is a locator such as `card "Island" #8`, `card #8` or
`button "Resolve"`. You can paste the whole line. Use
`region "prompt" > button "Cancel"` to search within another element.
Filter the tree with `--within <place|locator>` or `--role <role>`. Use
`--full` when an inventory or tree says entries were left out.

Each `--do` runs one action, in argument order after `--at` (after `--from`
for `diff`). Use `press <locator>`, `hover <locator>`, `menu <locator>`
(right click), `drag <locator> to <locator>`, or `hold <locator> over <locator>`
to stop mid-drag. Other actions are `type <locator> "<text>"`, `key <name>`,
`move <x>,<y>`, `step <duration>` and `still`. A point can replace a
locator, as in `drag 700,500 to card #8`. Give `diff` the same start and end
to isolate an action's effect.

```bash
just client tree pay-default --at prompt:2 --role button
just client explain pay-default --at prompt:2 'button "Auto Pay"'
just client diff pay-default --from prompt:2 --to prompt:2 --do 'hover button "Auto Pay"'
```

`explain` shows the node's placement, keep-clear regions, active motion,
owning feature and the sources of its style values when available. Use
`--json` for structured output. `just client map <kind> --full` gives source
paths and any scaffold recipe for that inventory. To create a part, get the
recipe with `just client new <kind> --help` before running
`just client new <kind> <name>`.

`just client story from verbs-zones --at end` saves a replayable story and prints
the command to open it. Use `--out <path.story>` to keep the file and
`--feature settings` to capture settings controls as editable values.
The [toolkit story grammar](../../../client/crates/kit_probe/AGENTS.md)
and [game story grammar](../../../client/crates/game_probe/AGENTS.md)
show how to write stories by hand.

`check` reports each fault with a `show:` command. `test` groups identical
faults and names the seat and window where they occurred. `test --every-tick`
checks between the usual check times. Bad names, moments, locators and options
exit with status 2 and print the nearest available names or matching elements.

## Run the app without a window

Use `just smoke` for startup and `just table-pair --headless` for a local
host and guest. For player-driven tests against the engine, use
`just client play --for <duration>` and the printed `just client live` commands.
The [ui-review skill](../ui-review/SKILL.md) covers live play and the
[specs skill](../specs/SKILL.md) covers card recordings.

## What these tools cannot show

A text replay supplies a stub atlas. A `tree` line confirms text layout, and
the drawn check catches glyphs the client never requested. Open a rendered PNG
to see whether the host actually drew the glyphs and whether clips or
transparency hide an element. For Retina faults, use `--scale 2` so glyphs,
icons and clips use the app's physical pixel scale. A test whose name ends in
`@2` already does this.

`png --tree` prints the tree of the frame it drew. Use `--crop <locator>` to
inspect a region. `film --from <moment> --to <moment>` draws up to 24 frames
with their times. Select them with `--frames <n>` or `--every <duration>`.
A film's `--crop` follows the element. Both commands print the picture's path
under `$BERYL_CACHE/b3d/out/`. They draw in Baseline unless you give `--tier`.
They read cached card scans and draw Beryl's card frame when a scan is
unavailable. Point `BERYL_CLIENT_SCANS` at an empty directory to use drawn faces
throughout.
