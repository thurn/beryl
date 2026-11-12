# Bug history

Rules engine bugs identified and fixed, kept to understand how to mitigate
similar classes of bugs in future (e.g. via the type system). AI, UI and tooling
bugs do not belong here. Each entry gives a short description of the bug, how a
similar bug could be prevented, and the fixing commit when it was not found and
fixed in the same session. Newest first.

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

||||||| parent of 51be9f58 (fix: layers and counts hold past a layout's widths)

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
  filter cost eligibility and candidate counts by card identity, rather than
  assuming every object in a graveyard is a card; the Oculus Treasure scenario
  covers the interval between payment and state-based actions.

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
