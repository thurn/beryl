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
  desktop windows, named `b3d:<recording>/<seat>/<width>x<height>`, checked at
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

Pictures are drawn in the Baseline tier with placeholder card faces, and text in
the table is not drawn yet.

Every command that takes a recording also takes `--seat you|opponent`,
`--layout desktop|phone`, `--size <width>x<height>` (1600x900 by default),
`--at <moment>`, `--do <action>` (repeatable, run in order after `--at`) and
`--json`. A test's name, `b3d:verbs-zones/you/1600x900`, works as a recording's
name and sets the seat and the window.

## Moments, locators and actions

- **A moment** is a duration (`850ms`, `1.25s`) or a name from `info`
  (`view:12`, `prompt:1`, `step:4.1.0`, `start`, `end`), optionally followed by
  an offset: `view:12+250ms`.
- **A locator** is the start of a tree line: `card "Island" #8`, `card #8`,
  `button "Resolve"`. A whole tree line works too, since what follows the id is
  ignored. `region "prompt" > button "Cancel"` looks inside another element.
- **An action** is `press <locator>`, `hover <locator>`, `menu <locator>` (a right click),
  `drag <locator> to <locator>`, `type <locator> "<text>"`, `key <name>`,
  `move <x>,<y>`, `step <duration>` or `still`.

For example, copy a line from `tree` into `--do`:

```bash
just b3d tree verbs-zones --at 1500ms --role button
just b3d diff verbs-zones --from 1500ms --to 1500ms --do 'hover button "Resolve" #65541'
```

A wrong name, moment, locator or option exits with 2 and lists what would work,
such as the five nearest recording names or the elements a locator matched.
