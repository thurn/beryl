# What a strong digital MTG client does

Backticked names are the engine's decision kinds and `ask` variants. "Fails"
cites a known failure in Arena, MTGO, Forge or XMage.

### Every decision

- **Waiting on whom**: can the player tell at a glance whether the game waits on
  them or on the AI? Expected: one prompt in a fixed place on their move; an
  "opponent is thinking" state otherwise, the board still usable; never a dead
  end.
- **Wording**: does the prompt name the action, the card asking and any count
  ("Choose a target for Lightning Bolt", "Put back 2 cards: 1 of 2")? Do buttons
  say their result ("Keep 7", "Attack with 3", "No blocks") rather than OK/Done?
  Fails: Forge's OK/Cancel, whose Cancel means "no" in one prompt and "back" in
  the next.
- **Pick in place**: are objects on the table chosen by clicking them where they
  sit, eligible ones lit and the rest dimmed, with a list only for cards off the
  table?
- **No needless questions**: is the player never asked a question with one legal
  answer, or whose answers are indistinguishable (identical triggers or tokens)?
  Expected: the client answers and logs it; a "may" still asks.
- **Defaults**: is the likely answer the primary button, one key away (keep,
  lethal damage in order, the auto-tapper's plan)? Is there one key for pass?
- **Inspect while deciding**: can the player enlarge any card, open any zone and
  read the log with a prompt open? Fails: XMage and Forge dialogs covering the
  board they ask about.

### Starting a game and mulligans

- **Play or draw** (`playOrDraw`): "You won the coin flip: play or draw?" with
  Play primary; when the AI chooses, is its choice announced, not just enacted?
- **Keep** (`keepOrMulligan`): is the hand readable without hovering, with the
  mulligan count and the size you'd keep shown? Are the AI's mulligans shown?
- **Bottom** (`putOnBottom`): "Choose 2 cards to put on the bottom", selections
  toggle, confirm enabled only at exactly 2, and the kept hand the right size.

### Priority and passing

- **Where are we**: are turn, active player, step and priority holder always
  visible? Expected: a phase tracker that marks the step and whose turn it is.
- **Pass says where** (`priority`): does the pass button name where it leads?
  Expected: "Resolve Lightning Bolt" with a stack (resolving only the top item,
  then stopping again), else "To combat", "To end of turn", "To your turn".
- **Playable highlight** (`priority`): are exactly the cards and permanents
  usable now lit, none lacking mana, timing or targets? Is a second land refused
  with the reason?
- **One card, several uses** (`priority`): casting or cycling, two faces,
  flashback, several abilities: a short menu names each with its cost; a card
  with one use acts on one click.
- **Smart stops**: does the client skip steps where the player can do nothing,
  yet always stop when the opponent casts, activates or triggers something they
  could answer, and after attackers? Fails: MTGO's F6 passing through the
  opponent's spells; Arena skipping the opponent's end step unless set to stop.
  Is a stop-everywhere mode one visible toggle away?
- **Floating mana**: is unspent mana shown by colour, with one warning before a
  pass empties it?

### Casting and paying costs

- **Ways to cast** (`priority` cast with face and costs): does each way show its
  cost? Expected: Force of Will offers "{3}{U}{U}" and "Exile a blue card, pay 1
  life". Choosing how to cast (kicked or not, which face, which cost) and any X
  happen in one step, shown with the card, not as a menu followed by more
  prompts. Can it be cancelled until paid?
- **Free casts** (`castOffer`): is the card shown large, with its zone and the
  waived cost stated ("Cast Ancestral Recall from exile without paying its
  cost?")?
- **X** (`x`): is X capped at what the player can pay, with the total shown ("X
  = 3: pay {3}{R}"), typed numbers accepted?
- **Target count** (`targetCount`): for "any number" or "up to N" targets, is
  the count asked plainly ("How many targets for Fireball?")?
- **Auto-tapper** (`paymentPlan`): before committing, are the sources that will
  pay outlined? Expected: a plan keeping colours open for the rest of the hand,
  sparing painlands and sacrifice sources; one click accepts; another plan or
  manual tapping available. Fails: Arena tapping the wrong dual and stranding
  the second spell.
- **Manual payment** (`manaPayment` mana and pool): can the player tap chosen
  sources or spend floating mana, the remaining cost shrinking ("{1}{U} left")?
- **Life** (`yesNo`, purpose `pay-life`): "Pay 2 life to have Hallowed Fountain
  enter untapped?" with No saying "enters tapped"; Phyrexian mana as "{U} or 2
  life"; current life shown.
- **Waterbend** (`waterbend`): are artifacts and creatures that can tap toward
  the generic cost lit, each tap lowering the remaining cost shown?
- **Crew** (`crew`): "Crew 3: 2 of 3 power", untapped creatures eligible,
  summoning-sick ones included (crewing isn't their {T} ability).

### Targeting

- **Targets** (`target`, `player`): are legal targets lit (creatures,
  planeswalkers, players by portrait or life total) and illegal ones (hexproof,
  protection, wrong type) dimmed, the prompt saying which target of how many?
- **Off the table** (`target`): for graveyard or exile targets, does a browser
  show only legal cards, labelled by owner?

### The stack and responses

- **Stack**: once paid, is the item on the stack at once, top marked, showing
  card, controller, X, mode and arrows to its targets? Do abilities show their
  source and text, not just the source's name?
- **Opponent's spells**: when the AI casts, does the client stop with the spell
  readable and its targets shown, nothing resolving during an animation?
- **Pay or be countered** (`yesNo`, purpose `unless-pays`): "Pay {2} for Mana
  Leak?", untapped mana shown, Pay disabled when unaffordable.
- **Countered**: is a countered spell seen to leave as Arena shows it
  (crumbling in place, before the spell that countered it), logged with what
  countered it, or why it fizzled?
- **Resolving choices**: when a resolving spell asks something, is the spell
  still shown by the prompt, though off the stack?

### Triggers

- **Order** (`triggerOrder`): asked only when several of the player's triggers
  start at once and order matters? Does each show its source and text, with the
  direction explicit ("Choose which resolves first")?
- **"May"** (`yesNo`): does the prompt name the effect ("Return Bloodghast to
  the battlefield?", not "Use ability?")? Can a repeated may be answered for the
  rest of the game (XMage)?
- **Without decisions**: do triggers that ask nothing (Dark Confidant) still
  show on the stack, with their result (the revealed card, the life lost)?

### Combat

- **Attackers** (`attacker`): are eligible attackers lit (untapped, not
  summoning sick unless hasty) and toggled by one click? Expected: "Attack with
  all"; the button counts ("Attack with 3", "No attack").
- **Attack target** (`attackTarget`): with an opposing planeswalker, does each
  attacker choose player or planeswalker, shown by arrows, defaulting to the
  player? No prompt without one.
- **Blockers** (`blocker`, `blockTarget`): are attackers marked with power, and
  blocks made by picking a blocker then its attacker (or dragging)? Expected:
  several blockers per attacker; pairs shown by arrows; "No blocks" in one
  click; unblocked damage totalled ("You will take 7"), warning when lethal.
- **Restrictions**: are impossible blocks not offered (flying), and an illegal
  whole assignment explained (menace with one blocker), not silently refused?
- **Dividing damage** (`damageDivision`, `lethal`, `number`): with several
  blockers or trample, does the client propose lethal to each blocker in turn
  and the rest to the player, counting marked damage and deathtouch? Asked only
  when there's a real choice.
- **Damage**: are first-strike and regular damage shown apart, marked damage
  visible until cleanup, and combat markers cleared afterwards?

### Permanent state

- **Tapped and sick**: tapped cards rotated and still readable; summoning-sick
  creatures marked until their controller's next turn; neither by colour alone.
- **P/T**: modified power and toughness distinguished from printed, raised from
  lowered, marked damage shown.
- **Counters**: each kind with its count (+1/+1, -1/-1, loyalty prominent, time,
  lore), changes animated with the delta?
- **Links**: auras and equipment shown with their host, the link visible from
  either card; cards exiled by a permanent (Oblivion Ring) reachable from it and
  marked to return when it leaves?
- **Changes**: are tokens marked, identical ones grouped with a count? Does an
  animated land show P/T only while animated? Does a stolen permanent sit on its
  controller's side, marked with its owner?
- **Legend rule** (`legendRule`): "Choose which Jace to keep", both shown?
- **Inspect**: does any card (either side, the stack, emblems) enlarge to full
  text, current P/T, counters and attachments?
- **Crowding**: with 15+ permanents a side, are lands grouped apart, identical
  ones stacked, nothing hidden under anything else?

### Hidden and public zones

- **Counts**: are both players' library, hand, graveyard and exile counts always
  visible? Is the hand readable without hover, its order stable, 10 cards fit?
- **Browsing**: does either graveyard or exile open in one click, in order, and
  close without losing an open decision?
- **Library looks** (purposes `scry`, `look`, `put-back`, `fateseal`, `explore`):
  does each say whose library, how many cards and where each goes? Expected:
  scry defaults to all on top; Brainstorm's put-back says which ends on top;
  fateseal says it's the opponent's library; a search shows only matching cards.
- **Discard** (purposes `discard`, `cleanup`): chosen from the hand in place; the
  cleanup prompt says "Discard down to 7: choose 1".
- **Choose objects** (`chooseObject`, `chooseUpTo`): does the prompt name the
  effect and the count, with "choose none" labelled for "up to"? Do untap
  prompts ("Untap up to 3 lands", purpose `untap`) preselect tapped lands?

### Game log

- **Complete**: casts with targets, resolutions, counters, triggers, damage and
  life changes with their source, zone moves, draws, shuffles, coin flips,
  mulligans and the opponent's choices, yet its draws only as counts?
- **Readable**: turn and step headers, players named as on the table, card names
  enlarging on hover; following new entries unless scrolled up?

### The opponent's turn and AI activity

- **Pacing**: are AI actions shown one at a time, each long enough to read
  (lands tapped, then the spell; attackers, then the block prompt)?
- **Its choices**: are the AI's targets, modes, X, attack targets, blocks and
  discards visible as made?

### End of the game

- **Result**: a clear banner with the result and why ("You win: opponent's life
  reached 0", "Draw: the game stalled at the turn limit")?
- **Afterwards**: do the final board, zones and log stay inspectable? Is a new
  game one click away, with nothing left over from the old one?

### General feedback

- **Latency**: every click acknowledged within about 100 ms; engine replies to
  the player's actions within about 200 ms.
- **Animation**: Arena's motion and timing at the same moment (the
  `arena-video` skill), never blocking input or hiding the result.
- **Stillness**: nothing moves that didn't change: hand order, permanent
  positions and panels stay put; a prompt or stack item doesn't reflow the
  board; an enlarged card overlays rather than shifts the layout.
- **Refusals explained**: clicking an unusable card says why ("Not enough
  mana"), never nothing.
- **Window size**: from the smallest supported window up, nothing clipped and
  text legible?
