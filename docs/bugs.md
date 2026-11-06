# Bug history

Rules engine bugs identified and fixed, kept to understand how to mitigate
similar classes of bugs in future (e.g. via the type system). AI, UI and tooling
bugs do not belong here. Each entry gives a short description of the bug, how a
similar bug could be prevented, and the fixing commit when it was not found and
fixed in the same session. Newest first.

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
