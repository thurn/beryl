# Bug history

Bugs identified and fixed, kept to understand how to mitigate similar classes of
bugs in future (e.g. via the type system). Each entry gives a short description
of the bug, how a similar bug could be prevented, and the fixing commit when it
was not found and fixed in the same session. Newest first.

- **The expert's bonuses were discounted with the base value.** Returning a
  blocker that opened lethal was worth +500, but the quarter-value discount for
  creatures whose enters-the-battlefield effect is spent applied after it, so
  Jace bounced a tapped Overlord instead of the last Thundertrap Trainer
  blocking lethal. The intrinsic worth of a return is now its own function, and
  the situational bonuses are added to its result. Prevention: keep a value and
  the bonuses on it in separate terms, so that a scaling meant for one cannot
  reach the other.

- **An attack won on a block the defender need not make.** Weighing an attack as
  the worse of blocked and unblocked plus a quarter of the difference turned a
  lethal unblocked line (a win score) into a huge bonus, so the expert attacked
  into a block that lost it the game. When either line decides the game, the
  worse alone counts. Prevention: never take a fraction of a win or loss
  sentinel; compare such scores before doing arithmetic on them.

- **The expert's lethal checks ignored flying.** Its pre-combat damage estimate
  let any untapped creature block any attacker, so it missed lethals where only
  fliers could block (two bounces on the two flying blockers) and counted ground
  blockers against fliers. Its estimates now block fliers only with flying or
  reach (CR 702.9b). Prevention: one damage estimate shared by every lethal
  check, built on the same blocking rule the combat simulation uses, rather than
  a count of blockers in each.

- **The expert decked itself by choice.** Timetwister was cast with fewer than
  seven cards to shuffle in and draw, and Staff of the Storyteller drew the last
  cards of the library at the opponent's end step. Wheels now leave a card for
  the draw step after their seven draws, as other optional draws do. Prevention:
  route every voluntary draw through one check of the library, as the engine
  routes every draw through one function.

- **A long card name kept its ellipsis instead of shrinking.** `useNameFit`
  compared `scrollWidth` with `clientWidth`, which WebKit rounds to whole
  pixels, so "Swords to Plowshares" overflowing its name bar by under a pixel
  measured as fitting and was cut to "Swords to Plowshar…". It now measures the
  text's unrounded rect. Prevention: measure layout for decisions with
  `getBoundingClientRect` (or a Range over the text), never the integer scroll
  and client sizes.

- **An oracle's question surface allowlisted as a rules divergence.** Three
  XMage scenarios (Mana Tithe and No More Lies against a controller who cannot
  pay) sat on `data/oracle-divergences.json` under CR 118.3, so XMage never
  checked them. XMage was not wrong: it asks "Pay {N}?" anyway and the payment
  fails after a yes, with the same outcome. The exporter now answers no.
  Prevention: allowlist only a different game outcome, never a question the
  oracle asks and Beryl does not; translate those in the exporter, like its
  other per-card question tables.

- **The oracles ran only by hand.** `just oracle` was documented as nightly, but
  nothing ran it, so a card change could break agreement with XMage or Forge
  unnoticed. It is now slow CI's `oracle` lane. Prevention: a check that is not
  in `just check` or `just ci` does not run; put it in a lane or delete it.
