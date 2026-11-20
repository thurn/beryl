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
  (`1600x900@2`), checked at its check times (`--every-tick` checks every tick).
  A fault several tests find is printed once with a count. Paste a test's name
  into `tree` or `check` in place of a recording to see it from that seat at
  that size. The whole run takes about ten seconds; a prefix, as in
  `just b3d test b3d:verbs-zones`, runs one recording's tests.

- **`png <recording>`** draws the frame at `--at` with the real renderer, with
  no window, and prints the file's path (under `$BERYL_CACHE/b3d/out/`, named
  with the process id so two runs never share a file). `--tree` also prints the
  tree of the same frame; `--crop <locator>` keeps only the box around one
  element. View the file with the Read tool.

- **`film <recording> --from <moment> --to <moment>`** draws at most 24 frames
  as a grid no wider than 2,000 pixels in one PNG, each labelled with its time,
  and prints one line per frame: its time and what was moving. `--frames <n>` or
  `--every <duration>` chooses the frames; `--crop` follows one element. Use it
  to see an animation's path, and `diff` or `tree` to find out why.

- **`parity`** prints the parity list, one line per kind of item the web
  interface shows, `<kind> <done> / <total>` and up to five items still missing
  (`--full` for all, `--json` for the report). It runs every interface test on
  the desktop and phone windows, a minute or two; `--results <dir>` reads saved
  `test --json` outputs instead. `--check` only checks that
  `data/beryl3d/parity.json` maps every skill and web screen file.

- **`bug <report>`** prints a Beryl 3D bug report (a folder, or an id, id prefix
  or ticket of one): its words, the commit it was built from, the first 60 lines
  of `tree.txt`, the frame log's summary and the commands that reproduce it:
  `tree` and `png` at the recording's moment for a report saved while a
  recording replayed, or `just b3d app --replay <folder>/replay.json` and
  `just b3d live tree` for a live game. A web report says so.

Pictures are drawn in the Baseline tier unless `--tier` says otherwise, with the
scans in the app's scan cache and Beryl's drawn card frame where one is missing.

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
  glyphs drew. The "drawn" check of `check` and `test` faults text drawn with a
  glyph the client never asked for and an icon drawn at a size the atlas does
  not hold, but these commands deliver every glyph through a stub atlas, so a
  host that loses glyphs (an atlas page the renderer refused) shows only in a
  picture, or as `missing_glyphs` failing `just b3d app --for`.
- The interface checks judge interactive elements, and the "marks on their
  things" check holds each mark's outline round its card, ability or life total,
  and "text overlaps nothing" keeps text off other text and controls. Of the
  colours, only the action buttons' and the life orbs' are held to the web's, by
  the furniture tests (`beryl_inspect/tests/all/furniture.rs`), which read the
  web's CSS and the frame's draw records.
- A change to how something looks (text, icons, furniture, marks, prompts) is
  done only after you have read a picture of it beside the web interface's at
  the same moment and compared the two.
  `just b3d compare <recording> --at <moment>` writes both into one PNG: the web
  page shot from the recording's exported fixture on the left, Beryl 3D's frame
  on the right, at the same window size and `--scale`. The web side shows the
  moment's view at rest. Write what you compared, and the picture's path, on the
  ticket.
- `just b3d match snapshot <recording> --at <moment>` writes both clients'
  pictures and element records of the same moment, mid-animation too, at scale 2
  by default, to `$BERYL_CACHE/b3d/match/<tree>/<target>/` and prints the files:
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
  text on one side only. `--all` runs every desktop and phone test target at its
  end; give a prefix (`--all opening-play`) to run one recording's.
- `just b3d match pixels <recording> --at <moment>` lists the elements
  answerable for the pixels the two pictures differ in (by more than 8 levels,
  glyphs left out), worst first, each with the web's and Beryl 3D's box and a
  hint when one side has no element there, and writes a magnified crop of the
  web, Beryl 3D and their amplified difference for the worst ten into the
  snapshot's `crops/`. Read the crops: they show what a count cannot.
  `--only '<locator>'` (on `snapshot` too) draws that Beryl 3D element and its
  web pair alone, on black, so its own error shows apart from what lies behind
  it. `just b3d png --only '<locator>'` draws the Beryl 3D side alone.
- `just b3d match motion <recording> --from <moment> --to <moment>` compares an
  animation frame by frame: for each element that moves on either side it prints
  the largest difference of its box's center, its size and its opacity from the
  web's, each with its frame number and time, worst first, and writes a PNG plot
  of x, y, scale and opacity over time with the web's line solid and Beryl 3D's
  dashed. `--all` measures the span of each verb, effect and mark that
  `data/beryl3d/match/motion-census.json` lists and ranks them by summed
  deviation, worst first; `--refresh-census` finds the spans again from the
  recordings' timelines. Read the plots: an easing or a path that differs shows as two curves
  of different shape. Start the span after the view arrives (the `step:` lines
  of `just b3d info` give the times); `--only '<locator>'` picks one element and
  `--layout phone` the phone layout.
- `just b3d match styles <recording> --at <moment>` prints each paired
  element's style properties that differ (`<element>  <property>  <web>
  <Beryl 3D>`), compared normalized: colors within one level, lengths within a
  quarter point, shadows, gradients and filters field by field. Values `Style`
  cannot express are only counted.
- `just b3d match gaps <recording> --at <moment>` (or `--all`) lists the web
  style values `Style` cannot express, grouped by property and shape and ranked
  by screen area, with three example elements each, and writes them to
  `gaps.json`. Start a style-vocabulary change from it, and add each shape you
  teach `Style` to `data/beryl3d/match/supported.json`.
- `just b3d match score [prefix]` counts how far each reference target (a
  desktop and a phone window of every screen kind) is from the web interface
  and holds the counts to `data/beryl3d/web-match.json`. A count that rose, or
  fell without the entry being lowered, fails; `--update` lowers entries,
  `--update --raise` raises one (say why in the commit body), and
  `--refresh-web` rewrites the committed web reference under
  `data/beryl3d/match/web/`. Run it after a look change, then `--update`.
- `just b3d match samples [prefix]` draws each conformance sample (one element
  with a CSS effect the web uses, from `data/beryl3d/match/samples.json`) in
  WebKit and in Beryl 3D's renderer and prints, per sample, `pass` or `FAIL`
  with `p995` (the difference 99.5% of pixels are within; at most 4 passes),
  the largest and mean difference and a picture of web, Beryl 3D and their
  difference amplified. A sample `Style` cannot say yet prints `unsupported`
  with what it lacks and the web's picture. A sample's text is set in Open Sans
  on both sides and its glyph pixels are masked out. Use it to tune a shadow,
  gradient, filter or mask without replaying a recording; `--update` records the new
  values in `data/beryl3d/web-match.json`, which may only go down.

## Matching the web

To bring Beryl 3D closer to the web interface's look, work one screen area at a
time, largest differences first, and land each improvement as you go:

1. **Pick the worst area.** `just b3d match census` measures a fixed list of
   moments across many recordings (`data/beryl3d/match/census.json`) and ranks
   screen areas (hand, battlefield, stack, prompt, game log, piles, life orbs,
   steps, card face, cloth, screens) by differing pixels, with their box and
   style differences, the web values `Style` cannot express, the unpaired
   elements and the Beryl 3D source each area lives in. Each area lists its
   three worst moments with the `just b3d match pixels` command that shows
   each; take one of those as your target. `--areas hand,prompt` and
   `--recordings <prefix>` narrow a run, and `--json` prints the report.
   `just b3d match score` then shows which reference targets the change moves.
2. **Fix boxes.** `just b3d match boxes <target> --at <moment>` lists elements laid
   out in a different place, at a different size or with text wrapped
   differently, then the elements only one side has. A whole row of cards
   offset by one card width is an order difference, not a placement one: look
   at how each client orders that zone before moving anything. An element only
   the web has (such as a button) is a missing feature, not a style. The
   matcher leaves out wrappers (a `region` or `list` that draws nothing of its
   own), so `unmatched` counts only elements that draw something, and
   `joined.json` gives each one a `reason`. When an element is unmatched only
   because the two sides name it differently, give the web element Beryl 3D's
   role (and, with a `name` pattern, its name) in
   `data/beryl3d/match/roles.json`, or change Beryl 3D's name where it differs
   from the web's accessible name.
3. **Fix style values.** `just b3d match styles <target> --at <moment>` lists each
   paired element's property that differs, with the web's value and Beryl 3D's.
   Change the Beryl 3D `Style` or the design token it reads
   (`data/design-tokens.json`), never the web's CSS.
4. **Teach `Style` what it lacks.** `just b3d match gaps <target> --at <moment>`
   ranks the web's values `Style` cannot express by screen area. Tune a new
   effect against `just b3d match samples` before replaying a recording, and add
   the shape to `data/beryl3d/match/supported.json`.
5. **Fix pixel regions, worst first.** `just b3d match pixels <target> --at <moment>`
   names the element answerable for each region and writes crops of the worst
   ten. Read the crops: a dark ring round the screen in the difference picture
   is the table's background, a block on one side only is an element the other
   side lacks.
6. **Fix motion.** `just b3d match motion --all` ranks every verb, effect and
   mark by how far its motion is from the web's, worst first, and prints for
   each the element that differs most, the kind of difference (position, size,
   opacity or timing) and the `just b3d match motion <recording> --from
   <moment> --to <moment>` command that shows it. Run that command on the worst
   item in your area and read its plots.
7. **Check one element alone.** `--only '<locator>'` on `snapshot` and `pixels`
   draws one element and its web pair on black, so its own error shows apart
   from what lies behind it.
8. **Measure again, lower the ratchet and land.** Run the census over the
   recordings you touched (`--recordings <prefix>`) to see the area's totals
   fall. `just b3d match score --update` lowers the entries of the reference
   targets that improved; commit it with the change and land with
   `ignite land`.

A difference the loop cannot close in one change (a missing feature, a
placement rule, a new shader effect) gets its own ticket naming the files its
session must read.

Every command that takes a recording also takes `--seat you|opponent`,
`--layout desktop|phone`, `--size <width>x<height>` (1600x900 by default),
`--scale <number>` (1 by default), `--at <moment>`, `--do <action>` (repeatable,
run in order after `--at`) and `--json`. A test's name,
`b3d:verbs-zones/you/1600x900@2`, works as a recording's name and sets the seat,
the window and the scale.

## Moments, locators and actions

- **A moment** is a duration (`850ms`, `1.25s`) or a name from `info`
  (`view:12`, `prompt:1`, `step:4.1.0`, `start`, `end`), optionally followed by
  an offset: `view:12+250ms`.
- **A locator** is the start of a tree line: `card "Island" #8`, `card #8`,
  `button "Resolve"`. A whole tree line works too, since what follows the id is
  ignored. `region "prompt" > button "Cancel"` looks inside another element.
- **An action** is `press <locator>`, `hover <locator>`, `menu <locator>` (a
  right click), `drag <locator> to <locator>`, `hold <locator> over <locator>`
  (a drag not let go, to see what a drag shows mid-way),
  `type <locator> "<text>"`, `key <name>`, `move <x>,<y>`, `step <duration>` or
  `still`. A point `<x>,<y>` may stand in place of any locator, as in
  `drag 700,500 to card #8`, which sweeps from the empty table.

For example, copy a line from `tree` into `--do`:

```bash
just b3d tree verbs-zones --at 1500ms --role button
just b3d diff verbs-zones --from 1500ms --to 1500ms --do 'hover button "Resolve" #65541'
```

A wrong name, moment, locator or option exits with 2 and lists what would work,
such as the five nearest recording names or the elements a locator matched.
