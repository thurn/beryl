//! The present-cost benchmark: what a family of cards costs a rollout while one of them is in
//! play (`docs/benchmarking.md`, "The present-cost gate").
//!
//! A family is a few cards swapped into [`WU_CONTROL`] ([`Family`], [`FAMILIES`]). Its
//! positions are cut from seeded games between greedy agents playing that deck, each with a
//! card of the family on the battlefield, on the stack or the source of an effect in force
//! ([`FamilyWorkload::new`]). Each position is rolled out under the rollout policy, and every
//! step of every rollout is paired: from the state S the rollout is at, [`control`] builds S',
//! the same state with the family taken out, and the rollout's choice is applied to both and
//! the game advanced to the next decision, each in a function of its own
//! ([`present_step`], [`control_step`]), which is what Callgrind counts. The rollout goes on
//! from S; S' is dropped. Rollouts with and without the family would play different games, and
//! a game with more combat or more passes moves the instructions per decision by more than a
//! mechanism does; a step from one state isolates the mechanism.
//!
//! The control replaces each permanent and spell of the family with its stand-in
//! ([`stand_in`]), the same card with no static or triggered ability and no keyword, and ends
//! every effect a card of the family created. A card of the family in a hand, a library or a
//! graveyard stays: no ability of these cards works there.

use std::hint::black_box;

use beryl_cards::CARDS;
use beryl_cards::decks::WU_CONTROL;
use beryl_cards::names;
use beryl_engine::card::{Ability, AbilityIx, AbilityKind, CardDef, CastingOption, Face, Keywords};
use beryl_engine::{
    CardIx, Choice, Decision, Format, GameTable, Observation, Outcome, Profile, Replay, Scratch,
    StallReason, State, Step, Stops, Zone, apply, apply_decided, next, observe, rederive,
};
use beryl_search::{Agent, AgentDecision, GreedyAgent, Inline, RolloutPolicy};
use beryl_session::save::Chosen;

use crate::rollout::SEED;

/// A family: cards swapped into [`WU_CONTROL`], each `(count, card, replaced)`: `count` copies of
/// `card` in place of as many of `replaced`, so the deck keeps its size.
#[derive(Clone, Copy, Debug)]
pub struct Family {
    /// The name `just count-gate --family` takes and `count-gate.toml` keys its bar by.
    pub name: &'static str,
    pub swaps: &'static [(u8, &'static str, &'static str)],
}

/// Every family with a corpus. Adding one: an entry here, its id on both benchmarks in
/// `benches/family_counts.rs`, and its bar in `count-gate.toml` (`docs/benchmarking.md`).
pub const FAMILIES: &[Family] = &[
    // The control of the method: Tundra has no static or triggered ability, so its stand-in
    // is the same card under another index and the family must measure 1.00.
    Family {
        name: "blank",
        swaps: &[(1, names::TUNDRA, names::PLAINS)],
    },
    // The two cards whose static abilities the layers apply: an Aura that changes the
    // enchanted permanent's types and grants it crew, and an Equipment that gives +1/+1.
    Family {
        name: "attachments",
        swaps: &[
            (1, names::SWIFT_RECONFIGURATION, names::STOCK_UP),
            (1, names::SWIFT_RECONFIGURATION, names::THIRST_FOR_DISCOVERY),
            (1, names::SWIFT_RECONFIGURATION, names::WINDS_OF_ABANDON),
            (1, names::LEONIN_SCIMITAR, names::CURRENCY_CONVERTER),
            (1, names::LEONIN_SCIMITAR, names::STAFF_OF_THE_STORYTELLER),
            (1, names::LEONIN_SCIMITAR, names::FRACTURED_IDENTITY),
        ],
    },
    // The cards whose effects need every layer: Blood Moon (type changing, layer 4),
    // Humility (abilities and power and toughness, layers 6 and 7b), Glorious Anthem
    // (layer 7c) and Tarmogoyf (a characteristic-defining ability reading graveyards,
    // layer 7a). Mountains and Forests stand in for basics so Blood Moon and Tarmogoyf
    // can be cast.
    Family {
        name: "layers",
        swaps: &[
            (1, names::BLOOD_MOON, names::REPRIEVE),
            (1, names::HUMILITY, names::OUST),
            (1, names::GLORIOUS_ANTHEM, names::PORTABLE_HOLE),
            (1, names::GLORIOUS_ANTHEM, names::NO_MORE_LIES),
            (1, names::TARMOGOYF, names::COUNTERSPELL),
            (1, names::TARMOGOYF, names::DAMN),
            (2, names::MOUNTAIN, names::PLAINS),
            (2, names::FOREST, names::ISLAND),
        ],
    },
    // The replacement pipeline's cards (CR 614, 616): Rest in Peace (graveyard to exile),
    // Chains of Mephistopheles (draws, with a choice of order against other replacements),
    // Containment Priest (creatures that were not cast) and Furnace of Rath (damage doubled).
    Family {
        name: "replacement",
        swaps: &[
            (1, names::REST_IN_PEACE, names::REPRIEVE),
            (1, names::CHAINS_OF_MEPHISTOPHELES, names::OUST),
            (1, names::CONTAINMENT_PRIEST, names::PORTABLE_HOLE),
            (1, names::FURNACE_OF_RATH, names::NO_MORE_LIES),
        ],
    },
    // The costs family: Trinisphere (a floor on a spell's total cost, CR 601.2f), Thalia (a
    // static increase on noncreature spells), Pithing Needle (a prohibition keyed on a chosen
    // name) and Cavern of Souls (mana that remembers what it may pay for). Cost rules are read
    // at every priority for every card in hand.
    Family {
        name: "costs",
        swaps: &[
            (1, names::TRINISPHERE, names::CURRENCY_CONVERTER),
            (1, names::THALIA_GUARDIAN_OF_THRABEN, names::TRINKET_MAGE),
            (1, names::PITHING_NEEDLE, names::STAFF_OF_THE_STORYTELLER),
            (2, names::CAVERN_OF_SOULS, names::PLAINS),
        ],
    },
    // The every-zone and text family: Painter's Servant (a color added to every card in
    // every zone, CR 613.1e), Mind Bend and Crystal Spray (a text-changing effect on a
    // permanent or spell, CR 612) and Underworld Breach (casting from the graveyard, with
    // an escape cost). Mountains stand in for Plains so Breach can be cast.
    Family {
        name: "zones-and-text",
        swaps: &[
            (1, names::PAINTERS_SERVANT, names::CURRENCY_CONVERTER),
            (1, names::MIND_BEND, names::STAFF_OF_THE_STORYTELLER),
            (1, names::CRYSTAL_SPRAY, names::THIRST_FOR_DISCOVERY),
            (1, names::UNDERWORLD_BREACH, names::WINDS_OF_ABANDON),
            (2, names::MOUNTAIN, names::PLAINS),
        ],
    },
    // The copies family: Phantasmal Image and Quicksilver Gargantuan (a permanent that enters as
    // a copy of a creature, with a rewrite of entering), Thespian's Stage (a land that becomes
    // a copy of another land) and Fork (a copy of a spell on the stack). Copies are
    // permanents whose characteristics come from another object's through layer 1.
    Family {
        name: "copies",
        swaps: &[
            (1, names::PHANTASMAL_IMAGE, names::REPRIEVE),
            (1, names::PHANTASMAL_IMAGE, names::OUST),
            (1, names::QUICKSILVER_GARGANTUAN, names::PORTABLE_HOLE),
            (1, names::THESPIANS_STAGE, names::TRINKET_MAGE),
            (1, names::FORK, names::NO_MORE_LIES),
        ],
    },
    // The stack and identity family: Animate Dead (an Aura that enters from a graveyard and
    // reanimates), Gemrazer (a creature that mutates onto another, making a merged permanent,
    // CR 729 and 730), Teferi's Protection (phases out every permanent its controller has,
    // CR 702.26) and Stifle (a counterspell for an ability on the stack, an object that is not
    // a spell). Swamps and Forests stand in for basics so Animate Dead and Gemrazer can be cast.
    // Its positions also keep a merged permanent or a phased-out permanent (`in_play`), which
    // the control cannot undo and so leaves as they are.
    Family {
        name: "identity",
        swaps: &[
            (1, names::ANIMATE_DEAD, names::REPRIEVE),
            (1, names::ANIMATE_DEAD, names::OUST),
            (1, names::GEMRAZER, names::PORTABLE_HOLE),
            (1, names::TEFERIS_PROTECTION, names::CURRENCY_CONVERTER),
            (1, names::STIFLE, names::NO_MORE_LIES),
            (2, names::SWAMP, names::PLAINS),
            (2, names::FOREST, names::ISLAND),
        ],
    },
    // The card-shape family: cards with more than one face or a copiable shape the engine
    // reads for every card. Fire // Ice (a split card with combined characteristics off the
    // stack), Wear // Tear (a fuse card), Bonecrusher Giant (an Adventure), Valakut Awakening
    // (a modal double-faced card) and Delver of Secrets (a transforming card). Mountains
    // stand in for Plains so the red halves can be cast.
    Family {
        name: "two-faced",
        swaps: &[
            (1, names::FIRE_ICE, names::REPRIEVE),
            (1, names::WEAR_TEAR, names::OUST),
            (1, names::BONECRUSHER_GIANT, names::PORTABLE_HOLE),
            (1, names::VALAKUT_AWAKENING, names::TRINKET_MAGE),
            (1, names::DELVER_OF_SECRETS, names::NO_MORE_LIES),
            (2, names::MOUNTAIN, names::PLAINS),
        ],
    },
    // The level family: The Eldest Reborn (a Saga, with chapters), Wizard Class and Case of the
    // Crimson Pulse (permanents whose abilities appear as a level or solve condition is
    // reached) and Student of Warfare (a creature with level up). Mountains stand in for
    // Plains so the red cards can be cast.
    Family {
        name: "levels",
        swaps: &[
            (1, names::THE_ELDEST_REBORN, names::REPRIEVE),
            (1, names::WIZARD_CLASS, names::OUST),
            (1, names::CASE_OF_THE_CRIMSON_PULSE, names::PORTABLE_HOLE),
            (1, names::STUDENT_OF_WARFARE, names::NO_MORE_LIES),
            (2, names::MOUNTAIN, names::PLAINS),
        ],
    },
    // The face-down family: Exalted Angel (morph), Gathan Raiders (morph with a hellbent
    // bonus) and Illusionary Mask (a creature put onto the battlefield face down).
    Family {
        name: "face-down",
        swaps: &[
            (1, names::EXALTED_ANGEL, names::REPRIEVE),
            (1, names::GATHAN_RAIDERS, names::OUST),
            (1, names::ILLUSIONARY_MASK, names::STAFF_OF_THE_STORYTELLER),
        ],
    },
    // The outside-the-game family: Burning Wish (a sideboard card to hand) and Lurrus of the
    // Dream-Den (a companion with a graveyard permission).
    Family {
        name: "outside",
        swaps: &[
            (1, names::BURNING_WISH, names::STAFF_OF_THE_STORYTELLER),
            (
                1,
                names::LURRUS_OF_THE_DREAM_DEN,
                names::THIRST_FOR_DISCOVERY,
            ),
            (2, names::MOUNTAIN, names::PLAINS),
        ],
    },
];

/// The family called `name`.
pub fn family(name: &str) -> Option<&'static Family> {
    FAMILIES.iter().find(|f| f.name == name)
}

/// Positions a family's corpus keeps, at most.
pub const POSITIONS: usize = 12;

/// Games a family's corpus is cut from, at most, if fewer positions turn up.
pub const MAX_GAMES: u64 = 48;

/// Positions one game gives, at most: the first decision with a card of the family in play,
/// often the turn it arrives, then the first decision of every [`EVERY`]th turn after while
/// one is.
pub const PER_GAME: usize = 3;

/// Turns between the positions one game gives.
pub const EVERY: u16 = 3;

/// Rollouts per position, each from a copy reseeded so they diverge.
pub const ROLLOUTS: u64 = 2;

impl Family {
    /// [`WU_CONTROL`] with the family's cards swapped in.
    pub fn deck(&self) -> Vec<(u8, &'static str)> {
        let mut deck: Vec<(u8, &'static str)> = WU_CONTROL.to_vec();
        for &(count, card, replaced) in self.swaps {
            let out = deck
                .iter_mut()
                .find(|(_, name)| *name == replaced)
                .unwrap_or_else(|| panic!("{replaced} is not in the WU deck"));
            out.0 = out
                .0
                .checked_sub(count)
                .unwrap_or_else(|| panic!("the WU deck has too few {replaced}"));
            match deck.iter_mut().find(|(_, name)| *name == card) {
                Some(entry) => entry.0 += count,
                None => deck.push((count, card)),
            }
        }
        deck.retain(|&(count, _)| count > 0);
        deck
    }

    /// The family's distinct cards.
    pub fn cards(&self) -> Vec<&'static CardDef> {
        let mut defs: Vec<&'static CardDef> = Vec::new();
        for &(_, card, _) in self.swaps {
            let def = CARDS.get(card).unwrap_or_else(|| panic!("no card {card}"));
            if !defs.iter().any(|d| std::ptr::eq(*d, def)) {
                defs.push(def);
            }
        }
        defs
    }
}

/// `def` with every static and triggered ability and every keyword taken off each face: a card
/// of the same name, cost, types and size that does nothing while it is a permanent. Its spell,
/// activated and mana abilities stay, so a spell of it resolves as one of `def` would, and an
/// activation of it is the same choice in a save's terms (an activated ability is named by its
/// place among those, `beryl_session::save`). Leaked: a game table holds `'static` cards.
pub fn stand_in(def: &'static CardDef) -> &'static CardDef {
    let faces: Vec<Face> = def
        .faces
        .iter()
        .map(|face| {
            let kept = |a: &&Ability| {
                !matches!(
                    a.kind(),
                    AbilityKind::Static | AbilityKind::Replacement | AbilityKind::Triggered
                )
            };
            // A casting option (overload, mutate) names the face's ability it adds by index,
            // and dropping abilities moves that index: count the kept ones before it.
            let moved = |ability: AbilityIx| {
                let before = face.abilities[..ability.index()]
                    .iter()
                    .filter(kept)
                    .count();
                AbilityIx::new(u8::try_from(before).expect("a face has few abilities"))
            };
            let casting: Vec<CastingOption> = face
                .casting
                .iter()
                .map(|&option| match option {
                    CastingOption::Overload { cost, ability } => CastingOption::Overload {
                        cost,
                        ability: moved(ability),
                    },
                    CastingOption::Mutate { cost, ability } => CastingOption::Mutate {
                        cost,
                        ability: moved(ability),
                    },
                    other => other,
                })
                .collect();
            let abilities: Vec<_> = face.abilities.iter().filter(kept).copied().collect();
            Face {
                keywords: Keywords::NONE,
                abilities: abilities.leak(),
                casting: casting.leak(),
                ..*face
            }
        })
        .collect();
    Box::leak(Box::new(CardDef {
        name: def.name,
        shape: def.shape,
        faces: faces.leak(),
        tokens: def.tokens,
    }))
}

/// A family in one game table: each of its cards' game index beside its stand-in's.
#[derive(Clone, Debug)]
pub struct InTable {
    pub pairs: Vec<(CardIx, CardIx)>,
    /// Whether merged and phased-out permanents make a position (the identity family).
    pub identity: bool,
}

impl InTable {
    /// The family's deck, both seats, in a table that also holds the stand-ins.
    pub fn table(family: &Family) -> (GameTable, InTable) {
        let deck = family.deck();
        let cards = family.cards();
        let stand_ins: Vec<&'static CardDef> = cards.iter().map(|&def| stand_in(def)).collect();
        let t = GameTable::with_extra(Format::CONSTRUCTED, &CARDS, [&deck, &deck], &stand_ins)
            .expect("the family's deck makes a table");
        let pairs = cards
            .iter()
            .zip(&stand_ins)
            .map(|(&card, &stand_in)| {
                (
                    t.index_of(card).expect("the card is in its deck"),
                    t.index_of(stand_in).expect("the stand-in is an extra"),
                )
            })
            .collect();
        (
            t,
            InTable {
                pairs,
                identity: family.name == "identity",
            },
        )
    }

    fn stand_in_of(&self, card: CardIx) -> Option<CardIx> {
        self.pairs
            .iter()
            .find(|&&(of, _)| of == card)
            .map(|&(_, stand_in)| stand_in)
    }

    /// The permanents and spells in `s` that are cards of the family, each with its stand-in.
    fn objects(&self, s: &State) -> Vec<(beryl_engine::Slot, CardIx)> {
        let mut found = Vec::new();
        let mut look = |slot| {
            let owner = s.object(slot).owner();
            if let Some(stand_in) = Observation::new(s, owner)
                .card(slot)
                .and_then(|card| self.stand_in_of(card))
            {
                found.push((slot, stand_in));
            }
        };
        for (slot, _) in s.permanents_with_derived() {
            look(slot);
        }
        for entry in s.stack() {
            if let Some(spell) = entry.spell()
                && s.object(spell.slot).zone() == Zone::Stack
            {
                look(spell.slot);
            }
        }
        found
    }

    /// Whether a card of the family is in play in `s`: a permanent, a spell, or the source of
    /// an effect in force. For the identity family a merged permanent (one with cards under it)
    /// or a phased-out permanent counts too: they are what its cards make.
    pub fn in_play(&self, s: &State) -> bool {
        !self.objects(s).is_empty()
            || (self.identity
                && (!s.phased_out().is_empty()
                    || s.permanents_with_derived()
                        .any(|(slot, _)| s.pile(slot).next().is_some())))
            || s.effects()
                .iter()
                .any(|e| self.stand_in_of(e.source).is_some())
    }
}

/// The control state: `s` with the family taken out, its layer results current. Each
/// permanent and spell of the family becomes its stand-in, and every effect a card of the
/// family created ends. Nothing else changes. `None` if the recompute had no room.
pub fn control(s: &State, t: &GameTable, family: &InTable) -> Option<State> {
    // A question about which of several legendary permanents to keep is asked of a state with
    // such a group, which the control has no longer if a copy by the family was in it.
    if matches!(s.pending(), beryl_engine::state::Pending::LegendRule { .. })
        && s.effects()
            .iter()
            .any(|e| family.stand_in_of(e.source).is_some())
    {
        return None;
    }
    let mut c = *s;
    let mut board = c.board();
    for &(card, _) in &family.pairs {
        board.end_effects_from(card);
    }
    // After the effects end: a permanent that was a copy by a card of the family is that card
    // again, and is swapped like any other.
    rederive(&mut c, t).ok()?;
    let objects = family.objects(&c);
    let mut board = c.board();
    for (slot, stand_in) in objects {
        board.set_card(slot, stand_in);
    }
    rederive(&mut c, t).ok()?;
    Some(c)
}

/// One game's positions: plays greedy agents with `family`'s deck under smart stops and keeps
/// the state at the first decision with a card of the family in play, then at the first
/// decision of every [`EVERY`]th turn after while one is, [`PER_GAME`] at most.
fn cut(t: &GameTable, family: &InTable, seed: u64, out: &mut Vec<State>) {
    let mut s = State::new(t, seed, Profile::SMART).expect("decks that fit Small");
    let mut x = Scratch::new();
    let mut agents = [GreedyAgent::new(t), GreedyAgent::new(t)];
    let mut asked = [AgentDecision(0); 2];
    let (mut kept, mut next_turn) = (0, 0);
    while kept < PER_GAME {
        let Step::Decide(d) = next(&mut s, t, &mut x, &mut Replay) else {
            return;
        };
        if s.turn().number() >= next_turn && family.in_play(&s) {
            out.push(s);
            kept += 1;
            next_turn = s.turn().number() + EVERY;
        }
        let i = d.decider.index();
        let c = agents[i].choose(observe(&s, t, &mut x, d.decider), &d, asked[i].take());
        apply(&mut s, t, &mut x, &mut Replay, c);
    }
}

/// A family's table, positions and rollout starts, built before measuring.
pub struct FamilyWorkload {
    pub family: &'static Family,
    pub table: GameTable,
    pub in_table: InTable,
    pub policy: RolloutPolicy,
    pub positions: Vec<State>,
    pub starts: Vec<State>,
}

impl FamilyWorkload {
    /// Plays the family's seeded games, cuts its positions and reseeds its starts.
    /// Deterministic: `family_corpora_never_change` pins it.
    pub fn new(name: &str) -> FamilyWorkload {
        let family = family(name).unwrap_or_else(|| panic!("no family {name}"));
        let (table, in_table) = InTable::table(family);
        let mut positions = Vec::new();
        for game in 0..MAX_GAMES {
            if positions.len() >= POSITIONS {
                break;
            }
            cut(&table, &in_table, SEED ^ (game << 1), &mut positions);
        }
        positions.truncate(POSITIONS);
        let mut starts = Vec::new();
        for (i, position) in positions.iter().enumerate() {
            for r in 0..ROLLOUTS {
                let mut s = *position;
                s.reseed(SEED ^ ((i as u64) << 8 | r));
                s.set_profile(Profile {
                    stops: Stops::Smart,
                    breakpoint: None,
                    full_control: None,
                    asks_targets: None,
                    taps_mana: None,
                    ..s.profile()
                });
                starts.push(s);
            }
        }
        let policy = RolloutPolicy::new(&table);
        FamilyWorkload {
            family,
            table,
            in_table,
            policy,
            positions,
            starts,
        }
    }
}

/// What the paired rollouts did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Pairing {
    /// Steps counted on both sides.
    pub paired: u64,
    /// Steps left out of both: the control offered no candidate that does what the
    /// rollout's choice did, or its recompute had no room.
    pub skipped: u64,
    /// Rollouts that ended in `Stalled(Capacity)`: a fixed-capacity list filled, which a game
    /// in the `Large` layout would have gone on past. Their steps are in the sums, but a
    /// state of `Small` that cannot go on is not what a `Large` one costs.
    pub capacity_stalls: u64,
    /// `layer_recomputes` the present side's steps made, from the rollout's own counts.
    pub layer_recomputes: u64,
    /// `state_copies` the present side's steps made: the engine's whole-state copies for
    /// transactional replay, so the advances that ran as transactions.
    pub transactions: u64,
}

/// One transition of the rollout, the one the present side counts: `c` applied to `s`, then
/// the game advanced to its next decision. Out of line, so Callgrind counts from its entry
/// (`benches/family_counts.rs`); the constant keeps the linker from folding it into
/// [`control_step`], which has the same body.
#[inline(never)]
pub fn present_step(
    s: &mut State,
    t: &GameTable,
    x: &mut Scratch,
    a: &mut Inline<'_>,
    d: &Decision,
    c: Choice,
) -> Step {
    black_box(0x5052_4553);
    apply_decided(s, t, x, a, d, c);
    next(s, t, x, a)
}

/// [`present_step`] on the control state: the transition the control side counts.
#[inline(never)]
pub fn control_step(
    s: &mut State,
    t: &GameTable,
    x: &mut Scratch,
    a: &mut Inline<'_>,
    d: &Decision,
    c: Choice,
) -> Step {
    black_box(0x434F_4E54);
    apply_decided(s, t, x, a, d, c);
    next(s, t, x, a)
}

/// The control's step for the rollout's choice `c` of `d` in `s`, if the control `sc` offers a
/// candidate that does the same; none, and nothing counted, otherwise.
fn pair(
    w: &FamilyWorkload,
    s: &State,
    d: &Decision,
    c: Choice,
    sc: &mut State,
    xc: &mut Scratch,
    a: &mut Inline<'_>,
) -> bool {
    let t = &w.table;
    let Step::Decide(dc) = next(sc, t, xc, a) else {
        return false;
    };
    let Some(cc) = Chosen::new(t, s, d, c).find(t, sc, &dc) else {
        return false;
    };
    black_box(control_step(sc, t, xc, a, &dc, cc));
    true
}

/// Rolls each start out under the rollout policy, pairing every step with the control's.
pub fn roll_paired(w: &FamilyWorkload) -> Pairing {
    let t = &w.table;
    let mut a = Inline {
        policy: &w.policy,
        t,
    };
    let (mut x, mut xc) = (Scratch::unchecked(), Scratch::unchecked());
    let mut pairing = Pairing::default();
    for start in &w.starts {
        let mut s = *start;
        let mut step = next(&mut s, t, &mut x, &mut a);
        let mut before = x.counts.layer_recomputes;
        let mut copies = x.counts.state_copies;
        while let Step::Decide(d) = step {
            let c = w.policy.choose(t, Observation::new(&s, d.decider), &d);
            let paired = control(&s, t, &w.in_table)
                .is_some_and(|mut sc| pair(w, &s, &d, c, &mut sc, &mut xc, &mut a));
            if paired {
                pairing.paired += 1;
                step = present_step(&mut s, t, &mut x, &mut a, &d, c);
            } else {
                pairing.skipped += 1;
                step = next_unpaired(&mut s, t, &mut x, &mut a, &d, c);
            }
            pairing.layer_recomputes += x.counts.layer_recomputes - before;
            before = x.counts.layer_recomputes;
            pairing.transactions += x.counts.state_copies - copies;
            copies = x.counts.state_copies;
        }
        if matches!(step, Step::Over(Outcome::Stalled(StallReason::Capacity))) {
            pairing.capacity_stalls += 1;
        }
    }
    pairing
}

/// The rollout's transition on a step left out of both sums: [`present_step`]'s work under
/// another name, so neither side counts it.
fn next_unpaired(
    s: &mut State,
    t: &GameTable,
    x: &mut Scratch,
    a: &mut Inline<'_>,
    d: &Decision,
    c: Choice,
) -> Step {
    apply_decided(s, t, x, a, d, c);
    next(s, t, x, a)
}

#[cfg(test)]
mod tests;
