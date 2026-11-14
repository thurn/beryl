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
- `card-printings.csv`: the specific printing chosen for each card.
- `rules-version.json`: the Comprehensive Rules release that every citation in
  the code is checked against.
- `oracle-divergences.json`: test scenarios where an oracle (XMage or Forge)
  gets the rules wrong, each with its rules citation. Never edit it by hand:
  `just card diverge` writes an entry, only when the other oracle agrees with
  Beryl, and the `divergences` check holds each entry to the pinned rules.
- `oracle-exporter-gaps.json`: generated scenarios that an oracle's exporter
  can't express, which are reported as unsupported.
- `card-names-allowlist.json`: generic code files that still mention card names,
  and one-card mechanics still in the engine. Its counts may only go down.
  `python -m beryl_tools.checks card-names --shrink` lowers them after you
  remove some.
- `file-length-allowlist.json`: source files allowed to be over 800 lines.
- `ui-counts.json`: limits on the UI's renders, DOM mutations and style and
  layout work for a few fixtures, held by the nightly's `ui-counts` step.
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
- `ui-conventions.json`: the UI design rules, which `just conventions` turns
  into `docs/ui-conventions.md`. Edit the JSON, never the generated page.

The app downloads card images on demand into its cache (`beryl_app`'s `art.rs`).
No images are committed.
