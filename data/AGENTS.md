# The files in `data/`

This adds to the root `AGENTS.md`, whose rules all apply here.

- The [Beryl 3D data guide](beryl3d/AGENTS.md) describes the files in
  `beryl3d/`.
- `audio/manifest.json` selects the sources and variants for Beryl 3D's music
  and sound effects. `just audio` writes the media under `audio/assets/` and the
  exact music loop lengths to `audio/loops.json`.
- `pytest-reads.json` maps each Python module of `tools/` and `efficiency/` to
  the tracked files and directories it reads at run time, so a pytest check's
  input hash covers them (`tools/src/beryl_tools/ignite/pytest_reads.py`). A
  test that reads a file the record lacks fails with the entry to add;
  `uv run --project tools python -m beryl_tools.ignite.python reads` runs every
  test and adds what they read.
- `card-store.tsv` is the card store, built by `just cards` from Scryfall's bulk
  data. It has a record for every card in the pool and every token those cards
  make, sorted so it can be searched by seeking
  (`tools/src/beryl_tools/card_store.py` defines the format). The app embeds it
  (`beryl_card_store`).
- `set-groups.json`, also written by `just cards`, says which crate each card
  file goes in. Each crate covers a fixed range of first-printing sets that
  never changes (`set_groups.py`).
- `catalogs.json`, also written by `just cards`: Scryfall's type and keyword
  catalogs plus the counter kinds the pool uses (`catalogs.py`).
- `card-pool-exclusions.json`: cards left out of the store even though they are
  legal in Vintage.
- `census-coverage.json`: the Forge mechanics (kind names only) that
  `beryl_card` covers, each with its entry, for `just census` (`census.py`).
- `card-escalations.json`: cards the card vocabulary (`beryl_card`) can't
  express yet, by name, each with the mechanic it needs, the rule that mechanic
  falls under and the ability's Oracle text. The `card-writing` skill adds
  entries, and `just card brief` prints a card's entry. An entry with
  `"partial": true` is a card whose file exists but leaves out the ability its
  `text` quotes.
- `card-text-allowlist.json`: card files whose rendered rules text differs from
  their Oracle text, each with the exact word diff the `card-text` check
  (`tools/src/beryl_tools/card_text.py`) allows. Its `reason` is `wording` (the
  definition does what the card says and the renderer has no template for
  Oracle's words) or `escalated` (a clause is left out, and
  `card-escalations.json` has a partial entry for the card). Its `note` says
  why. Entries may only be removed, except a `wording` entry for a new card.
- `rulings-ledger.json`: Scryfall rulings for audited cards, by full card name.
  Each entry has the ruling's `published_at` date and `hash` (the first 12
  lowercase hex digits of SHA-256 of the exact UTF-8 comment), plus exactly one
  of `scenario` (a nonempty list of names from the card file or rules library),
  `covered` (the vocabulary entry or rules-library scenario that covers it), or
  `untestable` (the reason). `just card rulings <name> --add` adds missing
  entries with blank reasons for the writer to fill.
- `phase-data.json` pins the [Phase](https://github.com/phase-rs/phase)
  card-data release by its published `card_data_hash` and the SHA-256 of the
  JSON bytes. `just card brief` and `just card new` use Phase's typed parse as a
  drafting source. The MIT/Apache-2.0 project publishes the JSON at
  `https://data.phase-rs.dev/card-data.json`. Beryl stores it in
  `$BERYL_CACHE/phase/card-data.json` and verifies it against the pin when
  loading it. Phase publishes its current coverage report at
  `https://data.phase-rs.dev/coverage-data.json` and replaces it at each
  release. Beryl keeps the pinned release's original report in
  `phase-coverage-<card_data_hash>.json.gz`. `just set plan` verifies the
  uncompressed bytes against `coverage_sha256` in the pin before caching the
  report in the same directory and using it to identify cards with parse gaps.
- `card-printings.csv`: the specific printing chosen for each card.
- `rules-version.json`: the Comprehensive Rules release that every citation in
  the code is checked against.
- `rule-ownership-allowlist.json`: kernel and rules source files still waiting
  for an `Implements:` header of their own or in their parent module. Each
  module pass removes its files from the list.
- `oracle-divergences.json`: test scenarios where an oracle (XMage or Forge)
  gets the rules wrong, each with its rules citation. Never edit it by hand:
  `just card diverge` writes an entry, only when the other oracle agrees with
  Beryl, and the `divergences` check holds each entry to the pinned rules.
- `oracle-candidates.json`: reviewed upstream test fixtures and their card and
  scenario requirements, reported by `by oracle-candidates`. Each source names
  its pinned test and supporting files. An adaptation names its Beryl scenario
  and the assertions that carry the original test's observation.
- `oracle-exporter-gaps.json`: generated scenarios that an oracle's exporter
  can't express, which are reported as unsupported.
- `card-leaves-allowlist.json`: the crates that may depend on `beryl_cards` or a
  set crate outside `[dev-dependencies]` (`card_leaves.py`). It may only shrink;
  the check fails on an entry that no longer needs it.
- `card-names-allowlist.json`: generic code files that still mention card names,
  and one-card mechanics still in the engine. Its counts may only go down.
  `python -m beryl_tools.card_names --shrink` (from `tools/`) lowers them after
  you remove some.
- `file-length-allowlist.json`: source files allowed to be over 800 lines.
- `ui-counts.json`: limits on the UI's renders, DOM mutations and style and
  layout work for a few fixtures, held by the `ui-counts` check family.
- `save-breaks.json`: saved games in the corpus that can't be migrated past a
  particular change.
- `arena-motion.json`: MTG Arena's animation timings for each animation "verb"
  (a reusable animation such as a card moving to a zone), measured from the
  recordings. Beryl's own timings started from these.
- `table-commands.json`: the manual table's command list, context menus and
  default shortcuts, transcribed from the Cockatrice appendix. `key_aliases`
  adds alternative keys for the same command. `also_applies_to` gives commands
  additional targets with the same menu placement (`true`), a different
  placement (an object) or no menu placement (`false`). `when` names a condition
  the UI supplies, such as whether the card is two-faced. An `action` is a
  `beryl_table::Action` template, and a `client` names a UI operation. The
  dispatcher fills placeholders from the command, current target and prompt
  answers. `text_and_number` asks for a name, then an amount. `$in` is the zone
  where the selected objects were seen, `$owner` is their owner's seat and
  `$seat` is the affected seat. `$objects` follows the command: a card action
  uses the selection, a whole-pile action uses the pile, and a library action
  uses the requested part. `Primary` is the unlabeled group before "Move to".
  The Rust test in `beryl_table/tests/commands.rs` checks the shape of each
  action.
- `table-appendix-map.json` records how the Cockatrice appendix rows map to
  manual-table commands and gestures. Rows with several controls can list
  several targets. Zone keys join the zone and Cockatrice label with `/`.
  Protocol keys start with `Protocol / `. Leave excluded rows out of the map.
- `design-tokens.json` supplies colors, lengths, numbers, fonts and the flat
  layer's spacing, sizes and text styles. The `kit_base` build script derives
  its `tokens` module from the file.
- `ui-conventions.json`: the UI design rules, which `just conventions` turns
  into `docs/ui-conventions.md`. Edit the JSON, never the generated page.

## The efficiency dashboard's files

The files named `data/efficiency-*.json` are read only by the efficiency
dashboard in `efficiency/` (`docs/efficiency-dashboard.md`). A change to one
reruns only the efficiency project's checks and the repository checks.

- `efficiency-changes.json`: each change made to save time or tokens, with a
  one-sentence `label`, the `metric` it was meant to move (a name that
  `just efficiency metrics` lists, optionally with a cohort filter such as
  `bead.dollars[tier=sonnet]`; `bead` is the dashboard's name for a ticket) and
  `expect`, `up` or `down`. Add the entry in the commit that makes the change:
  that commit is found from the file's history, so the entry needs no hash. An
  entry for a change made before the file existed names it in `commit`, a full
  hash on `staging`. `just efficiency changes` shows each metric's median over
  the seven days before and after the commit reached `staging`.
- `efficiency-command-kinds.json`: the ordered table that gives each shell
  command an agent ran a kind, such as `rust tests` or `push`, for the time
  split. Each entry has a unique `kind`, a Python regular expression `pattern`
  and `heavy`, true for commands that take real time or CPU. The first entry
  whose pattern matches wins. Patterns are matched against each part of a
  command chained with `&&`, `||` or `;`, after a leading `cd <path>`,
  environment assignments and wrappers such as `timeout 600` are removed, so
  anchor them with `^`. When `just efficiency coverage` shows that commands of
  no kind are 5% or more of the week's tool time, add an entry for the longest
  ones it lists.
- `efficiency-prices.json`: dated tables of each model's price in US dollars per
  million tokens for uncached input, 5-minute cache writes (`cache_write`),
  1-hour cache writes (`cache_write_1h`), cache reads and output, each with the
  date it was copied (`as_of`) and the address of the provider's price page
  (`source`). Copy prices from that page, never from memory. When a price
  changes, add a new table and keep the old one: a request is priced by the
  latest table dated on or before its day (the earliest table also prices older
  requests). Codex's models (`gpt-*`) are in the same table, from OpenAI's page
  named in its `note`. `above` holds a model's prices for a prompt over `tokens`
  tokens. A model missing here has no dollars, and `just efficiency coverage`
  lists it.

## Card images

The app downloads card images on demand and caches them through `beryl_art`.

## Native client assets

`audio/manifest.json` and `audio/loops.json` describe the files under
`audio/assets/`. `just audio` regenerates those files from the staged source
packs. See [Beryl 3D data](beryl3d/AGENTS.md) for game fixtures, layout profiles
and motion tuning. No images are committed.
