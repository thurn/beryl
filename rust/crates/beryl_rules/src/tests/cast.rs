//! Test-only cards for casting, mana, the stack and resolution, and a harness that sets up
//! positions with them.

use beryl_card::effect::build::{
    ITS_CONTROLLER, TARGET, YOU, change_rule, counter, counter_and_exile, destroy as destroy_it,
    discard, draw, gain_flashback, permit_cast, return_to_hand, sequence, unless_pays,
};
use beryl_card::{CastTerms, Condition, Effect, Governed, Objects, RuleChange, Says, Takes, When};

use crate::actions::destroy::destroy;
use crate::bitboard::Bitboard;
use crate::card::{
    Ability, CardDef, CastingOption, Castings, Cost, CostPart, Face, FaceIx, Flow, Keywords,
    ManaColor, ManaCost, ManaSet, Players, Prompt, Purpose, Subtype, Subtypes, Supertypes,
    TargetKind, TargetSpec, Trigger, Types,
};
use crate::event::Reason;
use crate::filter::{Evaluate, Filter};
use crate::game;
use crate::ids::{Seat, Slot};
use crate::resolution::Resolution;
use crate::state::{CounterKind, Pending, TargetPlace, TurnStep, Zone, ZoneKey};
use crate::zone::Place;
use crate::{
    CardTable, Choice, Decision, DecisionKind, Decklist, GameTable, Opt, Profile, Replay, Scratch,
    State, Step, apply, next,
};
use crate::{CustomBody, Layout, custom_body};
use crate::{casting, choices, drawing, zones};
use beryl_kernel::two_player::TwoPlayerSeat as _;

use super::{DUAL, LAND, begin};

const fn mana(generic: u8, w: u8, u: u8) -> ManaCost {
    ManaCost::new(generic, [w, u, 0, 0, 0, 0])
}

const CREATURE: Filter = Filter::Types(Types::CREATURE);
const LANDS: Filter = Filter::Types(Types::LAND);

/// "Destroy target creature" as a Rust body: the escape hatch, written against the library
/// and registered with no macro (the crate's contract, "Custom bodies").
struct DestroyTarget;

impl CustomBody for DestroyTarget {
    const SAYS: Says = Says {
        asks: false,
        takes: &[Some(Takes::Removal)],
    };

    fn resolve<L: Layout>(
        game: &mut game::Game<'_, L>,
        resolution: &mut Resolution<'_, L>,
    ) -> Flow {
        if let Some(target) = resolution.target_object(game, TargetPlace::FIRST) {
            destroy(game, target)?;
        }
        Ok(())
    }
}

/// "Counter target spell unless its controller pays {1}."
const COUNTER_UNLESS_PAID: Effect =
    unless_pays(&counter(TARGET), ITS_CONTROLLER, ManaCost::generic(1));

/// A choice of one object, for nothing a purpose names.
const CHOOSE: Prompt = Prompt::new(Purpose::Choose).most(1);

/// "Draw two cards, then put a card from your hand on top of your library."
struct Ponder;

impl CustomBody for Ponder {
    const SAYS: Says = ASKS;

    fn resolve<L: Layout>(
        game: &mut game::Game<'_, L>,
        resolution: &mut Resolution<'_, L>,
    ) -> Flow {
        let you = resolution.controller();
        drawing::draw(game, you, 2)?;
        let hand = game.state().zone(ZoneKey::Of(you, Zone::Hand));
        if !hand.is_empty() {
            let card = choices::pick(game, resolution, you, hand, CHOOSE)?;
            let card = game.state().object_ref(card);
            zones::move_to(game, card, Zone::Library, Reason::Effect)?;
        }
        Ok(())
    }
}

/// A body that may ask and takes no target.
const ASKS: Says = Says {
    asks: true,
    takes: &[],
};

/// "You may cast target instant or sorcery card from your graveyard this turn. If that spell
/// would be put into your graveyard, exile it instead."
const CAST_THIS_TURN: Effect = permit_cast(
    YOU,
    TARGET,
    beryl_card::Duration::EndOfTurn,
    CastTerms::PLAIN.exiled_instead(),
);

/// "Look at target opponent's hand. You may exile a nonland card from it. For as long as that
/// card remains exiled, its owner may cast it. A spell cast this way costs {2} more to cast."
struct Binder;

impl CustomBody for Binder {
    const SAYS: Says = ASKS;

    fn resolve<L: Layout>(
        game: &mut game::Game<'_, L>,
        resolution: &mut Resolution<'_, L>,
    ) -> Flow {
        let Some(opponent) = resolution.target_player(game, TargetPlace::FIRST) else {
            return Ok(());
        };
        let (s, you) = (game.state(), resolution.controller());
        let nonland: Bitboard<L::Slot> = s
            .zone(ZoneKey::Of(opponent, Zone::Hand))
            .filter(|slot| !LANDS.matches(s, game.table(), slot, opponent));
        let may = Prompt::new(Purpose::May);
        if nonland.is_empty() || !choices::yes_no(game, resolution, you, may, Bitboard::EMPTY)? {
            return Ok(());
        }
        let card = choices::pick(game, resolution, you, nonland, CHOOSE)?;
        let card = game.state().object_ref(card);
        zones::move_to(game, card, Zone::Exile, Reason::Exile)?;
        let exiled = game.state().object_ref(card.slot);
        casting::allow(game, resolution, exiled, opponent, 2, false, false)?;
        Ok(())
    }
}

/// "Discard a card. You may cast it without paying its mana cost."
struct LootAndCast;

impl CustomBody for LootAndCast {
    const SAYS: Says = ASKS;

    fn resolve<L: Layout>(
        game: &mut game::Game<'_, L>,
        resolution: &mut Resolution<'_, L>,
    ) -> Flow {
        let you = resolution.controller();
        let hand = game.state().zone(ZoneKey::Of(you, Zone::Hand));
        if hand.is_empty() {
            return Ok(());
        }
        let card = choices::pick(game, resolution, you, hand, CHOOSE)?;
        let card = game.state().object_ref(card);
        zones::move_to(game, card, Zone::Graveyard, Reason::Discard)?;
        let discarded = game.state().object_ref(card.slot);
        casting::offer(resolution, discarded, you, true);
        Ok(())
    }
}

/// "Draw a card, then discard two cards."
const DRAW_THEN_DISCARD_TWO: Effect = sequence(&[draw(YOU, 1), discard(YOU, 2)]);

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

/// A mana creature: "{T}: Add {W}."
pub(crate) static ELF: CardDef = CardDef {
    name: "Test Elf",
    tokens: &[],
    faces: &[Face {
        cost: mana(0, 1, 0),
        types: Types::CREATURE,
        power: Some(1),
        toughness: Some(1),
        abilities: &[Ability::tap_for_mana(ManaSet::one(ManaColor::White))],
        ..Face::EMPTY
    }],
};

/// A sorcery of {U}{U} that draws a card.
pub(crate) static SAGE: CardDef = CardDef {
    name: "Test Sage",
    tokens: &[],
    faces: &[Face {
        cost: mana(0, 0, 2),
        types: Types::SORCERY,
        abilities: &[Ability::spell().effect(&draw(YOU, 1))],
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

/// A Treasure, as a card: "{T}, Sacrifice this artifact: Add one mana of any color."
/// ([CR 111.10a](https://yawgatog.com/resources/magic-rules/#R11110a)).
pub(crate) static TREASURE: CardDef = CardDef {
    name: "Test Treasure",
    tokens: &[],
    faces: &[Face {
        types: Types::ARTIFACT,
        abilities: &[Ability::mana(
            Cost::of_parts(&[CostPart::TAP, CostPart::SACRIFICE]),
            ManaSet {
                colors: 0b1_1111,
                amount: 1,
            },
        )],
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
            Ability::activated(Cost::of_parts(&[CostPart::TAP, CostPart::SACRIFICE]))
                .targeting(&[TargetSpec::one(TargetKind::Permanent, LANDS)])
                .effect(&destroy_it(TARGET)),
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
        abilities: &[Ability::spell()
            .targeting(&[TargetSpec::one(TargetKind::Permanent, CREATURE)])
            .custom(custom_body::<DestroyTarget>())],
        ..Face::EMPTY
    }],
};

pub(crate) static TITHE: CardDef = CardDef {
    name: "Test Tithe",
    tokens: &[],
    faces: &[Face {
        cost: mana(0, 1, 0),
        types: Types::INSTANT,
        abilities: &[Ability::spell()
            .targeting(&[TargetSpec::one(TargetKind::Spell, Filter::Any)])
            .effect(&COUNTER_UNLESS_PAID)],
        ..Face::EMPTY
    }],
};

pub(crate) static LIES: CardDef = CardDef {
    name: "Test Lies",
    tokens: &[],
    faces: &[Face {
        cost: mana(0, 0, 1),
        types: Types::INSTANT,
        abilities: &[Ability::spell()
            .targeting(&[TargetSpec::one(TargetKind::Spell, Filter::Any)])
            .effect(&counter_and_exile(TARGET))],
        ..Face::EMPTY
    }],
};

pub(crate) static PONDER: CardDef = CardDef {
    name: "Test Ponder",
    tokens: &[],
    faces: &[Face {
        cost: mana(0, 0, 1),
        types: Types::SORCERY,
        abilities: &[Ability::spell().custom(custom_body::<Ponder>())],
        ..Face::EMPTY
    }],
};

pub(crate) static RUMMAGE: CardDef = CardDef {
    name: "Test Rummage",
    tokens: &[],
    faces: &[Face {
        cost: mana(0, 0, 0),
        types: Types::SORCERY,
        abilities: &[Ability::spell().effect(&DRAW_THEN_DISCARD_TWO)],
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
            abilities: &[Ability::spell()
                .targeting(&[TargetSpec::one(
                    TargetKind::Permanent,
                    Filter::All(&[Filter::Not(&LANDS), Filter::Whose(&Players::EachOpponent)]),
                )])
                .effect(&return_to_hand(TARGET))],
            ..Face::EMPTY
        },
        Face {
            types: Types::LAND,
            abilities: &[Ability::tap_for_mana(ManaSet::one(ManaColor::Blue))],
            ..Face::EMPTY
        },
    ],
};

/// "Each opponent can cast spells only any time they could cast a sorcery."
static ONLY_AS_SORCERIES: RuleChange = RuleChange {
    players: Players::EachOpponent,
    what: Governed::CastSpells(Filter::Any),
    when: When::OnlyAsSorcery,
};

/// "You may cast sorcery spells as though they had flash."
pub(crate) static SORCERIES_AS_FLASH: RuleChange = RuleChange {
    players: Players::You,
    what: Governed::CastSpells(Filter::Types(Types::SORCERY)),
    when: When::AsInstant,
};

/// "You may activate its loyalty abilities any time you could cast an instant."
static LOYALTY_AS_INSTANT: RuleChange = RuleChange {
    players: Players::You,
    what: Governed::ActivateLoyaltyAbilities(Objects::This),
    when: When::AsInstant,
};

pub(crate) static RAVELER: CardDef = CardDef {
    name: "Test Raveler",
    tokens: &[],
    faces: &[Face {
        cost: mana(1, 1, 1),
        types: Types::PLANESWALKER,
        loyalty: Some(4),
        abilities: &[
            Ability::rule_change(&ONLY_AS_SORCERIES),
            Ability::loyalty(1)
                .starting(&SORCERIES_AS_FLASH)
                .effect(&change_rule(
                    &SORCERIES_AS_FLASH,
                    beryl_card::Duration::UntilYourNextTurn,
                )),
            Ability::loyalty(-3)
                .targeting(&[TargetSpec::up_to_one(TargetKind::Permanent, CREATURE)])
                .effect(&return_to_hand(TARGET)),
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
            Ability::rule_change(&LOYALTY_AS_INSTANT)
                .only_if(&Condition::EnteredThisTurn(Objects::This)),
            Ability::loyalty(1).effect(&draw(YOU, 1)),
            Ability::loyalty(-2)
                .targeting(&[TargetSpec::one(
                    TargetKind::Permanent,
                    Filter::All(&[CREATURE, Filter::Tapped]),
                )])
                .effect(&destroy_it(TARGET)),
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
        abilities: &[Ability::spell()
            .targeting(&[TargetSpec::one(
                TargetKind::GraveyardCard,
                Filter::All(&[
                    Filter::Types(Types::INSTANT.union(Types::SORCERY)),
                    Filter::Whose(&Players::You),
                ]),
            )])
            .effect(&CAST_THIS_TURN)],
        ..Face::EMPTY
    }],
};

pub(crate) static SNAPBACK: CardDef = CardDef {
    name: "Test Snapback",
    tokens: &[],
    faces: &[Face {
        cost: mana(0, 0, 1),
        types: Types::INSTANT,
        abilities: &[Ability::spell()
            .targeting(&[TargetSpec::one(
                TargetKind::GraveyardCard,
                Filter::All(&[
                    Filter::Types(Types::INSTANT.union(Types::SORCERY)),
                    Filter::Whose(&Players::You),
                ]),
            )])
            .effect(&gain_flashback(TARGET))],
        ..Face::EMPTY
    }],
};

pub(crate) static BOUNCE: CardDef = CardDef {
    name: "Test Bounce",
    tokens: &[],
    faces: &[Face {
        cost: mana(0, 0, 1),
        types: Types::INSTANT,
        abilities: &[Ability::spell()
            .targeting(&[TargetSpec::one(TargetKind::Spell, Filter::Any)])
            .effect(&return_to_hand(TARGET))],
        ..Face::EMPTY
    }],
};

pub(crate) static BINDER: CardDef = CardDef {
    name: "Test Binder",
    tokens: &[],
    faces: &[Face {
        cost: mana(0, 1, 0),
        types: Types::SORCERY,
        abilities: &[Ability::spell()
            .targeting(&[TargetSpec::one(
                TargetKind::Player,
                Filter::Whose(&Players::EachOpponent),
            )])
            .custom(custom_body::<Binder>())],
        ..Face::EMPTY
    }],
};

pub(crate) static LOOTER: CardDef = CardDef {
    name: "Test Looter",
    tokens: &[],
    faces: &[Face {
        cost: mana(0, 0, 1),
        types: Types::INSTANT,
        abilities: &[Ability::spell().custom(custom_body::<LootAndCast>())],
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
        abilities: &[Ability::triggered(&Trigger::ENTERS)
            .effect(&destroy_it(TARGET))
            .targeting(&[TargetSpec::one(
                TargetKind::Permanent,
                Filter::All(&[CREATURE, Filter::Whose(&Players::EachOpponent)]),
            )])],
        ..Face::EMPTY
    }],
};

pub(crate) static CAST_CARDS: CardTable = CardTable::new(&[
    &LAND, &ISLAND, &DUAL, &CRYPT, &TREASURE, &STRIP, &HQ, &BEAR, &REMOVAL, &TITHE, &LIES, &PONDER,
    &FLASHER, &DIVER, &TRAINER, &OVERLORD, &STUPOR, &RAVELER, &EMPEROR, &FLASHBACK, &BINDER,
    &LOOTER, &STINGER, &SNAPBACK, &BOUNCE, &ELF, &SAGE, &RUMMAGE,
]);

/// Every test card, two of most, and lands.
pub(crate) const EVERYTHING: &Decklist = &[
    (10, "Test Land"),
    (6, "Test Island"),
    (2, "Test Dual"),
    (1, "Test Crypt"),
    (2, "Test Treasure"),
    (1, "Test Strip"),
    (1, "Test HQ"),
    (2, "Test Bear"),
    (2, "Test Removal"),
    (2, "Test Tithe"),
    (2, "Test Lies"),
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
    (1, "Test Snapback"),
    (1, "Test Bounce"),
    (1, "Test Elf"),
    (1, "Test Sage"),
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
        Game::of(EVERYTHING, profile)
    }

    /// A game of `deck` mirrors, with empty hands.
    pub fn of(deck: &Decklist, profile: Profile) -> Game {
        let t = GameTable::new(&CAST_CARDS, [deck, deck]).unwrap();
        let mut s = State::new(&t, 5, profile).unwrap();
        begin(&t, &mut s);
        let me = s.turn().starting_player();
        for p in Seat::BOTH {
            // Any order: the test puts what it needs back on top.
            for slot in s.in_candidate_order(s.zone(ZoneKey::Of(p, Zone::Hand))) {
                s.move_object(
                    &mut crate::event::Sink::off(),
                    slot,
                    Zone::Library,
                    Place::Bottom,
                )
                .unwrap();
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
            .find(|&&slot| self.s.definition_name(&self.t, slot) == name)
            .unwrap_or_else(|| panic!("no {name} left in the library"));
        self.s
            .move_object(&mut crate::event::Sink::off(), slot, zone, Place::Top)
            .unwrap();
        if zone == Zone::Battlefield {
            let loyalty = self.s.copiable(slot).face(&self.t).loyalty;
            let mut board = self.s.board();
            board.set_sick(slot, false);
            // CR 306.5b: it enters with its printed loyalty.
            board
                .set_counters(slot, CounterKind::Loyalty, loyalty.unwrap_or(0))
                .unwrap();
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
            .find(|&&slot| self.s.definition_name(&self.t, slot) == name)
            .unwrap_or_else(|| panic!("no {name} left in the library"));
        self.s
            .move_object(
                &mut crate::event::Sink::off(),
                slot,
                Zone::Library,
                Place::Top,
            )
            .unwrap();
        slot
    }

    /// Makes it `player`'s precombat main phase with an empty stack and priority.
    pub fn main(&mut self, player: Seat) {
        self.at(player, TurnStep::PrecombatMain);
    }

    /// Makes it `active`'s turn, at `step`, with priority for the active player.
    pub fn at(&mut self, active: Seat, step: TurnStep) {
        let mut board = self.s.board();
        board.set_active(active);
        board.set_step(step);
        board.set_priority(active);
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
        crate::layers::refresh(&mut self.s, &self.t, &mut self.x.events).unwrap();
        crate::engine::decision(&self.s, &self.t, &mut self.x.events).unwrap()
    }

    /// Whether the open priority decision offers casting the card in `slot`.
    pub fn offers_cast(&mut self, slot: Slot) -> bool {
        let d = self.current();
        d.kind == DecisionKind::Priority
            && (0..d.len()).any(|i| {
                matches!(d.candidates.option(Choice(i as u16)), Some(Opt::Cast { slot: s, .. }) if s == slot)
            })
    }

    pub fn is_paused(&self) -> bool {
        matches!(self.s.pending(), Pending::Resolving(_))
    }
}

pub(crate) fn index_of(d: &Decision, opt: Opt) -> Option<Choice> {
    (0..d.len())
        .map(|i| Choice(i as u16))
        .find(|&c| d.candidates.option(c) == Some(opt))
}

pub(crate) fn object_index(d: &Decision, slot: Slot) -> Option<Choice> {
    (0..d.len())
        .map(|i| Choice(i as u16))
        .find(|&c| d.candidates.object(c) == Some(slot))
}

/// Casting `slot` with its front face and no casting options.
pub(crate) fn cast(slot: Slot) -> Opt {
    Opt::Cast {
        slot,
        face: FaceIx::FRONT,
        costs: Castings::NONE,
    }
}
