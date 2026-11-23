# Beryl 3D data

The app embeds runtime data. Tools load it from the checkout or from
`BERYL3D_DATA`. The host passes validated `kit_base::data::Data` to the client.
Run `just b3d data` to validate it, including inspection-only profiles.

- `budgets.json` sets each gated board's worst-frame and total work limits,
  handle allocations, peak draws and fragment limits for both visual tiers. Each
  limit has a `max` and a `why`.
- `frame-calibration.json` records each registered verifier's reference revision
  and runtime for the native and Callgrind frame gates. See
  [Frame calibrations](../../docs/beryl-3d.md#frame-calibrations).
- `levels.json` records the client-core levels, dependency order, source caps
  and remaining protocol and web references. Look for a seam before adding a
  `cap_allowances` entry. Record the current line count and the seams
  considered. `just b3d levels --update` lowers obsolete allowances.
- `corpus.txt` lists the first recording by name for each distinct signature and
  every fixture. `just b3d corpus --update` writes it. The
  [game probe guide](../../rust/beryl3d/crates/game_probe/AGENTS.md) defines the
  signature.
- `screens.json` names screens and prompt shapes with the recording, moment and
  tree line that show them.
- `conventions/` maps entries in `data/ui-conventions.json` to recordings with
  `at` and `shows`, or gives the reason in an `open` entry. A recording can
  specify actions (`do`), seat, layout and whether to let the frame settle
  (`still`). The inspector checks keys and recording names.
- `profiles/` holds window conditions, cameras, container tuning, slots,
  interface thresholds and test windows. `camera-fit.json` holds Arena card
  corner measurements that `just b3d camera-fit` fits to the desktop camera.
- `effects/` defines runtime drawing layers. The shared effect names and
  durations live in `data/ui-conventions.json`.
- `gates.json` holds material shader limits and interface test frame limits.
  `test-faults.json` holds the allowed fault counts by test and rule. A lower
  count also fails until `just b3d test --update` records it.
- `verbs.json`, `catch-up.json` and `slide.json` tune verb motion, queued
  scripts and table regrouping. `pointer.json` and `audit.json` tune input and
  interface checks.
- `card.json` supplies card dimensions and stacking, and `badges.json` supplies
  badge sizes. `light.json` and `own.json` supply lines, marks and control
  symbols drawn into the light atlas. `lighting.json` supplies renderer
  lighting, shadows and bloom.
- `hud.json`, `flat.json`, `notices.json`, `revealed.json` and `shown.json` tune
  presentation. `frame-rate.json` and `tier-probe.json` tune host pacing and
  startup tier selection.

The [web-match skill](../../.agents/skills/web-match/SKILL.md) documents the web
snapshots, timelines and their comparison counts.
