//! Test-only cards for casting, mana, the stack and resolution, and a harness that sets up
//! positions with them.

// Bodies have the `Flow` signature whether or not they can pause.
#![allow(clippy::unnecessary_wraps)]

use crate::bitboard::Bitboard;
use crate::card::{
    Ability, AbilityKind, CardDef, CastingOption, Cost, Face, Flow, Keywords, ManaColor, ManaCost,
    ManaSet, Rule, Subtype, Subtypes, Supertypes, TargetKind, TargetSpec, Trigger, Types,
};
use crate::effect::EffectCtx;
use crate::event::Reason;
use crate::filter::{Filter, Whose};
use crate::ids::{Seat, Slot};
use crate::state::{Pending, TurnStep, Zone};
use crate::{
    CardTable, Choice, Decision, DecisionKind, Decklist, GameTable, Opt, Profile, Replay, Scratch,
    State, Step, apply, next,
};

use super::{DUAL, LAND, begin};

const fn mana(generic: u8, w: u8, u: u8) -> ManaCost {
    ManaCost::new(generic, [w, u, 0, 0, 0, 0])
}

const CREATURE: Filter = Filter::Types(Types::CREATURE);
const LANDS: Filter = Filter::Types(Types::LAND);

fn destroy_target(ctx: &mut EffectCtx) -> Flow {
    if let Some(target) = ctx.target_object(0) {
        ctx.destroy(target);
    }
    Ok(())
}

fn bounce_target(ctx: &mut EffectCtx) -> Flow {
    if let Some(target) = ctx.target_object(0) {
        ctx.move_to(target, Zone::Hand, Reason::Effect);
    }
    Ok(())
}

/// "Counter target spell unless its controller pays {1}."
fn tithe(ctx: &mut EffectCtx) -> Flow {
    let Some(spell) = ctx.target_object(0) else {
        return Ok(());
    };
    let controller = ctx.spell_controller(spell).expect("a spell on the stack");
    if !ctx.unless_pays(controller, ManaCost::generic(1))? {
        ctx.counter(spell, false);
    }
    Ok(())
}

/// "Counter target spell. Exile it instead of putting it into its owner's graveyard."
fn counter_exile(ctx: &mut EffectCtx) -> Flow {
    if let Some(spell) = ctx.target_object(0) {
        ctx.counter(spell, true);
    }
    Ok(())
}

/// "Draw two cards, then put a card from your hand on top of your library."
fn ponder(ctx: &mut EffectCtx) -> Flow {
    let you = ctx.controller();
    ctx.draw(you, 2);
    let hand = ctx.state().zone(you, Zone::Hand);
    if !hand.is_empty() {
        let card = ctx.choose_object(you, hand)?;
        let card = ctx.state().object_ref(card);
        ctx.move_to(card, Zone::Library, Reason::Effect);
    }
    Ok(())
}

/// "You may cast target instant or sorcery card from your graveyard this turn. If that spell
/// would be put into your graveyard, exile it instead."
fn flashback(ctx: &mut EffectCtx) -> Flow {
    if let Some(card) = ctx.target_object(0) {
        let you = ctx.controller();
        ctx.allow_cast(card, you, 0, true, true);
    }
    Ok(())
}

/// "Look at target opponent's hand. You may exile a nonland card from it. For as long as that
/// card remains exiled, its owner may cast it. A spell cast this way costs {2} more to cast."
fn binder(ctx: &mut EffectCtx) -> Flow {
    let Some(opponent) = ctx.target_player(0) else {
        return Ok(());
    };
    let s = ctx.state();
    let nonland: Bitboard = s
        .zone(opponent, Zone::Hand)
        .iter()
        .filter(|&slot| !LANDS.matches(s, ctx.table, slot, opponent))
        .collect();
    if nonland.is_empty() || !ctx.choose_yes_no(ctx.controller())? {
        return Ok(());
    }
    let card = ctx.choose_object(ctx.controller(), nonland)?;
    ctx.move_to(ctx.state().object_ref(card), Zone::Exile, Reason::Exile);
    let exiled = ctx.state().object_ref(card);
    ctx.allow_cast(exiled, opponent, 2, false, false);
    Ok(())
}

/// "Discard a card. You may cast it without paying its mana cost."
fn loot_and_cast(ctx: &mut EffectCtx) -> Flow {
    let you = ctx.controller();
    let hand = ctx.state().zone(you, Zone::Hand);
    if hand.is_empty() {
        return Ok(());
    }
    let card = ctx.choose_object(you, hand)?;
    ctx.move_to(
        ctx.state().object_ref(card),
        Zone::Graveyard,
        Reason::Discard,
    );
    let discarded = ctx.state().object_ref(card);
    ctx.offer_cast(discarded, you, true);
    Ok(())
}

fn sorceries_as_flash(ctx: &mut EffectCtx) -> Flow {
    let you = ctx.controller();
    ctx.allow_sorceries_as_flash(you);
    Ok(())
}

fn draw(ctx: &mut EffectCtx) -> Flow {
    let you = ctx.controller();
    ctx.draw(you, 1);
    Ok(())
}

pub(crate) static ISLAND: CardDef = CardDef {
    name: "Test Island",
    tokens: &[],
    faces: &[Face {
        supertypes: Supertypes::BASIC,
        types: Types::LAND,
        subtypes: Subtypes::of(&[Subtype::Island]),
        abilities: &[Ability::tap_for_mana(ManaSet::one(ManaColor::Blue))],
        ..Face::EMPTY
    }],
};

pub(crate) static CRYPT: CardDef = CardDef {
    name: "Test Crypt",
    tokens: &[],
    faces: &[Face {
        types: Types::ARTIFACT,
        abilities: &[Ability::tap_for_mana(ManaSet {
            colors: 1 << ManaColor::Colorless as u8,
            amount: 2,
        })],
        ..Face::EMPTY
    }],
};

pub(crate) static STRIP: CardDef = CardDef {
    name: "Test Strip",
    tokens: &[],
    faces: &[Face {
        types: Types::LAND,
        abilities: &[
            Ability::tap_for_mana(ManaSet::one(ManaColor::Colorless)),
            Ability {
                kind: AbilityKind::Activated,
                cost: Cost {
                    tap: true,
                    sacrifice_self: true,
                    ..Cost::NONE
                },
                targets: &[TargetSpec::one(TargetKind::Permanent, LANDS)],
                body: destroy_target,
                ..Ability::NONE
            },
        ],
        ..Face::EMPTY
    }],
};

pub(crate) static HQ: CardDef = CardDef {
    name: "Test HQ",
    tokens: &[],
    faces: &[Face {
        types: Types::LAND,
        abilities: &[
            Ability::tap_for_mana(ManaSet::one(ManaColor::White)),
            Ability::cycling(ManaCost::generic(1)),
        ],
        ..Face::EMPTY
    }],
};

pub(crate) static BEAR: CardDef = CardDef {
    name: "Test Bear",
    tokens: &[],
    faces: &[Face {
        cost: mana(1, 1, 0),
        types: Types::CREATURE,
        power: Some(2),
        toughness: Some(2),
        ..Face::EMPTY
    }],
};

pub(crate) static REMOVAL: CardDef = CardDef {
    name: "Test Removal",
    tokens: &[],
    faces: &[Face {
        cost: mana(1, 1, 0),
        types: Types::INSTANT,
        abilities: &[Ability {
            kind: AbilityKind::Spell,
            targets: &[TargetSpec::one(TargetKind::Permanent, CREATURE)],
            body: destroy_target,
            ..Ability::NONE
        }],
        ..Face::EMPTY
    }],
};

pub(crate) static TITHE: CardDef = CardDef {
    name: "Test Tithe",
    tokens: &[],
    faces: &[Face {
        cost: mana(0, 1, 0),
        types: Types::INSTANT,
        abilities: &[Ability {
            kind: AbilityKind::Spell,
            targets: &[TargetSpec::one(TargetKind::Spell, Filter::Any)],
            may_pause: true,
            body: tithe,
            ..Ability::NONE
        }],
        ..Face::EMPTY
    }],
};

pub(crate) static LIES: CardDef = CardDef {
    name: "Test Lies",
    tokens: &[],
    faces: &[Face {
        cost: mana(0, 0, 1),
        types: Types::INSTANT,
        abilities: &[Ability {
            kind: AbilityKind::Spell,
            targets: &[TargetSpec::one(TargetKind::Spell, Filter::Any)],
            body: counter_exile,
            ..Ability::NONE
        }],
        ..Face::EMPTY
    }],
};

pub(crate) static PONDER: CardDef = CardDef {
    name: "Test Ponder",
    tokens: &[],
    faces: &[Face {
        cost: mana(0, 0, 1),
        types: Types::SORCERY,
        abilities: &[Ability {
            kind: AbilityKind::Spell,
            may_pause: true,
            body: ponder,
            ..Ability::NONE
        }],
        ..Face::EMPTY
    }],
};

pub(crate) static FLASHER: CardDef = CardDef {
    name: "Test Flasher",
    tokens: &[],
    faces: &[Face {
        cost: mana(1, 0, 1),
        types: Types::CREATURE,
        keywords: Keywords::FLASH,
        power: Some(1),
        toughness: Some(1),
        ..Face::EMPTY
    }],
};

pub(crate) static DIVER: CardDef = CardDef {
    name: "Test Diver",
    tokens: &[],
    faces: &[Face {
        cost: mana(1, 0, 1),
        types: Types::CREATURE,
        power: Some(2),
        toughness: Some(1),
        casting: &[CastingOption::Kicker {
            cost: ManaCost::X,
            min_x: 1,
        }],
        ..Face::EMPTY
    }],
};

pub(crate) static TRAINER: CardDef = CardDef {
    name: "Test Trainer",
    tokens: &[],
    faces: &[Face {
        cost: mana(1, 0, 1),
        types: Types::CREATURE,
        power: Some(1),
        toughness: Some(2),
        casting: &[CastingOption::Offspring(ManaCost::generic(1))],
        ..Face::EMPTY
    }],
};

pub(crate) static OVERLORD: CardDef = CardDef {
    name: "Test Overlord",
    tokens: &[],
    faces: &[Face {
        cost: mana(3, 1, 0),
        types: Types::ENCHANTMENT.union(Types::CREATURE),
        power: Some(5),
        toughness: Some(5),
        casting: &[CastingOption::Impending {
            cost: mana(1, 1, 0),
            counters: 4,
        }],
        ..Face::EMPTY
    }],
};

pub(crate) static STUPOR: CardDef = CardDef {
    name: "Test Stupor",
    tokens: &[],
    faces: &[
        Face {
            cost: mana(1, 0, 1),
            types: Types::INSTANT,
            abilities: &[Ability {
                kind: AbilityKind::Spell,
                targets: &[TargetSpec::one(
                    TargetKind::Permanent,
                    Filter::All(&[Filter::Not(&LANDS), Filter::Whose(Whose::Opponent)]),
                )],
                body: bounce_target,
                ..Ability::NONE
            }],
            ..Face::EMPTY
        },
        Face {
            types: Types::LAND,
            abilities: &[Ability::tap_for_mana(ManaSet::one(ManaColor::Blue))],
            ..Face::EMPTY
        },
    ],
};

pub(crate) static RAVELER: CardDef = CardDef {
    name: "Test Raveler",
    tokens: &[],
    faces: &[Face {
        cost: mana(1, 1, 1),
        types: Types::PLANESWALKER,
        loyalty: Some(4),
        abilities: &[
            Ability {
                kind: AbilityKind::Static,
                rule: Rule::OpponentsCastAsSorceries,
                ..Ability::NONE
            },
            Ability {
                kind: AbilityKind::Activated,
                cost: Cost::loyalty(1),
                body: sorceries_as_flash,
                ..Ability::NONE
            },
            Ability {
                kind: AbilityKind::Activated,
                cost: Cost::loyalty(-3),
                targets: &[TargetSpec::up_to_one(TargetKind::Permanent, CREATURE)],
                body: bounce_target,
                ..Ability::NONE
            },
        ],
        ..Face::EMPTY
    }],
};

pub(crate) static EMPEROR: CardDef = CardDef {
    name: "Test Emperor",
    tokens: &[],
    faces: &[Face {
        cost: mana(1, 1, 0),
        types: Types::PLANESWALKER,
        keywords: Keywords::FLASH,
        loyalty: Some(3),
        abilities: &[
            Ability {
                kind: AbilityKind::Static,
                rule: Rule::InstantLoyaltyTheTurnItEntered,
                ..Ability::NONE
            },
            Ability {
                kind: AbilityKind::Activated,
                cost: Cost::loyalty(1),
                body: draw,
                ..Ability::NONE
            },
            Ability {
                kind: AbilityKind::Activated,
                cost: Cost::loyalty(-2),
                targets: &[TargetSpec::one(
                    TargetKind::Permanent,
                    Filter::All(&[CREATURE, Filter::Tapped]),
                )],
                body: destroy_target,
                ..Ability::NONE
            },
        ],
        ..Face::EMPTY
    }],
};

pub(crate) static FLASHBACK: CardDef = CardDef {
    name: "Test Flashback",
    tokens: &[],
    faces: &[Face {
        cost: mana(0, 0, 1),
        types: Types::SORCERY,
        abilities: &[Ability {
            kind: AbilityKind::Spell,
            targets: &[TargetSpec::one(
                TargetKind::GraveyardCard,
                Filter::All(&[
                    Filter::Types(Types::INSTANT.union(Types::SORCERY)),
                    Filter::Whose(Whose::You),
                ]),
            )],
            body: flashback,
            ..Ability::NONE
        }],
        ..Face::EMPTY
    }],
};

pub(crate) static BINDER: CardDef = CardDef {
    name: "Test Binder",
    tokens: &[],
    faces: &[Face {
        cost: mana(0, 1, 0),
        types: Types::SORCERY,
        abilities: &[Ability {
            kind: AbilityKind::Spell,
            targets: &[TargetSpec::one(
                TargetKind::Player,
                Filter::Whose(Whose::Opponent),
            )],
            may_pause: true,
            body: binder,
            ..Ability::NONE
        }],
        ..Face::EMPTY
    }],
};

pub(crate) static LOOTER: CardDef = CardDef {
    name: "Test Looter",
    tokens: &[],
    faces: &[Face {
        cost: mana(0, 0, 1),
        types: Types::INSTANT,
        abilities: &[Ability {
            kind: AbilityKind::Spell,
            may_pause: true,
            body: loot_and_cast,
            ..Ability::NONE
        }],
        ..Face::EMPTY
    }],
};

pub(crate) static STINGER: CardDef = CardDef {
    name: "Test Stinger",
    tokens: &[],
    faces: &[Face {
        cost: mana(0, 1, 0),
        types: Types::CREATURE,
        power: Some(1),
        toughness: Some(1),
        abilities: &[Ability {
            targets: &[TargetSpec::one(
                TargetKind::Permanent,
                Filter::All(&[CREATURE, Filter::Whose(Whose::Opponent)]),
            )],
            ..Ability::triggered(Trigger::Enters, destroy_target)
        }],
        ..Face::EMPTY
    }],
};

pub(crate) static CAST_CARDS: CardTable = CardTable::new(&[
    &LAND, &ISLAND, &DUAL, &CRYPT, &STRIP, &HQ, &BEAR, &REMOVAL, &TITHE, &LIES, &PONDER, &FLASHER,
    &DIVER, &TRAINER, &OVERLORD, &STUPOR, &RAVELER, &EMPEROR, &FLASHBACK, &BINDER, &LOOTER,
    &STINGER,
]);

/// Every test card, two of most, and lands.
pub(crate) const EVERYTHING: &Decklist = &[
    (10, "Test Land"),
    (6, "Test Island"),
    (2, "Test Dual"),
    (1, "Test Crypt"),
    (1, "Test Strip"),
    (1, "Test HQ"),
    (2, "Test Bear"),
    (2, "Test Removal"),
    (2, "Test Tithe"),
    (1, "Test Lies"),
    (2, "Test Ponder"),
    (1, "Test Flasher"),
    (1, "Test Diver"),
    (1, "Test Trainer"),
    (1, "Test Overlord"),
    (1, "Test Stupor"),
    (1, "Test Raveler"),
    (1, "Test Emperor"),
    (1, "Test Flashback"),
    (1, "Test Binder"),
    (1, "Test Looter"),
    (1, "Test Stinger"),
];

/// A game of [`EVERYTHING`] mirrors, with empty hands, set up position by position.
pub(crate) struct Game {
    pub t: GameTable,
    pub s: State,
    pub x: Scratch,
    /// The starting player.
    pub me: Seat,
    pub them: Seat,
}

impl Game {
    pub fn new(profile: Profile) -> Game {
        let t = GameTable::new(&CAST_CARDS, [EVERYTHING, EVERYTHING]).unwrap();
        let mut s = State::new(&t, 5, profile);
        begin(&t, &mut s);
        let me = s.turn().starting_player;
        for p in Seat::BOTH {
            for slot in s.zone(p, Zone::Hand).iter() {
                crate::zone::put_on_bottom(&mut s, slot).unwrap();
            }
        }
        let mut g = Game {
            t,
            s,
            x: Scratch::new(),
            me,
            them: me.opponent(),
        };
        g.main(me);
        g
    }

    /// Puts a card of `player`'s from their library into a zone, returning its slot.
    pub fn give(&mut self, player: Seat, name: &str, zone: Zone) -> Slot {
        let slot = *self
            .s
            .library(player)
            .iter()
            .find(|&&slot| self.t.def(self.s.object(slot).card).name == name)
            .unwrap_or_else(|| panic!("no {name} left in the library"));
        crate::zone::move_to(&mut self.s, slot, zone).unwrap();
        if zone == Zone::Battlefield {
            let loyalty = self.t.def(self.s.object(slot).card).front().loyalty;
            let o = self.s.object_mut(slot);
            o.flags
                .set(crate::state::ObjectFlags::SUMMONING_SICK, false);
            // CR 306.5b: it enters with its printed loyalty.
            o.counters.loyalty = loyalty.unwrap_or(0);
        }
        slot
    }

    /// Puts a card of `player`'s from deeper in their library on top of it, so it is the next
    /// card drawn.
    pub fn on_top(&mut self, player: Seat, name: &str) -> Slot {
        let slot = *self
            .s
            .library(player)
            .iter()
            .find(|&&slot| self.t.def(self.s.object(slot).card).name == name)
            .unwrap_or_else(|| panic!("no {name} left in the library"));
        crate::zone::move_to(&mut self.s, slot, Zone::Library).unwrap();
        slot
    }

    /// Makes it `player`'s precombat main phase with an empty stack and priority.
    pub fn main(&mut self, player: Seat) {
        self.at(player, TurnStep::PrecombatMain);
    }

    /// Makes it `active`'s turn, at `step`, with priority for the active player.
    pub fn at(&mut self, active: Seat, step: TurnStep) {
        let turn = &mut self.s.turn;
        turn.active = active;
        turn.step = step;
        turn.priority = active;
        turn.passes = 0;
    }

    pub fn decide(&mut self) -> Decision {
        match next(&mut self.s, &self.t, &mut self.x, &mut Replay) {
            Step::Decide(d) => d,
            Step::Over(o) => panic!("the game ended: {o:?}"),
        }
    }

    /// Answers the open decision, then settles what follows with a single answer, as `next`
    /// would, but for priority: a pass is left to the test to count.
    pub fn choose(&mut self, c: Choice) {
        apply(&mut self.s, &self.t, &mut self.x, &mut Replay, c);
        while self.s.outcome().is_none() {
            let d = self.current();
            if d.kind == DecisionKind::Priority || d.len() != 1 {
                break;
            }
            apply(&mut self.s, &self.t, &mut self.x, &mut Replay, Choice(0));
        }
    }

    /// The open decision, which must offer `opt`, answered with it.
    pub fn pick(&mut self, opt: Opt) {
        let d = self.decide();
        let c = index_of(&d, opt).unwrap_or_else(|| panic!("{opt:?} not offered in {d:?}"));
        self.choose(c);
    }

    /// The open decision, which must offer object `slot`, answered with it.
    pub fn pick_object(&mut self, slot: Slot) {
        let d = self.decide();
        let c = object_index(&d, slot).unwrap_or_else(|| panic!("{slot} not offered in {d:?}"));
        self.choose(c);
    }

    /// Both players pass priority once, silently or not.
    pub fn pass_both(&mut self) {
        for _ in 0..2 {
            let d = self.current();
            assert_eq!(d.kind, DecisionKind::Priority, "{d:?}");
            self.choose(Choice(0));
        }
    }

    /// The open decision as it stands, without advancing past a silent pass.
    pub fn current(&mut self) -> Decision {
        crate::layers::refresh(&mut self.s, &self.t);
        crate::engine::decision(&self.s, &self.t).unwrap()
    }

    /// Whether the open priority decision offers casting the card in `slot`.
    pub fn offers_cast(&mut self, slot: Slot) -> bool {
        let d = self.current();
        d.kind == DecisionKind::Priority
            && (0..d.len()).any(|i| {
                matches!(d.candidates.option(Choice(i as u8)), Some(Opt::Cast { slot: s, .. }) if s == slot)
            })
    }

    pub fn is_paused(&self) -> bool {
        matches!(self.s.pending, Pending::Resolving(_))
    }
}

pub(crate) fn index_of(d: &Decision, opt: Opt) -> Option<Choice> {
    (0..d.len())
        .map(|i| Choice(i as u8))
        .find(|&c| d.candidates.option(c) == Some(opt))
}

pub(crate) fn object_index(d: &Decision, slot: Slot) -> Option<Choice> {
    (0..d.len())
        .map(|i| Choice(i as u8))
        .find(|&c| d.candidates.object(c) == Some(slot))
}

/// Casting `slot` with its front face and no casting options.
pub(crate) fn cast(slot: Slot) -> Opt {
    Opt::Cast {
        slot,
        face: 0,
        costs: 0,
    }
}
