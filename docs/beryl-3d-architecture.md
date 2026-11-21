# Beryl 3D architecture

*How Beryl 3D's client code is organized, and the rules that keep it that way.
Decided on 2026-11-20 in a question-and-answer session with the project's owner,
from a reading of the client as it stood that day and of the 209 fixes it had
taken in the three weeks before.*

Beryl 3D is the native 3D client of Beryl (`rust/beryl3d`). Its first design,
[Beryl 3D](beryl-3d.md), specified a client core that is a pure function of its
inputs, a renderer, hosts, and tools that let a coding agent drive the client
from a terminal. That design was built, and it works: the client has every
screen, prompt shape, verb, effect and mark that the web interface has.

This document replaces the parts of that design that describe how the client
core is organized inside. Where the two disagree, this one is right. The older
document is still the reference for what this one does not cover: the frame and
its determinism, hosts, rendering and its two visual tiers, card faces, threads,
sound, bug reports and the performance measurements.

Terms used throughout:

- **The session** is the Rust code that runs a game and tells a client what to
  show (`beryl_session`). **A view** is everything one player may see at one
  moment, and **a script** is the list of animation steps that turns the
  previous view into the next. Both are defined in `beryl_protocol`.
- **The client core** is the code that turns views, scripts and input into a
  frame. Until this design is built it is one crate, `beryl_scene`.
- **A recording** is the session's messages to one seat, saved, which the tools
  replay through the client with no rules engine running.
- **A level** is one of the three groups of crates described below. The word
  "tier" is not used for them, because it already names the two visual tiers,
  Baseline and Enhanced.

## What the first three weeks showed

The client took 528 changes in its first three weeks, 209 of them fixes. Read
together, the fixes show which properties the code did not guarantee and so had
to be repaired one case at a time.

| What kept going wrong                                                                              | How often                                                       | What was missing                                                                                                                            |
| -------------------------------------------------------------------------------------------------- | --------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------- |
| A card could not be pressed, or a card in flight was hidden, on a crowded hand, row, stack or pile | About 39 fixes; the same press-square fix was made 11 times     | Each zone computed its own poses, overlap and scrolling. Nothing guaranteed a pressable card for all of them.                               |
| A phone prompt, panel or button sat on the cards, or was too small                                 | About 20 fixes, and about 163 `phone` branches through the code | The phone layout was the desktop layout with exceptions, written wherever one was needed.                                                   |
| A mark, count or label did not follow the thing it belonged to, or sat on other text               | About 11 fixes                                                  | Things on the table and things in the flat layer were two systems. Each feature worked out for itself where the other system's things were. |
| A limit in `data/beryl3d/gates.json` had to be measured again                                      | About 24 fixes, and 45 fix commits touched the file             | Limits were pinned to exact measured counts, so every change that added work, and every pair of changes landing together, moved them.       |
| A screen read wrongly in the app but passed every tool                                             | 5 fixes                                                         | Only the picture tool's host loaded glyphs and card faces the way the checks assumed.                                                       |
| A change to `beryl_protocol` broke Beryl 3D's test builders                                        | 6 fixes                                                         | Beryl 3D built protocol values by hand in its own workspace.                                                                                |

Three more facts shaped the design:

- **One crate holds everything.** `beryl_scene` is 68,900 lines of source in 264
  files. Its modules import each other in 36 mutual pairs. An edit to one prompt
  recompiles all of it, which takes 1.3 to 2.1 seconds of the 3 seconds between
  an edit and the output of `just b3d tree`.
- **Every feature edits the same three places.** The `Client` struct has about
  55 fields. A screen or overlay adds a field there, a variant to the `Press`
  enum and an arm to `Client::press`.
- **An agent has no map.** Before changing the client an agent reads about 2,400
  lines of guidance, more than half of it about matching the web interface. No
  command lists the client's parts.

## Decisions

Later sections refer to these by number.

- **A1. Arena is the reference, and the web interface is the working reference
  until the switch.** Beryl 3D matches the web interface until its owner
  switches the app to Beryl 3D, because the web interface is the nearest thing
  to Arena that can be measured point by point. Everything that exists only to
  match the web interface is kept in one place that one change can delete (see
  [The web-parity seam](#the-web-parity-seam)). Nothing permanent depends on it.
- **A2. Three levels of crates: toolkit, game and app.** The toolkit level knows
  nothing about Magic: it names no type of `beryl_protocol`, no zone, no prompt
  and no card. A check holds the levels apart.
- **A3. The client core is divided in a short freeze, then built out in
  parallel.** The division into crates is a chain of changes that preserve
  behavior, during which no other Beryl 3D work lands. Everything else is built
  afterward on the new structure, as many changes at once as the work allows.
- **A4. Layout profiles: any number, and two are shipped.** A profile describes
  the table and the screen for one class of window. Desktop and phone portrait
  are the two shipped. The word `phone` appears only inside the phone profile.
- **A5. Frame cost is held to budgets, and its trend is recorded.** A board's
  work counters must stay under a fixed budget. The measured counts are not
  committed. A job at the tip of `staging` records them and files a ticket when
  one jumps or drifts.
- **A6. Interface tests run the distinct moments.** Each recording has a
  signature of what the client can tell apart in it. The tests that gate a
  client change run one recording for each distinct signature, and every story.
  Every moment of every card runs afterward, at the tip.
- **A7. A screen, overlay or HUD piece is a self-contained feature**: its own
  state, its own closed set of messages, an update function and a pure view
  function, in one directory, listed in one registry.
- **A8. Tuning values, motion curves and effects are data files that reload
  without a recompile.** Shaders and the primitives that effects are made of
  stay code.
- **A9. Legibility to agents is held by a generated map and by caps on the
  guides**, not by prose that is kept current by hand. The client-core crate cap
  prompts a search for a natural seam (see
  [Levels and crates](#levels-and-crates)).
- **A10. Everything on screen is a node of one tree, and relations between nodes
  are declared.** A mark is anchored to its card and a prompt dock keeps clear
  of the rows, because the code says so once, and the toolkit solves it.
- **A11. A state made without the rules engine is a story: a text file in the
  notation the tools already print.** Any moment of any recording can be written
  out as a story. Card specs, fixtures and the playground are unchanged.
- **A12. New agent tools: `map`, `explain`, `new` and `story`.**

Of the first design's decisions, C3 (a retained tree of named elements laid out
with flexbox and grid) is extended by A10 to the table. C10 (the gate holds the
worst single frame as well as the total) stands, held to budgets by A5. C14
(interface tests are fast enough to run before every push) stands, with A6
choosing which recordings run. The others are unchanged.

## Levels and crates

The client core becomes fifteen crates in three levels, with one more that holds
the web-parity code. A crate may depend on crates listed above it in its own
level and on any crate of a lower level, and on nothing else in this table.

| Crate          | Level    | What it holds                                                                                                           | Comes from, in `beryl_scene/src`                                                         |
| -------------- | -------- | ----------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- |
| `kit_base`     | toolkit  | Ids, geometry, paint, typography, `Pose`, curves, design tokens and the data store                                      | Shared types, `tokens.rs`, `data.rs`, curves and segments of `motion.rs`                 |
| `kit_text`     | toolkit  | Shaping, line breaking, glyph requests                                                                                  | `text/`                                                                                  |
| `kit_frame`    | toolkit  | `Frame`, `Draw` and `Material`: everything a renderer reads                                                             | `frame.rs`, `frame/`                                                                     |
| `kit_ui`       | toolkit  | Elements, styles, flexbox and grid layout, widgets, focus, text entry, transitions, scrolling                           | `element/`, `layout/`, `widgets/`, `input.rs`                                            |
| `kit_space`    | toolkit  | Objects in table space, the camera, tracks, containers and the arrangement solver                                       | `motion.rs`, `table/camera.rs`, the generic parts of `table/`                            |
| `kit_fx`       | toolkit  | Effect primitives, the effect player, lines and outlines between nodes                                                  | `effects/`, the shapes of `light/`                                                       |
| `kit_scene`    | toolkit  | The one tree: nodes, relations, the solve order, paint order, hit testing, input routing, the `Feature` trait           | `agent/hit.rs`, `pointer/`, and new code                                                 |
| `kit_probe`    | toolkit  | The tree printout, locators, actions, the interface checks, toolkit stories                                             | `agent/`, the generic parts of `driver/`                                                 |
| `game_table`   | game     | Each zone as a container, for each profile: rows, hands, the stack, piles, trays, badges                                | `table/`                                                                                 |
| `game_play`    | game     | Scripts compiled to tracks, verbs, beats, life and mana floats, notices, sound cues, arrows and marks                   | `play/`, `verbs/`, `beat.rs`, `notice.rs`, `sound.rs`, `light/`, `art/`, parts of `hud/` |
| `game_hud`     | game     | The game screen's features: prompts, life totals, the log, the inspector, the pile viewer, the opening hand, the result | `hud/`, `prompts/`, `inspect/`, `navigation.rs`, parts of `screens/`                     |
| `game_manual`  | game     | The manual table and its lobby                                                                                          | `manual/`, parts of `screens/`                                                           |
| `game_probe`   | game     | Game elements in recordings, table and feature stories, checks that read scripts                                        | `driver/element.rs`, `agent/steps.rs`, `testing.rs`, `probe.rs`                          |
| `app_screens`  | app      | The screens with no game: start, settings, bug report, the story browser, testing                                       | `screens/` and the settings sheet's controls                                             |
| `app_client`   | app      | `Client`: the feature registry, routing, session commands and the test driver that runs the client                      | `client/`, `command.rs`, `settings.rs`, parts of `driver/`                               |
| `scaffold_web` | scaffold | Fitting rows and piles to the web interface's positions; deleted at the switch                                          | `table/web_fit.rs`                                                                       |

The crates that are not part of the client core keep their names and jobs:
`beryl_render`, `beryl_assets`, `beryl_host`, `beryl_inspect`, `beryl_pictures`,
`beryl_record` and `beryl_3d`. `beryl_render` and `beryl_assets` depend on
toolkit crates only, so an edit to a prompt no longer rebuilds the renderer.
`beryl_scene` is deleted.

Rules, each held by the `levels` check from `data/beryl3d/levels.json`:

- **A toolkit crate does not depend on `beryl_protocol`**, on a game crate or on
  an app crate, and its source names no Magic zone, step or card type. A game
  crate hands the toolkit plain values: an opaque key for each object, a label
  for the tree printout, a size.
- **A game crate does not depend on an app crate.**
- **Only `scaffold_web` may cite `ui/src`** or hold a value copied from the web
  interface. No crate but `app_client` depends on it.
- **At 12,000 source lines, look for a natural seam in a client-core crate.**
  Keeping a crate small makes it cheaper to rebuild after an edit and lets its
  `AGENTS.md` describe the whole crate in at most 60 lines. Crate boundaries
  prevent import cycles between crates. A natural seam separates a group of
  modules with a narrow interface to the rest of the crate. Divide the crate
  only where such a seam exists. Otherwise, when the crate exceeds the default
  `cap_lines`, keep it together and record its current `lines` under its name in
  `cap_allowances` in `data/beryl3d/levels.json`. The entry's `reason` names the
  seams considered and explains why each was rejected. The cap applies only to
  the Beryl 3D client core. Crates under `rust/crates/` are outside its scope.
  The line count approximates rebuild cost. Ignite's reported rebuild counts
  compare edits to one feature, one toolkit crate and one data file with the
  rebuild count for `beryl_scene` before the division.
- **The toolkit is tested with no Magic in it.** Its tests and stories build
  rows of plain rectangles and trees of plain elements, so a change to the
  protocol cannot break them.

During the division, `levels.json` lists the toolkit crates that still name a
protocol type, with a count for each that may only go down. The division is
finished when every count is zero.

## The scene: one tree

Everything on screen is a **node**: a card, a pile, a row, a prompt bar, a
button, a line of text, an arrow, a mark. A node has an id, a role, a name, a
parent and a **space**: table space for things that have a pose under the
camera, or screen space for things laid out in logical points. The tree printed
by `just b3d tree` is this tree.

Nodes are related in three ways, and each relation is declared where the node is
made:

- **Anchor.** A node stands at a point of another node's box on screen: a mark
  on its card, a count on its pile, an arrow's end on its target, the inspector
  beside the card under the pointer, a tooltip over its button. The anchored
  node follows its target on every frame with no layout and no allocation.
- **Keep clear.** A node names what it must not cover and the places it may
  stand, in order of preference: the prompt dock stands at the right of the
  viewer's strip, or else at the right of the middle band, or else at its left,
  taking the first that is clear of the viewer's rows and the stack.
- **Show.** An element shows a table object in its box, as the pile viewer and
  the opening hand do. The object is drawn in the box and cut to the box's clip.

The toolkit solves a change in a fixed order, once, with no iteration:

1. The table's containers arrange their items (see [Arrangement](#arrangement)).
2. Screen-space layout runs, with the boxes of the table's nodes known, and each
   keep-clear node takes its first clear place.
3. Anchored nodes are placed from their targets.

An anchor or a keep-clear relation never moves anything on the table, which is
why the order needs no second pass. The split between `handle`, `prepare` and
`frame` is unchanged: steps 1 and 2 run in `handle` and may allocate, and a
frame that only animates does step 3 for the nodes whose targets moved.

Two policies that features used to apply case by case belong to the tree:

- **Paint order.** Nodes are drawn in bands, in one order for every profile: the
  table at rest, things moving over the table, screen panels, objects shown in
  elements, overlays, and what the pointer is dragging. A card in flight is
  therefore above every card at rest without its verb saying so.
- **Hit testing and input.** One hit test walks the tree in reverse paint order.
  Pointer, touch and key input reach a node through one router, and the node's
  feature receives a message. The game, the manual table and touch no longer
  have routes of their own.

The interface checks gain one rule, **relations hold**: on every frame an
anchored node is within its tolerance of its target, and a keep-clear node
covers none of what it named. It replaces the check written for marks alone.

## Arrangement

A **container** is a node that arranges items in a region of the table: a
battlefield row, a hand, the stack, a pile, a tray of cards held up to be chosen
from. A container has a **policy** from a closed set:

| Policy    | Arranges its items                                  | Used by                                  |
| --------- | --------------------------------------------------- | ---------------------------------------- |
| `row`     | Side by side, in groups, from one end or the middle | Battlefield rows                         |
| `fan`     | In an arc, toward the viewer                        | Hands                                    |
| `cascade` | Stepped, each partly under the next                 | The stack                                |
| `pile`    | Stacked, with a count                               | Libraries, graveyards, exile, twin piles |
| `grid`    | In rows and columns                                 | Trays, the pile viewer, the opening hand |
| `spot`    | One item alone                                      | A card held up to be read                |

An item is a key, a size, whether it can be pressed, the key of the group it
belongs to and the key of the item it lies under, if any. The game level decides
which items a container holds and how they are grouped. The solver,
`kit_space::arrange`, decides where they go, and it alone guarantees:

- **A press square.** Every item that can be pressed and is in view keeps an
  uncovered square on screen of the profile's target size, 24 points on the
  desktop and 44 on a phone, measured through the camera.
- **One overflow order.** A container with too many items for its region tries,
  in this order: full size; identical items gathered into piles with a count;
  items overlapped down to the press square; the container scaled down to its
  least scale; a window that scrolls, with a count of the items off each end.
  Each policy and each profile says which of these it allows.
- **Stable places.** An item that leaves moves only the items after it.
- **A pure result.** The same items, region, camera and scroll offset give the
  same poses. The result records which overflow stage applied, which
  `just b3d explain` prints.

A fix to how a crowded container looks is made in the solver, once, and every
zone and both profiles get it.

## Layout profiles

A **profile** describes one class of window: when it applies, the camera, the
table's containers and their regions, the named **slots** of the screen where
features stand, the sizes of touch targets and text, and the windows its tests
run in. A profile is a data file, `data/beryl3d/profiles/<name>.json`, and one
module in `game_table` for what data cannot say.

- **Two profiles are shipped**: `desktop`, and `phone` for a window whose
  shorter side is under 500 points. One function chooses.
- **A feature asks for a slot by name**, as `slot("prompt.dock")`, and for the
  profile's **size class**, compact or regular, when its content differs by
  size. It never asks which profile is in use.
- **The words `phone` and `desktop` appear only in the profiles** and their
  data, which the `profile-words` check holds.
- **Adding a profile** is adding a data file, a module and a row of the test
  matrix. A third profile used only by tests proves that no feature needs to
  change.

## Features

A **feature** is one screen, overlay or piece of the HUD: the prompt bar, the
game log, the pile viewer, the settings sheet, the bug report, the start screen.
Each is one directory holding:

- **State**: a plain struct. It holds only what the session does not know, such
  as whether the log is folded, how far a list is scrolled or the text of a
  draft. Anything that follows from the latest view or prompt is computed in the
  view function, so the client keeps no copy of the game that can disagree with
  the session's.
- **Messages**: a closed enum of what can happen to the feature. A message is a
  value, so an unchanged element compares equal and a test can print it.
- **`update(state, message, context)`**, which changes the state and may send a
  session command, ask the host for something or send another feature a message.
- **`view(state, inputs) -> Element`**, a pure function, run again only when the
  state or an input changes.
- **Stories**: the feature in the states worth looking at.

`kit_ui` carries a message through an element without knowing its type, as an
opaque `Action` that can be compared and printed, and the router hands it back
to the feature that built the element. `app_client` holds the registry, the one
list of features, and routes. `Client` holds no feature's state by name and no
`Press` enum.

## Data and reload

Values that are tuned are data: the camera, regions, sizes, durations, curves
and the definitions of effects. They live in `data/beryl3d/` as JSON, one file
per subject, and `kit_base` reads each into a typed struct once.

- **The client core still reads no file.** A host hands it the data as an input,
  as it hands it a view, so a frame is still a function of the inputs.
- **A release build embeds the files.** The text tools, the picture tools and
  the app started with `--watch` read them from the checkout, and the app reads
  them again when one changes.
- **A file is validated when it is read**: an unknown key, a missing key or a
  value out of range fails with the file and the key. The `b3d-data` step of the
  source checks reads every file, and fails for a key that no code reads.
- **A frame looks nothing up by name.** Values are resolved into structs when
  the data arrives.

Changing how far a card lifts is then an edit to a JSON file and one
`just b3d png`, with no compile. Verb tuning in `space.json` already takes this
path. During the conversion, `space-constants.json` holds spatial values that
still generate Rust constants.

### Effects

An **effect** is a data entry: a name, what it plays on, and layers on a shared
clock. Each layer is one **primitive** with its parameters on curves. The
primitives are a closed set that `kit_fx` implements once: a sprite of one of
the light shapes, a ring, a burst, a streak along a path, particles from an
emitter, a parameter of the subject's own surface (whiten, tint, dissolve,
opacity), a shake of the picture, and a number that floats away. A layer may be
marked for the Enhanced tier only.

The 14 effects become 14 entries that draw what the code drew. A new effect is a
data entry and a story, with no Rust and no shader change unless it needs a new
primitive.

Each definition in `data/beryl3d/effects/` names its default anchor and length
source. A step can supply an explicit target and duration in its shared timing.
Its duration defaults to the named effect's duration in
`data/ui-conventions.json`, or to the step's length. Each layer's interval
defaults to 0 through 1, with the run's end excluded. It samples the run's eased
clock by default. An elapsed clock lets a layer, such as a floating number,
advance uniformly while light follows the run's easing.

A layer parameter can be a constant, a ramp or a wave. A ramp adds `by` to
`from` over the interval from `start` to `end`, with optional easing. A wave's
amplitude uses a ramp. All of these values are typed before playback. The
shipped definitions for `glow`, `dissolve` and `loss` show how to combine
sprites, surface changes, emitters, rings and floating numbers.

## Toolkit capabilities

The web platform gave the first client these for nothing. The toolkit provides
each once, for every feature, instead of where a fix first needed it.

| Capability        | What the toolkit provides                                                                                               |
| ----------------- | ----------------------------------------------------------------------------------------------------------------------- |
| Transitions       | A style may declare how a property changes over time, and how an element enters and leaves                              |
| Scrolling         | Any element or container scrolls by wheel, drag, keys and touch, by one implementation                                  |
| Grid and flexbox  | Both, on every element                                                                                                  |
| Anchoring         | Any node to any node (A10)                                                                                              |
| Clipping          | A table object shown in an element is cut to its clip                                                                   |
| Paint             | Layered and inset shadows, drop shadows, linear and radial gradients, masks, blur and backdrop blur, grayscale, borders |
| Text              | Styled spans, inline symbols, truncation, entry and selection                                                           |
| Focus and keys    | A focus order, and shortcuts that each feature declares and `just b3d map` lists                                        |
| Pointer and touch | Hover, press, long press, wheel and drag, recognized in one place                                                       |
| Drag and drop     | Dragging an item from one container to another                                                                          |

The same scroll recognizer takes input for clipped elements and arranged
containers. A flat element keeps its offset with its retained node and scrolls
in points. An arranged row scrolls in whole groups and reports the counts off
each end. After a touch drag, the client samples the offset track on its frame
clock before it draws.

A capability's conformance is shown by stories that draw it. The web interface's
CSS samples stay the reference for paint until the switch.

## Scenes: recordings and stories

Every tool that shows or checks the client takes a **scene** by name. A scene
comes from one of two places.

- **A recording is played by the rules engine.** Card spec moments are generated
  from each card's definition and scenarios. Fixtures are boards or seeded games
  stopped on a condition. Playground examples are games a person plays. Anything
  the engine can play is reached this way, and none of it changes. A fix to how
  a card behaves in the interface still adds a scenario or a fixture.
- **A story is a text file with no engine behind it.** Stories are for what the
  engine cannot reach or need not: a board at capacity whose cards all have
  different names, an app screen with no game, and toolkit samples such as a row
  of forty rectangles or a button in each of its states.

A story is written in the notation the tools print: a line of a story is a line
of `just b3d tree`, with the few things the printout leaves out. There is one
story form for each level of thing: an element tree, a container with its items,
an effect, a feature's state, and a table with its view and script.

- **Any moment can be written out.** `just b3d story from <scene> --at <moment>`
  writes that moment as a story, and replaying the story gives the same tree. A
  fault seen in a real game is written out, cut down to what shows it, and kept
  as a test.
- **A story lives beside what it shows**, in the `stories` directory of its
  crate or feature, and is named `story/<crate>/<name>`.
- **Stories replace the states that were built in Rust by hand**: the stress
  boards, the `screen/*` recordings, the sample views of the scene's tests and
  the gallery screen's samples. The gallery screen becomes a browser of stories.
- **The builders of protocol values belong to `beryl_protocol`**, behind a
  feature that only tests and tools turn on, so that a change to a protocol type
  fixes its builder in the same commit.

## The agent interface

The first design's rules for `just b3d` stand: one entry point, commands that
are words, nothing to guess, output that can be pasted back, and bounded output.
Four commands are added.

- **`just b3d map [kind]`** prints the client's parts from its own registries:
  crates by level, features, containers, profiles and their slots, verbs,
  effects, primitives, roles, data files and checks, each with its path.
- **`just b3d explain <scene> --at <moment> <locator>`** prints why a node is
  where it is and looks as it does: its container, policy and overflow stage;
  its anchor or the places it tried to keep clear; and for each style value the
  rule, token or data key that set it, with paths.
- **`just b3d new <kind> <name>`** writes the files of a new feature, widget,
  container policy, profile, effect, verb or story, with a first story and a
  first test.
- **`just b3d story`** lists stories and writes one from a moment.

The guides are held to sizes:

- `rust/beryl3d/AGENTS.md` is at most 150 lines, and says what the levels are
  and where to look.
- Each crate has an `AGENTS.md` of at most 60 lines.
- Each kind of thing that `just b3d new` writes has a recipe of at most 30
  lines.
- The tools that compare Beryl 3D with the web interface are described in a
  skill of their own, which is deleted at the switch.

The `guides` check holds the sizes, and fails for a kind in the map with no
recipe. Structure is recorded by the map, which cannot go stale, and the guides
say only what a program cannot.

## Checks

- **The interface checks** are generic, as before, with "relations hold" added.
  They run on every story and on the corpus.
- **The corpus** holds the first recording by name for each distinct
  **signature**, plus every fixture. A signature covers what the client can tell
  apart in a recording: the shape of each view (how many objects in each zone,
  grouped as the table groups them), each script's verbs and beats, each
  prompt's shape and its numbers of candidates and buttons, and each text's
  measured number of lines. Card names and rules text are left out.
  `data/beryl3d/corpus.txt` names the corpus, and the recordings pack built
  before a landing holds only those recordings.
- **At the tip**, a job with no deadline records every moment of every card,
  runs the interface checks on all of them, and files a ticket naming any
  signature the corpus lacks, with the command that adds it.
- **Budgets.** `data/beryl3d/budgets.json` gives each gated board a budget for
  its worst frame and its total, for each work counter, and for the fragments it
  shades. A count over its budget fails. A count under it passes. A budget is
  set from the floor device's frame time with headroom, with the derivation
  beside it, and is changed by a deliberate change that says why. A frame that
  only animates still allocates nothing, which has no budget because the limit
  is zero.
- **The trend.** An Ignite exploration check records the gated boards at the
  verified tip. It includes Callgrind instruction counts when the benchmark
  machine is free. A rise of more than 5% since the previous measurement or more
  than 10% since the oldest measurement within seven days files a ticket naming
  the commit. `just b3d trend record` measures the current checkout,
  `just b3d trend check` checks the saved counts, and `just b3d trend` shows
  recent measurements from `$BERYL_CACHE/b3d/trend/frames.jsonl`.
- **Compile cost** is recorded by Ignite's rebuild counts, for an edit to one
  feature, one toolkit crate and one data file. It is a measurement, not a gate.

## Hosts and assets

One asset service prepares glyphs, icons and card faces, and every host uses it:
the desktop window, the two phone programs and the windowless host that draws
pictures. The hosts differ only in where frames go and where input comes from.
The text tools keep their stub atlas, which answers every glyph the client asks
for, because they draw nothing.

## The web-parity seam

Until the switch, Beryl 3D is fitted to the web interface (A1). All of that is
kept where one change can remove it:

- `scaffold_web`: the fit of rows and piles to the web stage's positions, the
  shear that makes a card's box as wide as the web's, and the values copied from
  the web interface's source. `game_table` exposes one hook, the fit of a
  container's region, which `scaffold_web` implements. Without it the regions
  come from the profile alone, with the camera fitted to Arena.
- `tools/src/beryl_tools/beryl3d/match/`, `data/beryl3d/match/`,
  `data/beryl3d/web-match.json`, `data/beryl3d/timeline/`,
  `data/beryl3d/timeline-parity.json` and `data/beryl3d/parity.json`.
- The `web-match` skill, which documents those tools.

The counts in `web-match.json` may only go down while this design is built. The
change that makes the switch deletes everything in this list.

## How it is built

1. **The contract.** The new crates are added empty, with `levels.json`, the
   `levels` check and Ignite's rebuild counts for the client as it stands.
2. **The division.** No other Beryl 3D change lands during this step. First the
   36 mutual imports between modules are removed, so that every import points
   down the table above. Then the modules move into their crates from the bottom
   up, with `beryl_scene` left as a crate that only re-exports them, so that
   everything else still compiles after each move. The last move deletes
   `beryl_scene`. Every interface test and every gate passes after each change,
   because nothing but the paths has changed.
3. **The build-out**, in parallel: the tree and its relations, the arrangement
   solver and each zone's move onto it, profiles, features, the data store and
   effects, the toolkit's capabilities, stories, the agent tools, budgets and
   the corpus. Budgets do not touch the client core and are built during the
   division.
4. **The close.** The toolkit crates stop naming protocol types, the guides are
   rewritten to their caps, and [Beryl 3D](beryl-3d.md) is cut down to what this
   document does not replace.

What a player sees does not change at any step, except that a state the old code
got wrong is right.

## Where a thing goes

| To add                            | Add                                                   | In                                         |
| --------------------------------- | ----------------------------------------------------- | ------------------------------------------ |
| A screen, overlay or HUD piece    | A feature directory and a line of the registry        | `game_hud`, `game_manual` or `app_screens` |
| A prompt shape                    | A feature under the prompts, and its `ShapeBody` arm  | `game_hud`                                 |
| A verb                            | One file, and its entry in `data/ui-conventions.json` | `game_play`                                |
| An effect                         | A data entry and a story                              | `data/beryl3d/effects/`                    |
| A new way to arrange cards        | A policy of the solver, with stories                  | `kit_space`                                |
| A zone, or a zone's new behavior  | A container declaration                               | `game_table`                               |
| A window class                    | A profile                                             | `data/beryl3d/profiles/`, `game_table`     |
| A widget                          | One function returning an element, with stories       | `kit_ui`                                   |
| A way to draw                     | A paint capability or a primitive, with stories       | `kit_ui`, `kit_fx`, `beryl_render`         |
| A tuning value                    | A key of a data file                                  | `data/beryl3d/`                            |
| A test of a state no game reaches | A story                                               | Beside what it shows                       |
| A test of a card's moment         | A scenario in the card's file                         | `rust/cards/`                              |

`just b3d new` writes each of these, and `just b3d map` shows the ones that
exist.
