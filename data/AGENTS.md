# The files in `data/`

This adds to the root `AGENTS.md`, whose rules all apply here.

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
- `card-printings.csv`: the specific printing chosen for each card.
- `rules-version.json`: the Comprehensive Rules release that every citation in
  the code is checked against.
- `oracle-divergences.json`: test scenarios where an oracle (XMage or Forge)
  gets the rules wrong, each with its rules citation.
- `oracle-exporter-gaps.json`: generated scenarios that an oracle's exporter
  can't express, which are reported as unsupported.
- `card-names-allowlist.json`: generic code files that still mention card names,
  and one-card mechanics still in the engine. Its counts may only go down.
  `python -m beryl_tools.checks card-names --shrink` lowers them after you
  remove some.
- `file-length-allowlist.json`: source files allowed to be over 800 lines.
- `save-breaks.json`: saved games in the corpus that can't be migrated past a
  particular change.
- `arena-motion.json`: MTG Arena's animation timings for each animation "verb"
  (a reusable animation such as a card moving to a zone), measured from the
  recordings. Beryl's own timings started from these.
- `ui-conventions.json`: the UI design rules, which `just conventions` turns
  into `docs/ui-conventions.md`. Edit the JSON, never the generated page.

The app downloads card images on demand into its cache (`beryl_app`'s `art.rs`).
No images are committed.
