# Bug history

Rules engine bugs identified and fixed, kept to understand how to mitigate
similar classes of bugs in future (e.g. via the type system). AI, UI and tooling
bugs do not belong here. Each entry gives a short description of the bug, how a
similar bug could be prevented, and the fixing commit when it was not found and
fixed in the same session. Newest first.

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
  does, where CR 712.8e makes a copy of a back face 0, so mana value filters
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
