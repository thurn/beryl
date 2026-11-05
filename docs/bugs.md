# Bug history

Bugs identified and fixed, kept to understand how to mitigate similar classes of
bugs in future (e.g. via the type system). Each entry gives a short description
of the bug, how a similar bug could be prevented, and the fixing commit when it
was not found and fixed in the same session. Newest first.

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
