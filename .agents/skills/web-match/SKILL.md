---
name: web-match
description: Delete this skill at the switch from Beryl's web client to Beryl 3D. Until then, use it to compare the clients' pictures, element records, motion, timelines and feature coverage or update the committed web references.
---

Delete this skill and its `.claude/skills/web-match` symlink at the switch
from the web client to Beryl 3D.

# Comparing Beryl 3D with the web client

Use the same recording, seat, layout, window, scale and moment on both sides.
The [b3d skill](../b3d/SKILL.md) explains moments and locators. A test name such
as `b3d:verbs-zones/you/1600x900@2` supplies the seat, window and scale.
Comparisons run Beryl 3D at real time with `--no-catch-up`. Give the same
flag to `tree` or `png` to reproduce the Beryl 3D frame on its own.

```bash
just b3d compare pay-default --at prompt:2 --scale 2
just b3d match snapshot pay-default --at prompt:2
just b3d match boxes pay-default --at prompt:2
```

`compare` writes one PNG with the web client on the left and Beryl 3D on
the right. It exports the recording to load the web page. Use `--web <png>`
to reuse the web screenshot and give the same window and scale as that image.

`match snapshot` defaults to scale 2. It takes pictures and records each
element's role, name, parent, box, paint order, style and text lines, including
during animations. It prints the paths under
`$BERYL_CACHE/b3d/match/<tree>/<target>/`. Read `report.txt` for differences in
fonts and card scans before comparing `web.png`, `b3d.png`, `web.json` and
`b3d.json`. The snapshot is reused while both clients' sources match the
cached inputs. Use `--fresh` to retake it or `--art` for cached scans on both
sides. Use `tree --records` for Beryl 3D records alone.

## Find and measure a difference

1. Run `just b3d match census` to rank areas such as the hand, battlefield
   and prompt by differing pixels. Each includes its source directory, box
   and style counts and the three worst moments with commands to inspect them.
   Narrow the report with `--areas hand,prompt` or `--recordings <prefix>`.
   Use `--json` for structured output. The report is saved under
   `$BERYL_CACHE/b3d/match/census/`.
2. Run `just b3d match boxes <recording> --at <moment>` for unmatched
   elements, boxes differing by more than half a point, text that wraps to a
   different number of lines and overlaps on either side. `WRAP` describes
   the same words and `LINES` describes differing words. If a whole row is
   offset by one card, compare zone order first. `joined.json` explains each
   pairing failure. The matcher skips wrappers that draw nothing. Use
   `boxes --all [prefix]` to check the test windows at their end moments.
3. Run `just b3d match styles <recording> --at <moment>` to see the web and
   Beryl 3D values side by side. It compares colors within one level per
   channel and lengths within a quarter point. Shadows, gradients and filters
   are compared field by field. Change Beryl 3D's `Style` or its design
   token. Run `match gaps <recording> --at <moment>` or `match gaps --all
   [prefix]` for CSS shapes `Style` cannot yet express, grouped by property
   and shape and ranked by screen area. It writes `gaps.json`.
4. Run `just b3d match pixels <recording> --at <moment>` and read the crops
   for the worst ten regions. It counts pixels that differ by more than eight
   levels, excluding glyphs and pixels painted by a region's children.
   Marks pair by outline kind and card object and own only a band around
   their boxes, so the card face keeps its pixels. Use `--only <locator>`
   with `pixels` or `snapshot` to draw one element and its web pair on black.
5. Run `just b3d match motion --all` to rank verbs, effects and marks. It
   prints the most divergent element and a command for measuring the span.
   `just b3d match motion <recording> --from <moment> --to <moment>` measures
   position, size and opacity at 60 Hz and plots the web's solid line and
   Beryl 3D's dashed line. Read the plots to see differences in easing or
   path. Start the span after the view arrives, using `info` for step times.
   Use `--only <locator>` for one element or `--layout phone` for the phone.
   Effect scores use their own `effect` elements and show pixel differences
   where the effect is strongest.

For a paint capability, run `just b3d match samples [prefix]`. It compares
one CSS effect at a time in WebKit and the real Beryl 3D renderer. Both draw
Open Sans and the comparison masks glyph pixels. A sample passes when 99.5%
of pixels differ by at most four levels (`p995 <= 4`). The tool prints the
maximum and mean difference and writes pictures of both sides and their
amplified difference. An unsupported sample names the missing capability.

## Update the comparison records

Run `just b3d match score [prefix]` after changing the look. It holds the
desktop and phone reference targets to the counts in `web-match.json`.
A count above its recorded value fails. A count below its recorded value
also fails until `--update` records the improvement. Read the affected
pictures before updating. Pass `--update --raise` only after trying to keep
the count, and explain the affected targets, counts and reason in the
commit body. `match samples --update` records improved `p995` values and new
samples. Keep supported samples supported.

Files under `data/beryl3d/`:

- `web-match.json` holds target counts for unmatched elements, boxes,
  styles, unsupported styles and pixels, plus each sample's `p995` or
  `unsupported`.
- `match/web/<target>.json` holds the trimmed web element records
  `b3d-web-match` uses without launching a browser. Rebuild them with
  `just b3d match score --refresh-web`. `b3d-web-drift` checks the live
  web client against them.
- `match/census.json` holds the moments for `match census` in milliseconds
  by recording. Use `match census --choose` to select from the check times
  printed by `info --json`. The `added` table keeps hand-picked moments for
  marks and other details those times miss.
- `match/motion-census.json` holds each verb, effect and mark's recording,
  seat and span, or the reason no recording shows it. Regenerate with
  `match motion --all --refresh-census`.
- `match/roles.json` maps CSS selectors to Beryl 3D roles and optional name
  patterns. Use it if an element is unmatched because of its name or role.
  The first matching selector wins, then ARIA role, then tag.
- `match/samples.json` holds each paint example's name, purpose, box, page
  and style, with optional text. Use the CSS values the web client uses.
  Add new style shapes the renderer supports to `match/supported.json` in
  the same change.
- `screens.json` supplies the web route, optional query and initial click
  for a screen target. The default route is `#/game`.

## Compare timelines and coverage

`just b3d timeline-parity [--fixtures] [prefix]` compares script step
boundaries, verbs, cases, effects, sounds and placement. Inspect the Beryl 3D
steps with `just b3d timeline <recording> --seat <seat>`. The web timeline
reader (`ui/scripts/timeline.mjs`) uses the `stage_timings` response that
`just b3d export` writes first. When the web reader cannot time a recording,
every step counts as a difference.

`data/beryl3d/timeline-parity.json` holds the number of differing steps per
recording and seat. `--update` lowers counts and drops retired recordings.
Adding or raising one requires `--raise` and a reason in the commit body.
`just b3d timeline-parity --fixtures --refresh-web` rewrites the committed
JSONL timelines in `data/beryl3d/timeline/web/` from the live web client.
`b3d-timeline` checks against those files and `b3d-timeline-drift` checks
whether the live web client has moved.

`just b3d parity` reports which fixtures, specs, timelines, verbs, effects,
marks, prompt shapes, screens, UI conventions, tools and launch tests the
clients share. Use `--full` to list everything missing or `--json` for
structured output. It runs the desktop and phone interface matrices unless
`--results <dir>` supplies saved `desktop.json` and `phone.json` outputs
from `test --json`.

`data/beryl3d/parity.json` maps web tool jobs to Beryl 3D commands, skill
directories to jobs or `none:` explanations, web screen files to screen names,
and verbs, effects and marks to example recordings. A web screen can be
`missing` or have a `none:` explanation. Run `just b3d parity --check` to
verify the maps without running the interface tests.
