# Bug history

Rules engine bugs identified and fixed, kept to understand how to mitigate
similar classes of bugs in future (e.g. via the type system). AI, UI and tooling
bugs do not belong here. Each entry gives a short description of the bug, how a
similar bug could be prevented, and the fixing commit when it was not found and
fixed in the same session. Newest first.

- **Haste and summoning sickness were read differently by the mana paths.** A
  creature with haste that came under its controller's control this turn could
  tap for an unrestricted mana ability, but not for a restricted one such as
  "Spend this mana only to cast a creature spell", and a summoning-sick creature
  without haste was offered its "Activate only as an instant" {T} mana ability
  at priority (CR 302.6, CR 702.10c). `mana::sources::source` made the
  summoning-sickness test with the haste exception, `can_tap` (read for
  restricted mana) made it without, and `instant_options` tested only whether
  the permanent was tapped. Fixed: all three ask `can_tap`, which now has the
  haste exception. Guard: the one query,
  `a_hasty_creature_taps_for_restricted_mana_the_turn_it_arrives` in
  `beryl_rules/src/tests/remembered_mana.rs` and
  `a_summoning_sick_creature_offers_no_instant_mana_ability` in
  `beryl_rules/src/tests/payment.rs`. A review found a fourth reader: the
  automatic payment's estimate of the costs a player may still pay this turn
  (`mana::lookahead`) counted a summoning-sick creature's {T} ability, so a
  payment kept mana open for an ability that could not be activated, and it
  skipped every ability of a tapped permanent, including those without {T}. Both
  estimates now read the same list, `lookahead::activations`, which asks
  `can_tap`. Guard:
  `a_payment_keeps_mana_only_for_a_tap_ability_that_can_be_activated` in
  `beryl_rules/src/tests/payment.rs`.

- **A search for any card let the player find nothing.** Searching a library
  with `Filter::Any` (no stated quality) offered `Opt::Done` on every pick, even
  while cards remained in the library, although CR 701.23b allows finding fewer
  cards only when searching a hidden zone for cards with a stated quality.
  `actions::search::search_library` and `search_with_casts` always called
  `choices::pick_up_to` and `choices::search_pick`, both of which always offered
  `Opt::Done`. `choices::pick_up_to` now takes a minimum count and asks a
  mandatory `choices::pick` while fewer than that minimum have been found, and
  `choices::search_pick` omits `Opt::Done` when a search with `Filter::Any` has
  matching cards left. Guard: the Demonic Tutor scenarios (a non-empty library
  offers only its cards; an empty library finds nothing),
  `search_for_any_card_cannot_end_early_while_cards_remain`, and the mutation
  check on Sylvan Scrying and Spellseeker (whose `Filter::Any` mutants are no
  longer in `ORACLE_BLIND`).

- **Automatic mana payment chose which copy of a source to tap by slot.** When a
  payment needed one of two copies of the same land or mana creature, the rules
  tapped the copy in the lower slot. Slots are assigned by the deal, so the same
  position could tap the enchanted copy in one game and the plain one in
  another, or tap a creature that could otherwise have attacked.
  `State::mana_payment_sources` now takes copies of one definition in the order
  they entered the battlefield, and it was the last walk in slot order that
  reached a mutation from outside the kernel. Guard: `Bitboard::iter` is private
  to the kernel, so a rules module can only walk a set in an order the state
  names; `a_payment_taps_copies_in_battlefield_order` covers this query.

- **A save with a wished card could not replay.** When Burning Wish brought a
  sorcery in from outside the game, the save recorded the card's name as a
  `Reveal` entry, as it does for every hidden card a player first learns. On
  replay, `Entry::random` (`beryl_session/src/replay.rs`) looked for that name
  only in the owner's main deck, so the save failed with "player 0's deck has no
  Relentless Assault to reveal". No save had hit it, because session games had
  no sideboards until the same change. The lookup now searches the owner's
  sideboard after their main deck. Guard: the Burning Wish and Lurrus of the
  Dream-Den games in `beryl_session/tests/card_shapes.rs` cut a save after the
  card is brought in and replay it under each perturbation.

- **Lurrus's graveyard permission worked on the opponent's turn.** "During each
  of your turns, you may cast one permanent spell with mana value 2 or less from
  your graveyard" let its controller cast a flash creature from their graveyard
  on the opponent's turn, which also used up that turn's permission.
  `casting/graveyard.rs` checked who controlled the permanent with the
  permission and whether it was used this turn, but never whose turn it was. The
  checks only showed up for a permanent spell with flash, and no scenario had
  one. `permitter` and `sources` now require the player to be the active player.
  Guard: the test
  `the_graveyard_permission_works_only_during_its_controllers_turn`. No check
  can tell that a permission's Oracle words name a turn its code ignores; a test
  with a flash card covers the timing words of a casting permission.

- **Two different game histories hashed alike.** The hash of a `State` added the
  turns played before a restart and the turns played in ended subgames only when
  each was nonzero, with nothing to say which was which. So a game five turns
  past a Karn Liberated restart and the same position five turns after a
  Shahrazad subgame had the same hash, although the turn limit
  (`State::turns_for_limit`) counts subgame turns and not turns before a
  restart. Each of these fields, and the suspended game, now hashes after a tag
  of its own, and an ordinary game's hash is unchanged. Guard: the tests in
  `beryl_kernel/src/subgame/tests.rs`. No check can tell that two optional
  fields share a hash prefix; a field added to the hash later needs a tag too.

- **An Aura that became a creature stayed attached.** An effect that made an
  Aura a creature left it on its permanent, although "an Aura that's also a
  creature can't enchant anything" (CR 303.4d). `attach::may_enchant` quoted the
  rule but checked only that the Aura was not enchanting itself, and the
  state-based action that puts an illegally attached Aura into the graveyard
  asks `may_enchant`. Both `may_enchant` and `may_enchant_player` now refuse an
  Aura that is a creature. Guard: the test
  `an_aura_that_becomes_a_creature_goes_to_the_graveyard`. No mechanical check
  can tell a quoted clause from an implemented one; the review of the module
  passes found it while declaring the rules `actions/attach.rs` implements.

- **A permanent an Aura put onto the battlefield stopped being linked to it when
  the Aura's card returned.** `Memo::PutOntoBattlefieldWith`, which Animate
  Dead's "enchant creature put onto the battlefield with this Aura" reads, named
  only the Aura's slot. The zone move ended the link when a new object arrived
  in that slot, so an ability of the old Aura still on the stack lost "that
  creature"; before the move, the Aura's card in the graveyard read as the same
  object. No implemented card reaches either case: Animate Dead's own leave
  trigger carries the creature. The memo now holds a `Link`, the slot and
  incarnation byte that `Memo::ExiledWith` already used, and `State::linked`
  takes an `ObjectRef`, so the type stops a caller from asking by slot alone.
  Guard: the type, and the test
  `a_permanent_put_onto_the_battlefield_with_an_object_is_not_linked_to_a_new_object`.

- **An ability lost the cards its source exiled once a new object entered the
  source's slot.** If Knowledge Pool left the battlefield and returned while one
  of its abilities was on the stack, that ability found none of the cards the
  old Pool had exiled. The same happened to a waiting ability of a token that
  had ceased to exist. `Memo::ExiledWith` named only the slot of the exiling
  object, so the zone move dropped every link to the slot when a new object
  arrived there, or else the new object would have read the old object's cards
  as its own. The memo now holds a `Link`: the slot and the low byte of the
  exiling object's incarnation. `State::linked` takes an `ObjectRef`, so the
  type stops any caller from asking by slot alone. Kernel and rules tests cover
  an ability whose source left and returned, a token that ceased to exist, and
  the 256th object in a slot.

- **The next-stop prediction could skip an extra combat.** The prediction
  excluded a creature with vigilance while it was still attacking in the current
  combat. It now checks whether the creature can attack after its current combat
  role expires. `a_vigilant_attacker_makes_each_combat_a_distinct_smart_stop`
  checks both combat occurrences.

- **An effect that moved a merged permanent lost track of the cards under it.**
  When Flickerwisp exiled a mutated creature, its "return that card" ability
  found only the permanent's own card, so the cards that had been under it
  stayed in exile. The rules say such an effect finds every card the permanent
  became (CR 731.3c, and CR 713.8c for melded permanents). The instructions that
  move permanents now add the cards under them to what they moved
  (`Run::with_piles`), and cards exiled together remember it, so a delayed
  ability that carries one returns all. Guard: Gisela, the Broken Blade's
  Flickerwisp scenario, which Forge plays the same way.

- **Flipping a permanent could make it appear attached to a player.** The flip
  flag overlapped the bits that record which player an Aura enchants. The flag
  now uses a separate bit, and a compile-time assertion checks that the two
  fields remain disjoint.

- **Narrowing a `Large` game into `Small` could panic instead of refusing it.**
  `State::try_relayout` promises a `Misfit` for a game the target layout cannot
  hold, but `State::fits` checked only the lists it named. It missed the steps
  left in the turn (extra steps and phases can push them past `Small`'s 24), the
  effects that reach other zones (four in `Small`) and each player's mana that
  remembers its source (four in `Small`). A game past one of those passed the
  check, and the copy that followed panicked on the full queue. `fits` now
  checks all three. Prevention: `fits` destructures every field of `State`, as
  `Layout::clone_state`, `State::hash_position` and `Relayout for State` already
  did, so a new field does not compile until it says whether it fits.
  `a_turn_past_small_does_not_narrow` in `beryl_search` covers the turn plan.

- **A restarted game could deal a different mulligan hand after its slots were
  renumbered.** The shuffle took the returned cards in slot order. Restart
  mulligans now shuffle from the same card definition order used to return the
  hand. The generated Karn Liberated scenarios compare the hands and bottom
  picks across slot permutations.
  [Fix](https://github.com/thurn/beryl/commit/0687a4112dbabc7edbc587bf46307499a4777f95).

- **Pithing Needle omitted the back name of a double-faced card.** The name
  catalog added a second name only for split cards. It now also records the back
  names of modal and transforming double-faced cards. A Pithing Needle scenario
  chooses Glasspool Mimic and Glasspool Shore in the same game.

- **Linked exile could leave a spell's entry on the stack.** Linked exile moved
  a card directly to exile without removing its spell. It now uses the same
  stack removal as other exile effects. The Knowledge Pool scenarios check that
  the exiled spell is gone and only the offered spell resolves.

- **A spell could use the same artifact for mana and for its sacrifice cost.**
  Casting checked the two costs independently, then offered a payment that spent
  all the available artifacts as mana sources. The casting check and mana
  options now test whether the rest of the cost is payable after consuming those
  sources. The Shrapnel Blast scenario checks that two Treasures cannot pay both
  costs, and that adding a land makes the cast possible.

- **A spell copy could target a creature without triggering its ability.**
  Copying a spell put its targets on the stack without announcing them to
  trigger listeners. Copies now use the same target announcement as spells that
  are cast. A scenario where Fork copies a spell targeting Bonecrusher Giant
  checks that the copy triggers the Giant again.

- **A front half of a split spell could show both colors.** The card store used
  the combined color field when Scryfall did not list a half's colors, and
  object queries read only one face off the stack. The store now derives missing
  split face colors from that face's mana cost. The Fire // Ice scenarios check
  that Fire is red on the stack and that both colors exist when the card is in a
  graveyard.

- **A subgame could offer the suspended game's sideboard.** Both games use the
  same card table. Sideboard choices now require the main game, while cards
  already in its library can enter a subgame from their pool slots.
  `a_subgame_carries_library_cards_from_the_pool_and_offers_no_wish` checks
  library transfer, determinization and wish resolution in the subgame.

- **A split card in a library reported only the mana value of its first half.**
  Cascade could stop at a split card whose combined mana value was too high. The
  mana-value query now reads both halves outside the stack.
  `cascade_uses_combined_split_cost_and_offers_each_half` checks the value of
  the card in exile and of the chosen half on the stack.

- **A compact enum tag changed the benchmark's position hashes.** Removing the
  old cast offer preserved the numbers of the remaining `Pending` variants. The
  new representation also changed the tag that Rust's derived hash wrote from a
  machine word to one byte. The benchmark pins then failed even though play was
  the same. `Pending` now hashes the original word-sized tags explicitly.
  `corpus_never_changes` and `rollouts_never_change` guard the hash of the
  benchmark's positions and results.

- **A creature blocking several attackers assigned all its damage to the first
  one.** The assignment step only asked about attacking creatures. It now asks
  the blocker's controller and keeps each pick with its source.
  `a_blocker_of_two_attacks_divides_for_its_controller` checks which player
  chooses and which attacker survives.

- **Returning a card transformed could put the wrong face on the battlefield.**
  The rule used the number of printed faces to identify double-faced cards. It
  now reads the generated card shape and checks that the back face can be on the
  battlefield. The regression
  `returning_transformed_keeps_a_flip_card_and_an_instant_back_face_in_exile`
  covers a flip card and a modal card whose back face is an instant.

- **A resolution that ran out of capacity could leave events from its scratch
  run.** The engine now truncates those events and restores the event scopes
  when it discards the run. This applies to costs paid while casting a spell
  during the resolution, including a mana ability that asks before the cast
  finishes. The replay tests check that only a completed run publishes its
  events.

- **Removing the final phase could still start its cleanup step.** An empty turn
  plan used cleanup as a fallback. It now advances to the next turn.
  `skipping_the_ending_phase_advances_to_the_next_turn` checks the turn number
  and its step events.

- **Advancing one step could pass through a repeated cleanup.** The engine
  compared the turn number and step name, which are the same in both cleanups.
  It now uses a position within the turn. The regression
  `an_ordinary_turn_distinguishes_repeated_cleanup_steps` covers the boundary,
  and the turn plan test checks that advancement stops for both upkeeps.

- **Checking whether a spell could pay its sacrifice or discard costs allocated
  on the heap.** The payability and choice checks built temporary vectors of
  ordered cost parts and their eligible objects. They now use the layout's
  queues, which store entries inline in the compact engine and return `Full`
  when the game needs to widen. The search allocation check plays fuzz decks
  that exercise this path and requires zero allocations.

- **Widening during a mandatory loop could delay the draw.** The loop detector
  stored a hash of the position using the current layout's storage, so it could
  miss a checkpoint made in the other layout. Both layouts now hash the position
  in `Large`. The regression `widening_preserves_a_mandatory_loop_checkpoint`
  checks that a widening game finds the draw on the same resolution, and the
  scenario and fuzz comparisons check the loop detector along with the rest of
  the state.

- **A draw could take a token or a spell copy into hand.** During a resolution,
  either object could sit above the top card until state-based actions ran. The
  draw used the last library slot, and dredge counted every slot toward its
  minimum. Draws, dredge and scry now use the same ordered card query. The rules
  report the slot the draw moved. The regressions
  `top_cards_and_draws_skip_objects_waiting_to_cease_to_exist` and
  `a_library_token_cannot_make_a_draw_eligible_for_dredge` cover draw events,
  empty libraries and the dredge threshold.
  [Fix](https://github.com/thurn/beryl/commit/15115883e5fddb1fc0342bfaf6016291982da3ef).

- **A permanent could phase in with damage from a previous turn.** Cleanup
  cleared damage from the battlefield list, which skips phased-out permanents.
  Cleanup now also clears damage from the phased-out set. The regression
  `cleanup_removes_damage_from_a_phased_out_permanent` checks that a damaged
  creature phases in with no marked damage after the previous turn's cleanup.

- **Quicksilver Gargantuan's copy of an animated artifact was reported as 7/7
  after Oko animated the copy.** The current engine applies the copy exception
  in layer 1, then applies Oko's 3/3 in layer 7b. The scenario
  `Oko makes the Gargantuan's artifact copy a 3/3` checks that the copy enters
  as a noncreature Portable Hole and becomes a 3/3 Elk after Oko's ability
  resolves.

- **Players could not activate abilities from the graveyard or exile.** The
  engine gathered candidates from the battlefield and hand, and the card table
  rejected activated abilities that declared any zone other than the
  battlefield. The engine now uses each ability's declared zones and looks for
  cards in the graveyard and exile when the game has abilities that work there.
  The Mister Immortal scenarios verify that its owner can return it tapped from
  each zone, and the Pithing Needle scenarios verify that naming it removes both
  activations.

- **Widening a game during Show and Tell could lose the nonactive player's hand
  pick.** The resolution record held the card's slot index from the smaller
  layout, which names a different slot in the larger layout. Relayout now
  converts hidden picks by the card's place in its owner's deck. The Show and
  Tell layout regression widens the game after both hand picks and compares the
  remaining choices and committed events.

- **Show and Tell asked the nonactive player where to attach their Aura first.**
  The engine gathered entry choices in the same card-name order it used to
  record the moves. It now gathers all entry choices in turn order before moving
  the cards together. The scenario
  `Show and Tell chooses both Auras before either attachment` checks the order
  of the hand picks and attachment choices, and checks that both Auras choose
  from the battlefield as it was before either entered.

- **Fork could not choose itself as the new target of a copied Counterspell.**
  Target selection looked at stack entries, and the engine had already set
  Fork's entry aside to run its effects. Spell targets now come from the stack
  zone, where the resolving card stays until it moves. Fork's regression
  scenario chooses Fork, checks that the copy leaves the stack by itself, and
  checks that the original Counterspell still counters Lightning Bolt.

- **Containment Priest exiled Clever Impersonator before it could copy an
  artifact.** The engine decided whether to redirect the move using the
  Impersonator's original type line. It now asks for the copy first and checks
  the remaining entry replacements against the resulting permanent. If a
  replacement redirects the move, the chosen copiable values are discarded.
  Containment Priest's scenarios check the copy choice and final zone when the
  Impersonator copies an artifact, copies a creature, or declines to copy.

- **Circle of Protection: Red skipped Lightning Bolt in a graveyard while
  Snapcaster Mage's trigger targeted it.** The choice looked only at permanents
  and spells, and a shield for a permanent spell lost track of its source as the
  spell resolved. The Circle now offers objects referred to by pending spells,
  abilities and effects when those references still name the current object. A
  shield follows its chosen permanent spell onto the battlefield. The Circle's
  Snapcaster Mage and Raging Goblin scenarios check the offered sources and the
  damage prevented after the spell resolves.

- **Quicksilver Gargantuan became 2/3 when it copied a 2/3 Tarmogoyf.** The 7/7
  copy exception set the copy's power and toughness. The copied
  characteristic-defining ability then overwrote both values. The exception now
  omits the ability from the copy's copiable values. The regression
  `a_power_toughness_copy_exception_omits_the_defining_ability_from_later_copies`
  checks that the Gargantuan and a Glasspool Mimic copying it are both 7/7 and
  have no copy of the ability.

- **A spell could not escape using the card sacrificed or discarded to cast
  it.** The cast check counted graveyard cards before paying any costs. The
  casting steps now pay sacrifices and discards first and check what actually
  reached the graveyard before offering the cast or a payment choice. The
  Underworld Breach scenarios verify that both kinds of cost can supply the
  third card before an opponent receives priority. They also cover tokens and
  sacrifice moves that Rest in Peace replaces with exile.

- **Phantasmal Image became an Illusion when it copied a land animated by Life
  and Limb.** The copy effect added a creature subtype to a land before the
  animation made it a creature. Copy exceptions now check the card types the
  copy has in layer 1. The scenario
  `an Image of an animated Forest gains no Illusion subtype` checks the copy
  while Life and Limb animates it.

- **A game could panic when Equipment became unattached as a creature took
  lethal damage.** The state-based check detached the Equipment before it
  classified lethal damage for regeneration. Detaching had invalidated the
  cached characteristics and could change toughness. The check now classifies
  lethal damage before applying any of its actions. The regression
  `regeneration_uses_toughness_before_equipment_detaches` checks that a creature
  regenerates when the damage is lethal at the start of the check and its
  toughness rises after detachment. The debug assertion on derived reads still
  catches attempts to use invalidated characteristics.

- **Beryl 3D drew no arrows, and attackers wore a dull salmon smear instead of a
  chevron.** The scene listed every target arrow, source line and line of combat
  (`just b3d tree` showed them), but `png` drew none. `light::arrows` wrote each
  ribbon and head as `Draw::glow(..., 1.0)`, and the glow material fades a glow
  out as its `progress` runs to 1, so every arrow was fully faded from the
  commit that added it (f558ef039). Separately, the `WEDGE` sprite was a 60°
  slice of a disc, which squashed into the chevron's thin quad came out as a
  flat trapezoid with no rim or halo, and the opponent's attackers sank out of
  sight: a battlefield row's plane rises from the near edge of an untapped card,
  the opponent's attacker moves toward the viewer, and both the card's foot and
  its mark (which lies past that edge, in the card's plane) went under the
  cloth. Fixed: arrows write `progress` 0; `WEDGE` and a new `WEDGE_HOT` draw
  the web's chevron from `glow.ts` in its glow and core colors; an attacker
  keeps its row height as it advances, and a fight mark past a card's near edge
  folds up to lie level. Prevention: the scene tests checked that an arrow
  element existed, never that its records gave light, and the text tools cannot
  see a record's `progress`. Guard: `each_case_of_point_plays_to_its_placement`
  now requires an arrow's glow records to have opacity and `progress` below 1. A
  renderer check that compares drawn pixels with the tree's elements would catch
  the rest of this class.

- **Every hub write failed with `database is locked` for minutes.** Claims,
  heartbeats and results were refused, so `ignite start` answered "Claim
  refused: no live reservation" and the pool froze. `POST /next` reserves a
  ticket inside an `IMMEDIATE` transaction, which holds the database's write
  lock, and in it `rules::backlog::ratios` summed last week's dollars with
  `json_extract` over the `metrics` table, which had grown to gigabytes. While
  rule evaluations read the same table, that scan took minutes. Hotfix 2ca7543c5
  ran one evaluation at a time; now `rules::backlog::refresh` takes last week's
  use during the hourly rule evaluation and keeps it in `meta`, so the dispatch
  transaction reads one row instead (d8cb347ea). Guards:
  `a_hand_off_reads_no_metrics_rows` renames `metrics` away and requires
  `POST /next` to still reserve a ticket, and
  `rule_evaluations_run_one_at_a_time_and_not_again_within_the_retry` covers the
  evaluation gate. Prevention in general: a write transaction reads only rows
  found through an index, and a read of a table that grows without bound is done
  outside it.
  [Fix](https://github.com/thurn/beryl/commit/d8cb347ea9d0676a817113b938a2872e74f24c18).

- **A face-down spell could expose the card that was cast.** A stack entry
  normally keeps the spell's card index. Face-down spells now record only their
  public characteristics.
  `morphs_hide_the_card_on_the_stack_and_in_saved_targets` checks that redealing
  preserves the visible stack and that a saved target identifies the face-down
  permanent by battlefield order.

- **Redealing a face-down copy could corrupt its copied values.** The hidden
  card was dealt into the field that holds the copied definition. Search now
  deals it into the memo that keeps the original card. The regression
  `redealing_the_card_under_a_face_down_copy_preserves_the_copy` checks that the
  original card can vary and the copy effect remains consistent.

- **Entering as a copy of a back face could panic.** The Aura attachment check
  looked up the copied face on the original card. It now reads the selected copy
  and applies its values in the preview used to choose a legal attachment.
  `a_copy_of_a_back_face_aura_enters_attached` covers a single-faced card
  entering as an Aura's back face and attaching to a land.

- **A draw replacement could read stale abilities during resolution.** The draw
  code consulted the battlefield after a layer input changed without refreshing
  the layer results. The replacement lookup now refreshes them before reading
  abilities or controllers. `a_draw_replacement_reads_the_current_controller`
  tests a draw immediately after a control change.

- **Seventeen damage prevention effects could stall a game.** The damage
  replacement code used the same fixed lists in both layouts, so retrying the
  event in `Large` still ran out of room. The candidate list and the list of
  effects already applied now grow in `Large`.
  `damage_uses_more_than_sixteen_shields_in_large` checks the retry and the
  amount of damage that gets through.

- **Scry could offer a token as a library card.** A token put into a library can
  still be there until state-based actions run. The top-card query now skips
  tokens and spell copies, and library reordering requires a `CardIn<Library>`.
  `top_cards_and_draws_skip_objects_waiting_to_cease_to_exist` covers the
  interval before the token disappears.

- **A spell could still affect a player who gained protection after it was
  cast.** Target selection checked protection from everything, but resolution
  only checked the target's player filter. Resolution now checks protection
  again. The Teferi's Protection scenario with Tendrils of Agony checks that
  gaining protection invalidates the spell's only target, so even the caster's
  life gain is skipped.

- **The fifth basic land text change could be dropped.** The text map stored
  only four subtype replacements, so a fifth could leave a printed Island
  reading Forest when it should read Swamp. `Words` now maps all five basic land
  types and applies each substitution without a capacity check.
  `five_land_word_changes_compose_in_live_and_captured_text` checks the live
  type line and the words abilities capture before and after the fifth change.
  [Fix](https://github.com/thurn/beryl/commit/a309fcfa5ec7f56c9db7e6f774d9a4e3ead7fb68).

- **Considering a payment could overflow a full mana pool.** The solver narrowed
  mana left over from tapped sources and added it to a byte without checking the
  sum. It now caps both the remainder and the sum at the pool's byte limit. The
  cast lint and
  `offering_payments_with_a_full_pool_does_not_wrap_remaining_mana` cover the
  conversion and the full-pool case.

- **Layer dependencies past the 256th effect could apply the wrong effect.** The
  pending list stored each effect's position in a byte. It now uses full slice
  indices, and the loop check sizes its visited set to the segment.
  `large_layer_orders_dependencies_above_the_byte_limit` covers a dependency on
  an effect beyond that boundary.

- **A permanent with 200 +1/+1 counters could shrink.** Layer 7c read each
  unsigned counter total as a signed byte before computing the difference. It
  adds the full signed difference and clamps the resulting power and toughness
  to their representation. The cast lint blocks the unchecked cast, and
  `large_counter_totals_saturate_power_and_toughness_with_the_right_sign` checks
  both signs.

- **Paying a cost with cards in a large graveyard could revisit earlier cards.**
  The casting cursor was a byte and wrapped after the 255th card. Casting now
  checks the conversion and reads the position from the chosen set when the
  cached cursor is full. The cast lint catches unchecked narrowing, and
  `graveyard_cursor_reaches_above_the_byte_limit` covers the fallback.

- **A nontoken copying Jace, Telepath Unbound had mana value 2.**
  `State::mana_value` used token status to recognize copied back faces (CR
  712.6d). The query now checks whether the effective face comes from a copy
  effect or from the permanent's own top component.
  `a_card_copying_a_transformed_back_face_has_mana_value_zero` compares the
  original and the nontoken copy and checks that merging a card underneath
  either keeps its mana value.

- **A granted ability could define both power and toughness.** The card table
  refused granted abilities that define power alone, but accepted the variant
  that defines both values. The layer reader requires an ability from the card's
  own face for those amounts. The table now rejects both variants, and
  `a_granted_ability_cannot_define_power_and_toughness` checks that it rejects a
  card definition with that grant.

- **Querying zone membership for merged components panicked.** The zone query
  indexed the owner's zone table with `Zone::Merged`, which is an internal
  location for a component of a permanent. The query now returns an empty set.
  `merged_components_have_no_zone_membership` covers the query, and the
  exhaustive zone match requires new locations to choose how membership works.

- **A permanent cast for impending kept its creature types while it was an
  enchantment.** The layer code removed the Creature card type without checking
  which subtypes the remaining card types could have. Both layer paths now
  filter subtypes after removing a card type, as they do when setting the card
  types. `impending_counters_and_type_changes_apply_in_layer_four` checks that
  the creature subtype goes away and comes back, and that the enchantment
  subtype stays.

- **Worldgorger Dragon exiled its controller's permanents in slot order.**
  `linked::exile_instruction` emitted a zone change for each permanent. It
  visited the set by slot number, which changed which land the player saw leave
  first when the same position used different slot numbers. Linked exile now
  follows battlefield order. The three-numbering scenario
  `linked_exile_reports_permanents_in_battlefield_order` checks the actual
  events. Making `State::in_candidate_order` kernel-private also prevents a
  rules function from using this traversal for a batch of mutations.

- **Some batches inherited slot order at another visible step.** Spell removal
  reported stack positions in slot order, and top-or-bottom effects asked about
  cards in slot order. These paths now use stack order and game order. The same
  traversal guard applies to them.

- **Payment substitution could treat a restricted mana source as spare.** The
  payability check ruled out unrestricted mana sources before choosing which
  objects could pay generic mana in their place. A restricted-only source
  counted as spare even when it could make colored mana for that spell. Which
  candidates were excluded from the mana check then depended on slot order.
  `Funds::sources_for` now includes the sources whose restrictions accept the
  spell, and the substitute check needs only the number of spare objects. The
  restricted-mana fixture checks both an eligible spell and an unrestricted
  payment.

- **Mutating over a face-down creature left the merged permanent unknown to the
  opponent.** The nightly fuzz panicked with "a public card unknown": a player
  cast Gemrazer for its mutate cost over a creature that Abhorrent Oculus had
  manifested, and put Gemrazer on top. The merged permanent stays the object in
  the face-down card's slot (CR 731.2c), and a copy effect gives it Gemrazer's
  face-up values. Knowledge is tracked per slot, though, and only the controller
  knew that slot, so the face-up Gemrazer on the battlefield was hidden from its
  opponent. Revealing the slot alone would have been wrong in the other
  direction: the manifested card underneath stays a face-down component, known
  only to its controller, until it leaves the battlefield (CR 731.2e, CR 708.9).
  Fixed: `State::merge` records who knew the face-down card in a new memo,
  `Memo::FaceDownUnder`, and then reveals the slot to every player.
  `State::knows_card_really_is` answers who knows the card behind it, the AI's
  dealt worlds (`Observation::determinize`) deal that card at random for a
  player who doesn't know it, and the session's view draws it face down with no
  name for that player. A Gemrazer scenario over a manifested card and tests in
  `beryl_cards` (`knowledge.rs`) and `beryl_session` (`object_states.rs`) hold
  it. Prevention: a slot can now carry two identities with different audiences,
  the permanent and the card behind it. Any reader of the card behind a slot
  (`State::really`, `State::card_really_is`) on behalf of a player must ask
  `knows_card_really_is`, not `knows`. No type enforces this yet, but the
  invariant check and the determinization tests catch the first misuse in a
  scenario.

- **Delve could exile the cards an escape cost needed.** The nightly fuzz
  panicked with "enough cards to pay the cost" in `casting::parts::choices`:
  Gurmag Angler, cast with the escape that Underworld Breach grants, reached the
  step that asks for escape's "exile three other cards from your graveyard" with
  an empty graveyard. A spell's delve exiles its cards while the mana is paid,
  before the spell's other cost parts, and both the legality check
  (`casting::delve::delve_pays`) and the delve step (`pay_substitutes`) let
  delve take every card in the graveyard. `spell_payable` counted the same cards
  again for escape, so a spell was offered whose total cost could not be paid
  (CR 601.2h). Fixed: `delve_most` caps delve at the graveyard's cards less
  those the spell's other graveyard parts take, in both places. Two Underworld
  Breach scenarios hold it. Prevention: when two parts of one cost draw on the
  same pool, the legality check must count the pool once for both. The fuzz
  caught this class, and no type can express a shared pool cheaply.

- **An additional cost and escape could count the same graveyard cards.** After
  the delve fix above, the nightly fuzz still panicked with "enough cards to pay
  the cost" in `casting::parts::choices`: Abhorrent Oculus, whose additional
  cost exiles six cards from its caster's graveyard, was offered with the escape
  that Underworld Breach grants, which exiles three other cards, though the
  graveyard held fewer than nine. `casting::parts::spell_payable` asked
  `payable` about each part alone, so each saw enough cards. The delve fix had
  counted the pool once only for delve against the other parts, not for two
  counted parts against each other. Fixed: `parts_share_enough` checks every set
  of the parts that take a number of objects from one zone against the objects
  any of them accepts (Hall's condition, exact for any filters). Two Underworld
  Breach scenarios hold it. Prevention: the same as the delve entry; the
  shared-pool check now covers every counted part of a spell's cost, and the
  fuzz's shrink test (`fuzz_games_keep_the_invariants`) found the case.

- **A prompt's lookahead could misread a paused resolution's answers.** The
  nightly's session fuzz (`session_games_keep_the_script_checks`) panicked with
  "an answer among the objects" in `choices::pick_about` while the session
  played a prompt's answers ahead on a copy of the game, to see which answer
  leaves more cards castable (`prompt::ahead`). `ahead::forced` switches the
  copy to forced stops, under which every question is asked, even one with a
  single answer. A paused resolution resumes by running its body again with the
  answers so far recorded in order, and under smart stops a question with one
  answer is answered silently and never recorded. Asked on the copy, such a
  question took the next question's recorded answer, here an index out of its
  range. Fixed: `forced` leaves the stops alone while a resolution or a step's
  actions are paused. A session test
  (`a_paused_resolution_keeps_its_stops_on_a_forced_copy`) holds it. Prevention:
  the record's meaning depends on the stops it was made under, and no type ties
  the two together yet; the session fuzz catches a misreading.

- **A spell's life payment ignored "your life total can't change".** Under
  Teferi's Protection or Platinum Emperion, a player was offered Force of Will
  for its alternative cost, and casting it exiled the blue card and paid no
  life, though no life can be paid while a player's life total can't change (CR
  119.8). An activated ability's life cost was already refused. Whether a
  spell's cost could be paid was decided by `casting::parts::spell_payable`,
  which asked the kernel's `State::can_pay_life`. That function only compared
  the amount with the life total, but its comment said every rule asked it. The
  check that also reads the rule change is `beryl_rules::game::life::can_pay`.
  Found by an adversarial review of the keystone cards. Fixed: `spell_payable`
  asks `life::can_pay`, and the kernel function is renamed
  `State::has_life_to_pay`, with a comment saying that it is only half the
  question. A Platinum Emperion scenario holds Force of Will under it.
  Prevention: when the kernel can answer only part of a rules question, its
  function should be named for that part, so that a caller can't mistake it for
  the whole answer. No check can stop the next such pair. Here the rename makes
  the mistake visible at the call site.

- **Painter's Servant entering with another permanent at once panicked.** When
  Worldgorger Dragon left and returned Painter's Servant together with another
  permanent that has a static ability, the engine panicked with "the layers read
  the amounts of a part that has some". Before asking a player to order the
  timestamps of permanents that entered together (CR 613.7m),
  `layers::together::parts` lists each static ability's parts. It resolved them
  with `LayerChange::resolve`, which cannot read "the chosen color", and took
  every part it left unresolved for a power or toughness amount to be read as
  the layer applies. The layers themselves resolve parts with
  `layers::elsewhere::resolve`, which reads the chosen color from the permanent.
  `parts` now uses `elsewhere::resolve` too, and a Worldgorger Dragon scenario
  returns Painter's Servant and Glorious Anthem together. Found by `just fuzz`.
  Prevention: `LayerChange::resolve` now takes the chosen color as an input, so
  every caller (the layers, the order of permanents entering together, and the
  kernel's reach into other zones) turns a part into a change with the one
  function, and `None` means only an amount read as the layer applies.
  `layers::elsewhere::resolve` is gone.

- **A merged permanent lost the replacement abilities and object rule changes of
  the cards under it.** Gemrazer cast for its mutate cost over Hullbreacher made
  no Treasure when an opponent drew an extra card, and over Bloodghast the
  merged creature could still block, though a mutated permanent has every
  ability of each card that represents it (CR 702.140e). `replacement::statics`
  dropped every ability other than `ObjectAbility::Own`, the per-permanent "may
  replace" tests read only the top face's mask, and the object rule change
  readers in `continuous/rule_change.rs` read only the top face's rules
  (`Definition::rules`), because each kept an `AbilityIx` on the permanent's own
  face as an ability's identity. Fixed: a replacement's source
  (`Source::Ability`, `Key::Ability`, `DrawReplacement::index`, `Sink::applied`)
  and a rule change's maker (`maker_of`) are named by `ObjectAbility`,
  `replacement::may_replace` and `continuous::rule_change::object_rules` add the
  faces under a merged permanent (`State::merged_faces_now`), and
  `static_condition_holds` reads a merged ability's condition as its own card's.
  Prevention: an ability's identity is `ObjectAbility`, never an `AbilityIx`; a
  reader that narrows it to `Own` with a `let ... else` should say in a comment
  which other kinds the card table refuses.

- **A merged permanent lost the activated, mana and static abilities of the
  cards under it.** Gemrazer cast for its mutate cost over Drudge Skeletons
  could not regenerate, over Gold Myr could not tap for {W}, and over Lord of
  Atlantis gave other Merfolk nothing, though a mutated permanent has every
  ability of each card that represents it (CR 702.140e). Only keywords and
  triggered abilities were read from the cards under it: `State::abilities`, the
  layers and the mana solver each read the top face's abilities directly. Fixed:
  `State::abilities` yields `ObjectAbility::Merged(place, index)` for each
  ability of a card under the permanent, activation and the mana solver take
  them, and the layers list their static abilities, masks and player rule
  changes (`continuous/layers/merged.rs`). Static replacement abilities and
  object rule changes from under a merged permanent are still not read: those
  readers name an ability by its index on the permanent's own face. Prevention:
  every reader of an object's abilities goes through `State::abilities`; a
  reader that walks a face itself, or keeps an `AbilityIx` as an ability's
  identity, misses each new place abilities come from.

- **Phasing out a permanent that entered this turn could panic.** The tracker
  check looked for the permanent in the battlefield zone set, which omits
  phased-out permanents even though they stay in the same zone (CR 702.26d).
  Fixed: the check includes phased-out permanents, and a regression test
  verifies that both turn trackers survive phasing out and in. Prevention:
  validate object lifetime against zone membership that includes phased-out
  permanents. Use the gameplay zone set when a rule needs to ignore them.

- **Reading the hidden picks of a long resolution in the `Large` layout panicked
  with a shift overflow.** `Resolving::hidden_picks` tested bit `i` of the
  32-bit `hidden` mask for every entry `i` of the answer record, and
  `State::record_answer` set bit `record.len()` for a hidden pick. A `Small`
  record holds at most 32 answers, but a `Large` record grows, so a resolution
  past 32 answers (the promotion test's) overflowed the shift as soon as a
  session view read its hidden picks, with or without any hidden pick in it.
  Fixed in the change that first read them from every view: the read looks at
  the first 32 entries only, and a hidden pick past them is refused as a full
  record is (commit 83aaebfc added the mask). Prevention: a bit mask indexed by
  a list position must be sized by the list's capacity in every layout, or
  stored in the list itself (a flag beside each entry), so that a `Large` list
  cannot outgrow it.

- **A save with a copy replayed a different attack once its cards were in other
  slots.** A Phantasmal Image copying Grizzly Bears and a real Grizzly Bears,
  both able to attack, were named alike in a save: an answer names an object by
  its name by the rules, "Grizzly Bears" for both. Attackers are asked one
  creature at a time, in slot order, so under the "other slot numbers"
  perturbation the recorded attack went to the other creature, and replay
  rebuilt the game from the turn's start. Two such objects were interchangeable
  before copies existed: the same card in the same state. Fixed: since save
  version 13 an object names the card it really is (`Obj::card`) and its copy
  exceptions (`Obj::excepts`), a spell copy on the stack is named as a token,
  and alike candidates on the stack are told apart by their places on it.
  Prevention: two objects a save names alike must do alike in every rule; a new
  characteristic a copy can carry goes into `Obj` in the change that adds it,
  and a save test with that card replays with `Game::replay_exactly` under each
  perturbation, since `Game::from_replay` hides a divergence by rebuilding.

- **A copy lost the subtype its copy exception added once a text change changed
  a land type word on it.** A token copy that is "an Illusion in addition to its
  other types", whose "Forest" a text change made "Island", was an Island Dryad
  and no longer an Illusion. Layer 3's `layers::text::type_line` rebuilt the
  permanent's subtypes from the copied face read through its words, overwriting
  the layer 1 subtypes that held the exception's addition (CR 707.9b). No card
  of the pool reached it, since Phantasmal Image sacrifices itself when Mind
  Bend or Crystal Spray targets it; the group 4 Warden found it in review.
  Fixed: `Copiable::with_excepted_subtypes` adds the exceptions over the
  subtypes read through the words. Prevention: a later layer step changes the
  value an earlier step computed rather than recomputing it from a narrower
  source, or the two share one function, so a part one of them adds cannot be
  forgotten.

- An Aura stayed on the battlefield attached to an object its enchant ability no
  longer allowed, and an Aura that became a creature stayed attached. The
  state-based action for Auras (CR 704.5m) asked only whether the enchanted
  permanent had protection from the Aura, because the only Aura then in the pool
  enchanted "creature or Vehicle" and made its creature a Vehicle. An Aura put
  onto the battlefield other than as a resolving Aura spell entered attached to
  nothing and died, where CR 303.4f lets its controller choose what it enchants.
  Found and fixed while adding Animate Dead, whose enchant ability changes on
  the battlefield: `attach::may_enchant` asks every enchant ability the Aura has
  now (CR 702.5c) and CR 303.4d, the state-based action and the attach
  instruction both ask it, and `replacement::enchants` makes the CR 303.4f
  choice. Prevention: a rule with a general condition ("an illegal object") is
  written in general and not narrowed to the cards of the day; a doc comment
  that says "the one implemented card does X, so" marks code to revisit when a
  second card arrives.

- **Storm copied nothing once its spell was countered.** With Brain Freeze or
  Tendrils of Agony on the stack, countering the spell in response to its storm
  trigger left the trigger with nothing to copy, though the rules copy it from
  its last known information (CR 113.7a, 608.2h). `actions::copy::instruction`
  copied only spells still on the stack, because the stack entry holds the
  targets, X and costs paid a copy takes, and `State::take_off_stack` dropped
  the entry. Fixed: `take_off_stack` keeps the entry and copiable values of a
  spell that an ability of its own still names (`State::departed_spell`), in
  games whose cards copy their own spells, and the copy reads it. Prevention: an
  effect that reads its source names, in its vocabulary entry, what it does when
  the source has left its zone, and each such entry gets a scenario that removes
  the source first.

- **A delve spell that needed restricted mana could not be cast.** With a Cavern
  of Souls that chose Zombie as the only land and six cards in the graveyard,
  Gurmag Angler was not offered: the six cards pay {6} and only the Cavern's
  colored mana can pay {B}. The legality check tries restricted mana and delve
  as two separate fallbacks (`casting::legal::pays_otherwise`), and the delve
  fallback (`casting::delve::delve_pays`) asked for a payment for no spell
  (`mana::Paying::NOTHING`), which restricted mana never pays. The casting
  machine itself passed the spell (`Paying::entry`), so only the check was
  wrong. Nothing caught it because the reference payability check
  (`reference.rs`) only checked payments for no spell, and no scenario combined
  delve with restricted mana. Fixed: `delve_pays` takes the face and pays for
  the spell (`Paying::casting`), and the reference now also checks each card in
  hand as a spell, counting the restricted mana it accepts. Prevention: a
  function that answers "can this spell be paid for" takes the spell, never a
  default `Paying`; a check of a new kind of payment adds the reference's
  version of it in the same change.

- A copy of an instant or sorcery that had resolved stayed in the stack zone
  with no stack entry, and a copy of a creature spell never reached the
  battlefield. Copies of spells are objects in the shared token pool marked as
  tokens, and `event::move_batch` skipped every token outside the battlefield,
  which CR 111.8 says can't change zones. Found and fixed in the change that
  added copies (Fork, storm). A token on the stack is a copy of a spell, which
  moves as a spell does: `move_batch` now lets it leave the stack, and the
  state-based check ends it in any zone but the stack (CR 704.5e). Prevention: a
  rule written for one kind of object ("a token") names in a comment the zones
  it was written for, and a new kind of object that shares the representation
  gets a test of each zone change it makes (`tests/spell_copies.rs`).

- A landwalk keyword that a static ability grants ignored text changes of the
  permanent with the ability. Lord of Atlantis whose "Island" Mind Bend changed
  to "Swamp" still gave other Merfolk islandwalk instead of swampwalk.
  `LayerChange::resolve` read the color words and subtypes of a modification
  through the source's `Words`, but passed `Modification::AddKeywords` through
  unchanged: landwalk keywords are plain bits in `Keywords`, not a word type, so
  the compiler did not ask for a reading. The layers read a permanent's own
  landwalk through its words, which is why the gap went unseen. Found and fixed
  in the change that added Lord of Atlantis: `resolve` reads granted keywords
  through `Words::landwalk`. Prevention: hold a landwalk's land type as a
  private word type, as filters hold subtypes, so that reading it without the
  source's words does not compile.

- Tarmogoyf in a graveyard, hand or library read as 0/1, its printed values,
  where CR 604.3 says its characteristic-defining ability works in every zone;
  Adeline, Resplendent Cathar off the battlefield had power 0. `State::power`
  and `State::toughness` answered an object off the battlefield with its
  copiable values, because only the rules could evaluate the ability's amount
  and the kernel cannot call the rules. A filter such as "power 2 or less" over
  cards in a graveyard would have accepted a large Tarmogoyf. Fixed by
  evaluating, in the kernel (`beryl_kernel::defined`), the amounts the card
  table admits in a characteristic-defining ability, behind one test of
  `Capability::DefinesPt`. Prevention: a kernel query that answers "elsewhere"
  with a fallback names in its doc comment which rules the fallback ignores, so
  a reader sees the gap; a card whose ability defines a characteristic gets a
  scenario that reads it off the battlefield (`Assertion::PtIn`,
  `Assertion::Colors`).

- An ability activated before any text change was in force read its source's
  text as it was when the ability resolved, not as it was when the ability went
  on the stack. Circle of Protection: Red's ability, with Mind Bend changing
  "red" to "white" on the Circle in response, offered white sources. A stack
  entry noted its source's words only while some text change was in force, and
  the value an entry starts with meant "read the source's words now", which was
  right only for a mana ability that never goes on the stack. Found and fixed in
  the change that added text changes: an entry starts as printed, and the
  resolution of a mana ability says "read now" itself. Prevention: the default
  of a field that records what was true at a moment is what a game without the
  mechanism records (here "as printed"), never "look it up later".

- An instruction that deals damage counted the damage it proposed as dealt, so a
  follow-up that reads "the damage dealt this way" (`Amount::ThatMany`,
  `Condition::Did`) would have seen 3 damage when a prevention shield let only 1
  through. `damage::deal` applied the replacement and prevention effects but
  returned nothing, and `damage::instruction` added the amount it had asked for.
  No card of the pool reads the outcome of damage yet, so no game was affected.
  Fixed by having `deal` return the damage it dealt and summing that. Found in
  review (commit c00e2222 introduced the damage proposal). Prevention: a
  function that applies replacement effects to a proposal returns what actually
  happened, and its callers report that, never the proposal.

- Jace, Wielder of Mysteries' +1 aimed at its controller milled Life from the
  Loam, and the draw that followed in the same resolution was not offered dredge
  3\. `replacement::may_ask`, read as a resolution or a step's turn-based actions
  begin, held only while a dredge card already sat in a graveyard. A run that
  began with it false ran on the committed state, which cannot pause, so the
  dredge choice it met later took its first candidate, declining. Fixed by
  making `may_ask` hold in every game that has a dredge card
  (`Capability::Dredge`), wherever the card is. Prevention: whether a run can
  pause is decided from what the run could reach, not from the state as it
  begins; a cheap test of the start state is only sound when no step of the run
  can change its answer.

- A dredge offered for the draw step's draw was first asked over the upkeep: the
  end of the upkeep and the whole draw step ran on a copy, and a pause discarded
  the copy, so the committed state was still in the upkeep while the question,
  replayed for the viewer, showed the draw step. The app would have shown a
  decision in a step that had never been shown to begin. A session test that
  plays fuzz decks found it before it landed. Fixed by beginning the step on the
  state itself and running only its turn-based actions on the copy
  (`Pending::StepActions`). Prevention: a run that can pause starts at the
  latest point that is the same for every answer, so the committed state shows
  everything the question is asked after; the session tests' check that each
  step is seen as it begins holds any new pause to that.

- A card manifested while a "nontoken creatures that weren't cast are exiled
  instead" replacement applied went to exile, but the opponent was made to
  forget it, so a card in a public zone was hidden from them and the engine's
  knowledge invariant failed. `actions::manifest::manifest` moved the card and
  then always made the players who had not seen it forget it, assuming it was
  now a face-down permanent; a replacement can put it anywhere. A fuzz game with
  Containment Priest and Abhorrent Oculus found it. Prevention: code that runs
  after a move reads where the object ended up (its zone and face) rather than
  where it asked to send it, since every move goes through the replacement
  pipeline.

- The test of whether the order of permanents entering the battlefield together
  could change the layer results (CR 613.7m, `layers::order_may_matter`) read
  what a static ability's filter reads only for a set "each X", not "each other
  X", so Opalescence's effect read no card types there while the dependency test
  (CR 613.8) said it read them. No game with the implemented cards showed it,
  because a dependency orders such effects before timestamps do. Found in
  review: the two tests had each written out what finding a static ability's
  existence and set reads. Prevention: one function, `apply::existence_reads`,
  answers it for both; a fact two tests need is computed in one place.

- A replacement of an opponent's draws ("If an opponent would draw a card, you
  win the game instead") would have made the drawing player win. What happens
  instead of a draw names players such as "you", and `drawing::draw` read them
  for the player drawing, which was right only while a draw replacement could
  replace nothing but its own controller's draws. Widening `Replaced::Draw` to
  name whose draws it replaces broke that unstated assumption; a unit test for
  the new case found it before it landed. Prevention: `replacement::draw`
  returns the replacement's controller beside what happens instead, so the
  reader of those players has the right player in hand and no other to choose. A
  function that reads `Players` should take the player they are read for from
  the source of the ability, never from the event it is about.

- Five card definitions did less than their Oracle text said, and every check
  passed. Thieving Skydiver stole an Equipment without attaching it to itself
  ([CR 701.3a](https://yawgatog.com/resources/magic-rules/#R7013a)): its comment
  said "No Equipment is among the cards", though Kaldra Compleat was in a
  decklist. Damn's creatures could have regenerated
  ([CR 701.19c](https://yawgatog.com/resources/magic-rules/#R70119c)), which
  nothing showed because no card regenerates yet. Currency Converter would have
  exiled a discarded card that a replacement effect had already sent to exile,
  because its definition said "that card", not "that card from your graveyard".
  Elite Spellbinder said "cast" for "play", and Subtlety's text rendered
  differently; those two behaved correctly. The XMage and Forge oracles only
  compare the two engines inside the scenarios the card's author wrote, and an
  author who skipped a clause also wrote no scenario that reached it. Found by
  the Oracle text diff (`just card render`) the day it landed. Prevention: the
  `card-text` check fails CI on any difference between a card's rendered text
  and its Oracle text that `data/card-text-allowlist.json` doesn't pin as
  wording or as a partial escalation, so a left-out clause can't land quietly.
  The test `every_mutant_of_a_card_is_killed_by_its_scenarios` changes each part
  of a card's effect data to a neighbouring value and fails when none of the
  card's scenarios tells the change from the card, so a clause can't be written
  and left untested; generated scenarios start from witnesses, positions such as
  a creature with a regeneration shield, under which a field of an entry
  matters. When a scenario needs a card nobody has implemented, implement one,
  even one in no deck: Leonin Scimitar exists so that the Skydiver's attach is
  tested in all three engines.

- A continuous effect on a locked-in set was applied to only some of its
  members, or to none, once the set was large (CR 611.2c, 613.7). The layers
  listed one entry for each permanent an effect changed, in a list of 192, and
  threw away the push that did not fit: three +1/+1 effects on 120 creatures
  left them 3/3 and 4/4 where each should be 5/5, with no stall into `Large`.
  The comment beside the push argued the list could not fill, which was true
  until an effect could change more than one object. Found by the milestone 4
  audit (by-w8gv.8.1) before any card made such a set; fixed in by-w8gv.8.3,
  where an effect is one entry whatever it changes and a full list stalls the
  game. Prevention: never bind a fallible push to `_`. A list whose bound is
  argued takes `Queue::push_bounded`, which checks the argument in debug builds,
  so the scenarios and `just fuzz` trip on a wrong one; any other propagates
  `Full`. The `discarded-push` check keeps `let _ = ....push(` out of the
  kernel, the rules and the cards.

- A continuous effect on a permanent past the 256th on the battlefield changed
  another permanent instead (CR 613.1). The layers kept an effect's battlefield
  position in a `u8` and narrowed it with `as`, and `Large` holds more
  permanents than a byte numbers: +3/+3 on the permanent at position 300 went to
  the one at position 44. Triggers that count attackers or created tokens
  narrowed the count the same way, so 300 attackers read as 44. Found by the
  milestone 4 audit (by-w8gv.8.1); fixed in by-w8gv.8.3. Prevention: a position
  or a count that grows with the layout has the layout's index type
  (`L::Index`), never a byte, and is converted with `index` or `try_from`, which
  cannot narrow in silence. A count that must fit a byte saturates and says so.
  `as` is for an enum's discriminant or a number a constant bounds.

- Two objects of one card were offered as one candidate when they differed only
  in what another record of the state said of them: a permanent that exiled a
  card until it leaves and its twin that exiled none
  ([CR 610.3](https://yawgatog.com/resources/magic-rules/#R6103): "Some one-shot
  effects cause an object to change zones 'until' a specified event occurs"), a
  permanent with a card exiled with it (CR 607.2a), a creature wearing an
  attachment that changes none of its layer results, a creature that a triggered
  ability on the stack, waiting or delayed will act on without targeting it, and
  two spells of one card on the stack with different targets. So a player
  bouncing one of two Portable Holes could not choose which, though one returns
  a card and the other does not, and one countering either of two removal spells
  was not asked which creature to save. `State::signature` compares what the
  state holds about an object itself; what holds a reference to it was a list
  the engine kept by hand (`cast::referenced`, `combat::related`: stack targets,
  the sources of abilities, combat), which each new reference had to be added to
  and four were not. Two waiting triggers of twins were compared by signature
  alone, with no list at all. Found by the audit after milestone 4
  (by-w8gv.8.1), the spells while fixing it; as old as each reference. The
  kernel now answers `State::named` beside the fields it reads, each store says
  which slots its own entries name (`Store::names`), and the engine's lists are
  gone. Prevention: one query, in the crate that owns the fields, for "what
  refers to this object"; a store cannot be written without answering it, and
  the Memos store's invariant holds a new kind of memo that names a slot to it.
  A field outside the stores that names a slot is still held to it by the
  query's rustdoc alone; renaming two slots through the `Relayout` walk and
  comparing the states would hold every field without a list, and was not built
  (by-w8gv.8.2 says why).

- Two objects of one card were offered as one candidate while different
  continuous effects were on them, whenever the effects came to the same layer
  results today: a creature with +1/+1 until end of turn and its twin with +1/+1
  for good, or a member of an effect's locked-in set and a creature the same
  size without it. `State::signature` compared a permanent's layer results,
  which say what its effects make of it now and not for how long, so the player
  could not choose between two objects that lead to different games
  (architecture §7.2). The rules kept a separate list of the objects an effect
  that shows nowhere is on (a permission to cast, a spell's terms) to cover the
  rest. Found by the review of the stores (by-w8gv.7.11); as old as continuous
  effects. The signature now compares the effects on each object (`EffectsOn`),
  behind the bit test it already paid, and the list is gone. Prevention: what
  makes two objects interchangeable is everything the state holds about them, so
  a store answers "what do you hold about this object" once, for the signature,
  and a test of a new store shows two objects that differ only in it are told
  apart.

- The lands one effect untapped came untapped in slot order
  (`EffectCtx::untap_up_to_lands`, Teferi, Hero of Dominaria's +1 and Time
  Spiral, walked the chosen set in candidate order), so the order of the untap
  events, which the UI animates and a "becomes untapped" trigger would follow,
  depended on slot numbers. The effect untaps them together
  ([CR 701.26b](https://yawgatog.com/resources/magic-rules/#R70126b): "To untap
  a permanent, rotate it back to the upright position from a sideways
  position"), so they now go in battlefield order (`State::in_game_order`).
  Found by the audit of every walk of a set (by-w8gv.5.6); no scenario caught
  it, because the three slot numberings compare positions, which are the same
  whichever land untaps first, and the one scenario untapped two Plains.
  Prevention: a set whose members an effect acts on one by one goes through
  `in_game_order`; and a test of an order that only events show watches the
  events under every numbering (`beryl_scenario::watch_with`), with cards that
  differ.

- Three reads answered one rules question differently, depending on who asked: a
  filter's mana value left out a spell's X where the interpreter's counted it
  ([CR 202.3e](https://yawgatog.com/resources/magic-rules/#R2023e): "X is
  treated as the number chosen for it while the object is on the stack"); a
  token that was 1/1 by a copy's exception read as its card's size once it left
  the battlefield (CR 707.9b); and last-known information of a face-down
  permanent named its hidden card, which a copy made from it would have been (CR
  707.2, 708.2). No card of the pool reached any of them. Found moving the reads
  into `beryl_kernel::query`. Prevention: one query per rules question, named
  for its rule, with the definition index private to the kernel, so a second
  answer cannot be written beside the first.

- Declared attackers tapped in slot order (`combat::finish_attack` walked the
  attackers' `Bitboard`), so the order of the tap events, which the UI animates
  and a "becomes tapped" trigger would follow, depended on slot numbers. The
  taps are simultaneous under
  [CR 508.1f](https://yawgatog.com/resources/magic-rules/#R5081f): "The active
  player taps the chosen creatures." They now go in battlefield order. Found by
  running Teferi, Time Raveler's scenarios with the decks' slots renumbered.
  Prevention: every card's scenarios now run under three slot numberings
  (`every_card_has_scenarios_and_they_pass`) and must trace the same; never let
  a `Bitboard`'s iteration order reach an event.

- Casting Abhorrent Oculus exiled graveyard cards before activating the mana
  abilities used to pay for it. This violated
  [CR 601.2g](https://yawgatog.com/resources/magic-rules/#R6012g): "Mana
  abilities must be activated before costs are paid." The freedom to order
  ordinary payments under
  [CR 601.2h](https://yawgatog.com/resources/magic-rules/#R6012h) applies to
  spending mana and exiling cards, not to activating mana abilities. The
  casting-order fix activates mana abilities and spends the mana before opening
  the exile-cost choice. Prevention: model mana production separately from cost
  payment, and test event order across automatic and manual payment, including
  sacrificed mana sources and cancellation. A visually late mana animation can
  expose an engine sequencing violation, not just a UI defect.

- Moving mana payment before exile selection changed the order of saved cast
  decisions. A historical explicit exile choice followed by a mana plan could no
  longer replay. Version 7 reorders completed costs with their undo boundaries;
  an interrupted cast without a payment choice resumes at that choice with
  unfinished exile picks restored. Prevention: test historical complete and
  partial cost sequences, alongside migration idempotence and current-save
  reloads, when changing decision order. Exact replay also counts a resumed
  checkpoint only once when the new log repeats its opening board.

- Exile costs counted a Treasure sacrificed for mana as a graveyard card while
  the cast was still pending. Moving mana payment before the exile chooser
  exposed the extra candidate and could let a token pay a card cost. Tokens are
  not cards (CR 111.6), even before state-based actions remove them. Prevention:
  check card identity when listing candidates and building the
  `Cards<Graveyard>` passed to the payment function. Its compile-fail doctest
  rejects a token's slot set. The Oculus Treasure scenario covers the interval
  between payment and state-based actions.

- A slot could reuse its first incarnation after 65,536 zone moves, library
  rebinds or token removals, so a stale reference could name the current object.
  Each increment now checks for exhaustion and stalls the game at the capacity
  limit. A kernel test exercises the last valid incarnation and checks that the
  original reference stays stale.

- A paused resolution's runs disagreed on what was known: each pause kept in the
  committed state what its run had shown, and the next run started from that, so
  a card shown after one question was known before it. When Thirst for
  Discovery's first discard went to the graveyard, the opponent knew that
  Plains, and the next run merged it with the Plains they had already seen; the
  first discard then had other candidates, its recorded index named another
  Plains, and the second discard's index was out of range (a panic in a live
  game). Prevention: answers recorded by index are only as stable as the
  candidates they index, so every run of a paused body must start from the same
  state (`Resolving::knowledge` holds what was known as it began); a test that
  pauses over copies known to different players holds it. Rules knowledge and
  chance's "first time anyone learns it" then differ inside a run, so the sink
  carries what earlier runs revealed and chance counts each once; the frozen
  corpus caught the double reveals.

- More slot-order ties, found by replaying 68 saves under another seed: a
  floating effect named its source by slot, so once the source was shuffled into
  a library and its slot rebound, `State::off_board` (and the grant source the
  layers read) named whatever card the slot then held; combat damage left over
  after a division went to the chosen or remaining blockers in slot order;
  `collapse` merged a copy in hand the opponent had seen with one they hadn't,
  so which one was played, and what they still knew, followed slots; and a
  save's object description lacked the owner, so of two Mana Crypts one player
  controlled, a replay targeted the one in the lowest slot. Prevention: an
  effect keeps its source's card, not its slot (CR 113.7a); order by battlefield
  order, never by slot; a candidate signature and a save's description must
  cover everything that can make two choices lead to different games (owner, who
  knows it); the ignored sweep in `beryl_session/tests/chance.rs` replays 68
  games under another seed.

- Choices the engine and session make for a player depended on slot numbers,
  which the deck deal and every shuffle choose at random: mana sources of one
  rank sorted by lowest slot (`mana::Sources::of`), a class's members tapped in
  slot order, the human's look-ahead met hand costs in slot order before its
  bounded search gave up, and an answer inside a paused resolution was described
  in the committed position, where `collapse`'s lowest-slot pick among
  interchangeable cards could be the copy just drawn (still in the library
  there) or the one already in hand. So the same game dealt by another seed
  tapped other lands, and a save replayed under another seed went elsewhere.
  Found by replaying saves under another seed. Prevention: never let a slot
  number, or a list built in slot order, break a tie: order by card, then by
  battlefield order; test every behavior that must not depend on randomness by
  replaying under another seed.

- Knowledge of hidden cards is kept per slot, so a card a player saw in the
  opponent's hand (fatesealed, then drawn) stayed known after the opponent
  brainstormed, though nothing shows which hand cards went back on the library
  (CR 400.2, 402.3); the UI kept showing it face up in their hand.
  `zone::move_to_place` now makes the other player forget the moved card and the
  whole hand when a card goes from a hand to a library. Prevention: treat every
  move between hidden zones as a question of what each viewer can still tell
  apart, in the one zone-move chokepoint, and test knowledge after each such
  move, not just after reveals.

- Decisions collapsed an activated ability's source with an interchangeable twin
  (`engine::collapse`, which keeps only the objects `cast::referenced` names
  apart), so with two Strip Mines the target decision offered one, and taking it
  aimed the ability at its own source, sacrificed as its cost, rather than the
  twin (CR 601.2c, 601.2h, 602.2b). Found by a generated oracle scenario, where
  XMage destroyed the twin. Prevention: derive "interchangeable" from everything
  a pending object refers to, the stack entries' sources as much as their
  targets, in one place both the stack and collapse read; generated scenarios
  check name-equal candidates against the oracles.

- The battlefield's cached trigger listeners (`State::listening`, found by the
  layer recompute) counted face-down permanents by their hidden card, so masking
  or determinizing that identity for a viewer left the cache describing cards no
  longer there while the layer results stayed clean. A face-down permanent has
  no abilities (CR 708.2a), so no trigger was lost, but the cache was wrong; its
  debug assertion caught it in the expert's combat simulation. Prevention: cache
  only what the layer results themselves depend on, so any change that leaves
  them clean leaves the cache right too, and check every cache against a fresh
  computation in debug builds.

- After a mulligan the engine asked for the cards to put on the bottom before
  the keep decision, so a player chose what to bottom from a hand they might
  then shuffle away, and "keep" described a bottom already done (CR 103.5).
  Prevention: derive the bottom count from the keep itself (`start::keep`), the
  one place a hand becomes the opening hand, so no other step can create it.

- The payment solver ignored Treasures, so under auto and greedy payment a
  spell, ability or "unless pays" cost payable only with one was not offered;
  the Treasure was instead offered as five standalone activations at every
  priority, to float mana first (CR 111.10a, 605.3a, 601.2g). Prevention: feed
  every mana ability into one source list the solver, the prefilter and manual
  payment all read, marking a source's cost (sacrifice, life) as an attribute of
  the plan rather than keeping a second, parallel path for the abilities that
  cost more than {T}.

- Taking back an unpaid cast (`beryl_session`'s `Game::cancel`) judged "still
  casting" by `Pending::Casting`, which the engine also uses to choose a
  triggered ability's targets once a cast is complete (CR 603.3d), so a cast
  trigger's target prompt would have offered to rewind a paid spell. Found in
  review. Prevention: give a trigger's targeting its own pending variant, or a
  query such as "a cast awaits payment", so no caller has to know the flag.

- A token copying a face-down permanent still held the original's hidden card,
  masked only by knowledge, so `Observation::determinize`, which redeals unknown
  deck slots alone, handed it to the searching agent unchanged, and every
  face-down rule needed a token exemption (CR 707.2, 708.2). Prevention: a copy
  is a token of its copiable values, never of a hidden card; the face-down copy
  is a token of the public synthetic card `FACE_DOWN_COPY`, so no hidden
  identity exists outside the deck slots that determinization redeals.

- Fractured Identity's copy of a face-down permanent kept the original's card
  and face down, so the copy's controller knew the hidden card, could turn the
  copy face up for that card's cost, and both players saw it as the copy left
  (CR 708.2, 701.40b, 708.9). Prevention: treat "the card behind a permanent" as
  its own query that tokens answer with none, rather than reading `card` and
  `face` directly wherever a face-down rule applies.

- A token copy of a transformed double-faced permanent (Fractured Identity on
  Jace, Telepath Unbound) took its front face's mana value, as the original
  does, where CR 712.6d makes a copy of a back face 0, so mana value filters
  such as Portable Hole's misjudged it. Found in review of b2d02dd. Prevention:
  derive a characteristic from one function of the object's copiable values
  (card, face up, token), not from the face alone, so each new way of creating
  an object meets every rule keyed on how it was made.

- A token copy of Currency Converter (from Fractured Identity) exiled cards
  "with" itself, then left the battlefield and ceased to exist.
  `zone::cease_to_exist` reset the slot without unlinking those cards, so they
  named an empty slot, which the next token to take it would have inherited (CR
  607.2a, 400.7). Prevention: route every change of a slot's object through one
  function that clears the links into it, rather than repeating the clean-up at
  each site; the fuzz invariant on links found it once a token could carry a
  linked ability.

- Winds of Abandon counted each exiled creature for its controller but, when a
  replacement kept one from exile, took the correction from its owner's count,
  so a stolen creature could cost the wrong player a basic. Prevention: split
  the objects by the player that matters once, before they move, and derive
  every count from that one split rather than recomputing who it was.

- Saves still depended on slot numbers and candidate order, found by the save
  format's perturbation tests: a batch of objects moved in slot order, so cards
  put into a graveyard together (a wrath, combat deaths, a mill) lay in an order
  the seed chose, and Abhorrent Oculus's exile cost, which counts from the
  bottom of the graveyard, offered other cards; mana sources of one class (an
  Island and Sink into Stupor's land face) sorted by the card in their lowest
  slot, so the payment tapped other lands; and an answer named a permanent too
  loosely to tell a token copy from its card, a summoning-sick creature from
  another, or an attacker of a planeswalker from one of the player, nor which of
  a permanent's two triggers it ordered. Prevention: no engine ordering may come
  from a `Bitboard`'s iteration where the order outlives the call; key it on
  something the rules see (battlefield order, library order, name). Run the
  perturbation tests over freshly frozen saves after any change to how answers
  are described.

- A triggered ability with no targets went on the stack (CR 603.3) without
  emitting the stack announcement. If it resolved before the next view, the UI
  could show its effect without animating the ability from its source.
  Prevention: finish putting abilities on the stack through a shared
  announcement path, regardless of whether they ask for targets.

- The effect interpreter counted a target twice when an instruction set out to
  move it and could not, as when "exile target ... until this leaves the
  battlefield" resolves after its source has left (CR 610.3b): the target was
  marked moved before the move, so a later "its power" or "its mana value" added
  the last known value to the one it still had. Prevention: do not keep a flag
  for what the state already says. Whether an object moved is whether its
  reference is still current, so the register now only says that its last known
  information was noted.

- "Their controllers", said of what an instruction had just moved
  (`ControllerOf(Objects::Those)`), named each card's owner: the interpreter
  read the controller of a card where it now was, a graveyard or exile, where a
  card has none (CR 108.4a), though it kept who controlled each as it left for
  counting them (CR 608.2h). A permanent controlled by another player than its
  owner charged the wrong player. No card said it yet. Beside it, "if it was a
  [filter]" of a target the effect had moved was held only by a debug assertion,
  so a release build answered no. Prevention: one rule for what a moved object
  was. Each expression that needs last known information either reads a register
  the interpreter keeps, or the definition check refuses it as the card compiles
  (`Malformed::LastKnown`); a comment promising a value is no guard, and neither
  is an assertion a release build drops.

- A counted cost's cards (Abhorrent Oculus exiling six from a graveyard) and the
  cards of a put-back (Brainstorm) each moved as its own event, one per pick,
  though CR 601.2h pays a cost whole and CR 401.4 puts the cards in the library
  at the same time. Scripts read simultaneity from the engine's groups, so they
  showed one beat per card. The move came where the pick was answered, and a
  group cannot stay open across a decision. Prevention: when the rules do one
  thing to cards chosen one at a time, keep the picks (`Casting::chosen`, or a
  set in the resolution as `dig` does) and move the cards in one batch after the
  last; never move a card where its pick is answered.

- A permanent that had lost all its abilities in a `Small` game was widened into
  `Large` with only its first sixteen abilities lost: "all lost" is every bit of
  the layout's mask (`AbilityMask::ALL`), and widening copied the bits, so a
  `u16` of ones became a `u64` with sixteen. The widened game differed from one
  played wide all along (`a_widened_game_plays_on_as_the_small_one`), and a face
  of more than sixteen abilities would have kept the rest. Found and fixed in
  the change that added the mask. Prevention: a value that means "everything" is
  relayouted as that meaning (`widen_mask` maps `ALL` to `ALL`), never as its
  bits; a flag or an enum for "all" would make the copy impossible to get wrong.

- A static ability whose permanent lost it to another effect of the same layer
  still applied. The recompute asked whether each permanent still had its
  ability at the head of the layer, before any of the layer's effects applied,
  so Urborg, Tomb of Yawgmoth entering after Blood Moon still made every land a
  Swamp, and a creature's own layer 6 ability still applied under a "creatures
  lose all abilities" with an earlier timestamp. CR 613.8a makes such an effect
  depend on the one that removes its ability, and CR 613.8b applies it after;
  with no dependencies the head-of-layer check was the closest approximation.
  Found and fixed in the change that added dependencies: the existence of a
  static ability is asked as its effect comes up, after the effects it depends
  on. Prevention: a fact that an effect of the same layer can change is read
  where the effect applies, never cached at the layer's head; each effect's
  declared reads and writes (`LayerChange::writes`, `dependency::reads`) say
  which facts those are.

- A static ability that reads whether a permanent is tapped went stale when the
  permanent tapped. Builder's Blessing ("untapped creatures you control get
  +0/+2") kept its bonus on a creature that had tapped to attack, because
  `State::set_tapped` never marked the layer results stale, and the layers read
  `Filter::Tapped`. The test of the filter marked the results stale by hand
  (`forget_derived`), so it passed. Found by the card's scenario, which the
  layer reference check failed ("layer results differ from a fresh recompute").
  Fixed in the change that added the card: `Inputs::TAPPED` says that a
  permanent on the battlefield asks, and `set_tapped` marks the results stale
  only then. Prevention: every field a filter or condition can read has an
  `Inputs` bit and a mutator that checks it, and a test of such a read changes
  the field through the mutator the rules use, never an arranger that marks the
  results stale itself.

- **Choosing a creature type as a permanent entered allocated on the heap.** The
  allocation test (`beryl_search/tests/allocations.rs`), which plays random
  games from fuzz decks, counted four allocations once Cavern of Souls joined
  the card pool: it was the first card to choose a creature type as it enters.
  Two places added for chosen values allocated: `ChosenValue::candidates` boxed
  the iterator of each kind (`Box<dyn Iterator>`), and `Sink::chosen`, the
  values a batch of entering permanents chose, was a `Vec`. No test reached
  either with a card that chooses a creature type, so the allocation-free hot
  path (§18) silently broke. Fixed: the candidates chain one iterator of each
  kind, all but one empty, and `Sink::chosen` is a fixed queue of four, `Full`
  past it. Prevention: a new kind of choice gets a card in the fuzz pool in the
  change that adds it, so the allocation test plays it; and lists on the event
  sink are fixed-capacity queues, as the kernel's other lists are.

- **A copy of a spell that resolved before the next view was never put on the
  stack in the script.** `beryl_cli script-check` failed every Fork and Brain
  Freeze moment: the copy's effects named it as their cause, but no step put it
  on the stack. The script builder takes the record of an object moving to a
  zone from the view, or from the table before it. A copy has no card, so once
  it has ceased to exist neither one has a record of it, and the builder left
  out its arrival on the stack and, with that, its departure. Fixed: the session
  records each copy's record and stack item from the engine's stack arrival
  (`Heard::copies`), and the builder uses it when no view has the copy.
  Prevention: any object that can come and go within one view (copies, tokens)
  needs a record from the event that makes it, not from the view after it; the
  script checks catch the gap only when a spec moment plays it, so a new kind of
  object gets a spec moment in the change that adds it.

- **Fork's copy stayed red when a text change made Fork's "red" read "blue".**
  Crystal Spray on Fork, changing "red" to "blue", should make Fork's copy blue,
  since "except that the copy is red" is a color word of Fork's text (CR 612.1,
  CR 707.9b). Beryl made it red: `CopyChange::Colors` held a plain `Colors`, and
  a copy's exceptions are read lazily from the card, so nothing read them
  through the copier's text changes. Fixed: `CopyChange::Colors` holds
  `ColorWords`, which can only be read through an object's `Words`, and the
  copiers make copies with `Copiable::with_addition_read`, which records the
  colors as the copier's words read them when they differ from the printed ones.
  Prevention: every value in the card vocabulary that a card's text names as a
  color word or subtype is a word type from `beryl_card::words`, so a reader
  that skips the text changes does not compile.

- **Thespian's Stage copying itself stalled the game after a few turns.** Each
  time Thespian's Stage became a copy of itself (or two Stages copied each
  other), its copiable values gained its copy exception, "except it has this
  ability", once more. The list of exceptions holds three in the `Small` layout
  and eight in `Large`, so a few activations moved the game to `Large` and the
  ninth ended it `Stalled(Capacity)`, on a line any rollout or fuzzer can play
  for {2} a turn. Fixed: `Copiable::with_addition_read` does not add an
  exception the values already carry when carrying it twice would change
  nothing. A repeated `Has` is collapsed only for an ability whose instances
  can't be told apart (activated and mana abilities, CR 113.2c), and a repeated
  value-setting exception only when it is the last one (CR 707.9b). Prevention:
  any list that grows each time a repeatable action resolves needs a test that
  repeats the action past the list's capacity; the fuzzer reaches such lines
  only by chance.

- **A static ability that changes a rule for players did nothing until its kind
  was listed twice.** Platinum Emperion's "Your life total can't change" let
  Lightning Bolt lower its controller's life, while the same change made by
  Teferi's Protection's resolved effect worked. The layers collect a static
  ability's rule change for players only when the card table's set of such
  changes holds it, and `RuleChanges::of_players` built that set from a
  hand-written list of three kinds, separate from `RuleKind::of_players`, which
  already named the new `RuleKind::Player`. Found by the card's scenarios and
  fixed in the change that added the kind: `RuleChanges::number` now adds each
  change whose kind `RuleKind::of_players` names to the set, so the two can't
  disagree. Prevention: derive a set from the one predicate that defines it,
  never from a second list of variants.

- **A turn-start checkpoint with a merged permanent claimed to be complete.** A
  checkpoint records each permanent by its own card, and `State::off_board` says
  what a rebuild from it would lose. A mutated permanent's other cards (CR
  731.2) sit in `Zone::Merged`, which no checkpoint lists, but `off_board` did
  not mention them, so the save offered a board that could not be rebuilt with
  Gemrazer's card under its creature: the rebuild lost the card or failed on the
  library's count. Fixed: `off_board` reports a battlefield permanent with a
  non-empty pile, as it already reported a phased-out one. Prevention: a new
  zone or a new store entry that outlasts a turn needs a line in `off_board` or
  in the checkpoint; a check that every scenario's turn-start checkpoint either
  rebuilds to the same board or says why not would find the next one.

- **Ignite's build repair cleared the binary that failed to link, not the
  damaged library it linked.** In a landing tree, the Beryl 3D product failed
  five times in twelve hours, and tip verification held `master` back for over
  two hours: the binary `record` failed to link for `.llvm.<n>` symbols that
  `libberyl_inspect-<hash>.rlib` referenced but no longer held, because that
  library held objects from two compiles. The build manager clears and rebuilds
  a crate whose failure looks like damage to the tree's build, but it read the
  crates only from Cargo's `could not compile` lines, which name the binary.
  Each repair rebuilt `record` and failed again in about a second. Fixed:
  `compile_failure` in `ignite_daemon`'s `build/repair.rs` also names each crate
  whose library the linker reports an object of, and the repair clears those
  too. Seeding a new tree from another tree's build did not cause the damage
  (only ticket trees are seeded, and this was a landing tree), but it copied the
  damaged library to the ticket tree it seeded. Prevention: the guard is the
  test `a_link_failure_in_a_dependency_clears_the_dependency`; a repair that
  reads a failure's output should be tested against a real output, not one
  written for the test.

- **The gate reused a pytest pass after the data file the test reads changed.**
  A landing that taught Beryl 3D's `Style` to draw box shadows also edited
  `data/beryl3d/match/supported.json`, which lists the style shapes Beryl 3D can
  express. Its gate took 19 seconds because every pytest check was a cache hit,
  and tip verification then failed four tests in `test_beryl3d_match.py` and
  `test_beryl3d_match_styles.py`, which held `master` back for over an hour. A
  pytest check's input hash covered the test's file, the modules it imports and
  its conftest files, but `beryl_tools.beryl3d.match.support` opens
  `supported.json` at run time, which no import names. Fixed:
  `data/pytest-reads.json` records the tracked files each module reads at run
  time, and the adapter adds a module's entries to the inputs of every test that
  imports it. Prevention: the guard is the pytest plugin
  `beryl_tools.ignite.pytest_reads`, which the adapter loads for every pytest
  check. Its audit hook sees every file a test's code opens and fails the test
  when the record does not list the file for the module that opened it. The same
  gap remains for check families whose run-time reads happen in a subprocess
  (node, a browser, a Rust binary), which a Python audit hook cannot see. The
  first run with the record found two more stale passes of the same kind:
  `test_ignite_rest.py` reads `ignite/checks/rest.toml` and `test_changes.py`
  reads `data/efficiency-changes.json`, and changes to those files had broken
  both tests without the gate noticing.

- **The UI checks could reuse a pass after a data file they read changed.** An
  audit of every check family's declared inputs, made after the gate reused a
  pytest pass whose data file had changed, found three UI families in
  `ignite/checks/ui.toml` with the same gap. `ui-types` runs `tsc -b`, which
  follows `src/stage/timing.ts` and `src/table/commands.ts` into
  `data/ui-conventions.json` and `data/table-commands.json`, but the family
  listed only `ui/` files. `ui-unit` gave each test's check the data paths named
  in the test's own source, so the 17 stage tests and four table tests that
  reach those JSON files through `src/` modules did not declare them.
  `ui-format` runs prettier, which honors the root `.editorconfig`, outside
  `ui/**`. Fixed: `ui-types` and `ui-format` list those files, and
  `ui/scripts/test-inputs.mjs` follows each unit test's relative imports
  transitively and lists every file it reaches outside `ui/`. Prevention: the
  import walk keeps `ui-unit` right as tests and modules change, and its test,
  `ui/scripts/test-inputs.test.mjs`, checks that a stage test declares
  `data/ui-conventions.json`. No guard sees what `tsc` or prettier read, since
  they run as subprocesses that a Python audit hook cannot watch, so their
  inputs stay a hand-kept list.

- **The search dealt any card into a face-down creature cast through Illusionary
  Mask.** An opponent's face-down permanent that Illusionary Mask cast must be a
  creature card, which every player knows, but the determinization the hidden
  search plays its worlds from (`Observation::determinize`) dealt it any card of
  the opponent's remaining deck, often a land or an instant. It also took the
  spell on the stack for a morph and looked for a card with a morph ability.
  Fixed: `observation.rs` reads what the way a card was cast face down shows of
  it (`FaceDownEvidence`): a card that remembers it turns face up as it is used
  is dealt from the creature cards, and a morph from the cards with morph.
  Prevention: `beryl_session/tests/face_down.rs` deals the opponent's view of a
  Mask-cast creature on the stack and on the battlefield and holds every deal to
  a creature card. No type ties a face-down cast to its evidence, so a new way
  of casting face down must add its own case.

- **The greedy agent activated Illusionary Mask for X = 0 until the turn ran
  out.** With its lands tapped, the rollout policy scored the Mask's activation
  like any other, and with no mana for X the activation asks no question, so the
  agent activated it again at every priority and the game stalled on the
  per-turn decision limit. Fixed: the policy does not activate an ability with
  {X} in its cost while it has no untapped mana source to spare beyond the rest
  of the cost. Prevention: `beryl_search/tests/all/card_shapes.rs` plays the
  game that stalled, and every agent plays a game of each card-shape keystone
  family in `ignite check`.

- **Tarmogoyf counted only the left half of a split card in a graveyard.** A
  split card has each card type of either half in every zone but the stack (CR
  709.4c), so an instant // sorcery split card in a graveyard counts as two
  types. `State::card_types_in_graveyards` read the face that is up from the
  object's definition (`State::definition`), which for a split card is its left
  half alone, and so counted one. Fixed: in a game with a split card it reads
  the card's copiable values (`State::copiable`), which combine both halves off
  the stack. Prevention: `beryl_kernel/tests/queries.rs` holds the count of a
  split card with an instant half and a sorcery half to two. No type stops a
  reader from taking one face's characteristics directly; the review of the card
  shapes found this one by searching for readers of `definition(..).face(..)`.
