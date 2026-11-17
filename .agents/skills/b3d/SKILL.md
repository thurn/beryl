---
name: b3d
description: See and check what Beryl 3D, the native 3D client in rust/beryl3d, shows at any moment of a recording, as text, with no window and no GPU — `just b3d list`, `info`, `tree`, `diff`, `check` and `test`, and pictures of a moment or a span with `png` and `film`. Lists recordings and their moments, prints the elements on screen as a tree whose lines are locators, drives the client with actions (hover, press, drag), diffs two moments, runs every interface check on every tick, and runs every interface test over the window matrix. Use when changing or debugging Beryl 3D's scene, layout, animation or prompts, or checking a Beryl 3D change from the player's side.
---

# Seeing and checking Beryl 3D

`just b3d` lists every Beryl 3D command, and `just b3d <command> --help` gives
one command's options with two examples. The design is "The agent interface" in
`docs/beryl-3d.md`.

Use these after each edit to `rust/beryl3d`. They build the `b3d` program of
`beryl_inspect` and play a recording through the client in-process: no engine,
no window, no browser. Nothing has to be remembered between runs, because every
printout begins with the command that prints it again and every name it prints
can be pasted into the next command.

## The commands

```bash
just b3d list verbs                          # recordings whose names start with "verbs"
just b3d info verbs-zones                    # every moment of one recording
just b3d tree verbs-zones --at 1500ms        # the elements on screen at a moment
just b3d diff verbs-zones --from 800ms --to 1500ms   # what appeared, left, moved or changed
just b3d check verbs-zones                   # every interface check on every tick
just b3d test b3d:verbs-zones                # the interface tests whose names start so
just b3d png verbs-zones --at 1500ms --tree  # a picture of that moment, and its tree
just b3d film verbs-zones --from 0ms --to 3s --frames 12   # twelve frames in one picture
just b3d perf verbs-zones --verbs               # frame timing of a fixture played in the app
```

- **`list [prefix]`** prints each recording's name, its seat (`--seat you` or
  `--seat opponent`), its length and how many views it has, the first 95 unless
  `--full`.
- **`info <recording>`** prints one line per moment, each starting with the text
  `--at` takes: `start`, each `view:N`, each script step `step:V.B.S` with its
  verb, the card it moves, start and end, each `prompt:N` with its buttons, and
  `end`; the first 95 unless `--full`. A fault names a step by the same
  `step:V.B.S`.
- **`tree <recording>`** prints the command, a line that says where in the
  recording it is (`view 6 of 36, t=1500ms, animating, 48 elements`), and the
  elements: role, name, id, place, box and states. `--within <place|locator>`,
  `--role <role>` and `--full` choose what is printed.
- **`diff <recording> --from <moment> --to <moment>`** prints each element that
  changed as its tree line, with what changed after it in brackets. `--do`
  actions run after `--from`, so `--from` and `--to` at the same moment show
  what an action changes.
- **`check <recording>`** runs every check on every tick for one seat and window
  and prints each fault with a `show:` command, then one closing line. It exits
  with 1 when it finds a fault.
- **`test [prefix]`** runs every interface test whose name starts with the
  prefix, on every core: each recording from each seat in each of the four
  desktop windows, named `b3d:<recording>/<seat>/<width>x<height>`, with `@2`
  after the size for the window played at a Retina display's scale
  (`1600x900@2`), checked at
  its check times (`--every-tick` checks every tick). A fault several tests find
  is printed once with a count. Paste a test's name into `tree` or `check` in
  place of a recording to see it from that seat at that size. The whole run
  takes about ten seconds; a prefix, as in `just b3d test b3d:verbs-zones`, runs
  one recording's tests.

- **`png <recording>`** draws the frame at `--at` with the real renderer, with no
  window, and prints the file's path (under `$BERYL_CACHE/b3d/out/`, named with the
  process id so two runs never share a file). `--tree` also prints the tree of the
  same frame; `--crop <locator>` keeps only the box around one element. View the
  file with the Read tool.
- **`film <recording> --from <moment> --to <moment>`** draws at most 24 frames as
  a grid no wider than 2,000 pixels in one PNG, each labelled with its time, and
  prints one line per frame: its time and what was moving. `--frames <n>` or
  `--every <duration>` chooses the frames; `--crop` follows one element. Use it
  to see an animation's path, and `diff` or `tree` to find out why.

- **`parity`** prints the parity list, one line per kind of item the web
  interface shows, `<kind> <done> / <total>` and up to five items still missing
  (`--full` for all, `--json` for the report). It runs every interface test on
  the desktop and phone windows, a minute or two; `--results <dir>` reads saved
  `test --json` outputs instead. `--check` only checks that
  `data/beryl3d/parity.json` maps every skill and web screen file.
- **`bug <report>`** prints a Beryl 3D bug report (a folder, or an id, id prefix
  or bead of one): its words, the commit it was built from, the first 60 lines
  of `tree.txt`, the frame log's summary and the commands that reproduce it:
  `tree` and `png` at the recording's moment for a report saved while a
  recording replayed, or `just b3d app --replay <folder>/replay.json` and
  `just b3d live tree` for a live game. A web report says so.

Pictures are drawn in the Baseline tier unless `--tier` says otherwise, with
the scans in the app's scan cache and Beryl's drawn card frame where one is
missing.

## What these tools cannot show

Every command here lays the frame out at one physical pixel to a point unless
`--scale` says otherwise. A Retina Mac runs the desktop app at scale 2, and
faults that only happen there (letters missing from a word, a list drawn empty
by its clip) show only at `--scale 2`: the client is then told that scale, asks
for its glyphs and icons at it and gives its clips in physical pixels, as in the
desktop app. Draw a look you changed with `png --scale 2`. The windowless app
(`just b3d app` without a window) runs at scale 2 on macOS unless `--scale`
gives another, and `just b3d test` plays its 1600x900 window at scale 2.

- A `tree` line with the right string says the text was laid out, not that its
  glyphs drew. Only a picture shows that.
- The interface checks judge interactive elements, and the "marks on their
  things" check holds each mark's outline round its card, ability or life
  total, and "text overlaps nothing" keeps text off other text and controls.
  Of the colours, only the action buttons' and the life orbs' are held to the
  web's, by the furniture tests (`beryl_inspect/tests/all/furniture.rs`), which
  read the web's CSS and the frame's draw records.
- A change to how something looks (text, icons, furniture, marks, prompts) is
  done only after you have read a picture of it beside the web interface's at
  the same moment and compared the two.
  `just b3d compare <recording> --at <moment>` writes both into one PNG: the web
  page shot from the recording's exported fixture on the left, Beryl 3D's frame
  on the right, at the same window size and `--scale`. The web side shows the
  moment's view at rest. Write what you compared, and the picture's path, on
  the bead.
- `just b3d match snapshot <recording> --at <moment>` writes both clients'
  pictures and element records of the same moment, mid-animation too, at scale
  2 by default, to `$BERYL_CACHE/b3d/match/<target>/` and prints the files:
  `web.png`, `b3d.png`, and `web.json` and `b3d.json`, where each element has
  its role, name, box, paint order, parent, CSS style and lines of text with
  their baselines. `report.txt` warns of inputs the two sides do not share
  (fonts, card scans). A snapshot is reused until either client's sources
  change; `--fresh` takes it again and `--art` draws the app's scans on both
  sides. `just b3d tree <recording> --at <moment> --records` prints Beryl 3D's
  records alone.
- `just b3d match boxes <recording> --at <moment>` pairs the two clients'
  elements and prints the layout differences as numbers, worst first: each
  paired element whose box moved or resized by more than half a point (web box,
  Beryl 3D box, dx dy dw dh) or whose text wraps onto a different number of
  lines (`WRAP n/m`), the elements only one side has, and text drawn over other
  text on one side only. `--all` runs every desktop and phone test target at
  its end; give a prefix (`--all opening-play`) to run one recording's.

Every command that takes a recording also takes `--seat you|opponent`,
`--layout desktop|phone`, `--size <width>x<height>` (1600x900 by default),
`--scale <number>` (1 by default), `--at <moment>`, `--do <action>` (repeatable, run in order after `--at`) and
`--json`. A test's name, `b3d:verbs-zones/you/1600x900@2`, works as a
recording's name and sets the seat, the window and the scale.

## Moments, locators and actions

- **A moment** is a duration (`850ms`, `1.25s`) or a name from `info`
  (`view:12`, `prompt:1`, `step:4.1.0`, `start`, `end`), optionally followed by
  an offset: `view:12+250ms`.
- **A locator** is the start of a tree line: `card "Island" #8`, `card #8`,
  `button "Resolve"`. A whole tree line works too, since what follows the id is
  ignored. `region "prompt" > button "Cancel"` looks inside another element.
- **An action** is `press <locator>`, `hover <locator>`, `menu <locator>` (a right click),
  `drag <locator> to <locator>`, `hold <locator> over <locator>` (a drag not let go, to
  see what a drag shows mid-way), `type <locator> "<text>"`, `key <name>`,
  `move <x>,<y>`, `step <duration>` or `still`. A point `<x>,<y>` may stand in place of
  any locator, as in `drag 700,500 to card #8`, which sweeps from the empty table.

For example, copy a line from `tree` into `--do`:

```bash
just b3d tree verbs-zones --at 1500ms --role button
just b3d diff verbs-zones --from 1500ms --to 1500ms --do 'hover button "Resolve" #65541'
```

A wrong name, moment, locator or option exits with 2 and lists what would work,
such as the five nearest recording names or the elements a locator matched.
