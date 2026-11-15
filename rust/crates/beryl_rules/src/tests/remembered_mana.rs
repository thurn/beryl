//! Mana that remembers ([CR 106.6]): "Some spells or abilities that produce mana restrict how
//! that mana can be spent, have an additional effect that affects the spell or ability that mana
//! is spent on ... This doesn't affect the mana's type."
//!
//! [CR 106.6]: https://yawgatog.com/resources/magic-rules/#R1066

use beryl_card::{
    ChosenKind, Governed, ManaRestriction, Objects, Players, RuleChange, Subtype, When,
};
use beryl_kernel::chosen::ChosenValue;

use crate::card::{
    Ability, CardDef, Face, FaceIx, ManaColor, ManaCost, ManaSet, Replacement, Types,
};
use crate::event::Sink;
use crate::mana::{self, Paying};
use crate::{Bitboard, CardTable, GameTable, Opt, Profile, Replay, State};

use super::begin;
use super::cast::index_of;
use super::chosen::{GOBLIN, LAND, decide, to_hand};

static CANT_BE_COUNTERED: RuleChange = RuleChange {
    players: Players::EachPlayer,
    what: Governed::Counter(Objects::This),
    when: When::Never,
};

static CHOSEN_CREATURES: ManaRestriction = ManaRestriction {
    cast: beryl_card::Filter::Types(Types::CREATURE),
    of_chosen_type: true,
    rider: Some(&CANT_BE_COUNTERED),
};

/// "As this land enters, choose a creature type. {T}: Add one mana of any color. Spend this mana
/// only to cast a creature spell of the chosen type, and that spell can't be countered."
static CAVERN: CardDef = CardDef {
    name: "Test Restricted Cavern",
    faces: &[Face {
        types: Types::LAND,
        abilities: &[
            Ability::replacing(&Replacement::choose_as_enters(ChosenKind::CreatureType)),
            Ability::tap_for_mana(ManaSet {
                colors: 0b1_1111,
                amount: 1,
            })
            .spent_only(&CHOSEN_CREATURES),
        ],
        ..Face::EMPTY
    }],
    tokens: &[],
};

static CARDS: CardTable = CardTable::new(&[&CAVERN, &GOBLIN, &LAND]);

const DECK: &[(u8, &str)] = &[
    (4, "Test Restricted Cavern"),
    (4, "Test Goblin"),
    (32, "Test Plain Land"),
];

/// Mana a restricted ability makes floats as mana that remembers, apart from the pool's six
/// counts: it pays for a creature spell of the type its source chose, and for nothing else, and
/// the pool empties of it as of any mana.
#[test]
fn mana_that_remembers_pays_only_what_its_restriction_accepts() {
    let t = GameTable::new(&CARDS, [DECK, DECK]).unwrap();
    let mut s = State::new(&t, 5, Profile::EXACT).unwrap();
    begin(&t, &mut s);
    let p = s.turn().starting_player();
    let mut x = crate::Scratch::new();
    let cavern = to_hand(&t, &mut s, p, "Test Restricted Cavern");
    let goblin = to_hand(&t, &mut s, p, "Test Goblin");
    let d = decide(&t, &mut s, &mut x, &mut Replay);
    crate::apply(
        &mut s,
        &t,
        &mut x,
        &mut Replay,
        index_of(&d, Opt::PlayLand(cavern)).unwrap(),
    );
    let d = decide(&t, &mut s, &mut x, &mut Replay);
    let (number, _) = (t.subtypes().creature_types())
        .find(|&(_, subtype)| subtype == Subtype::Goblin)
        .unwrap();
    let chosen =
        Opt::Number(ChosenValue::<crate::layout::Small>::CreatureType(Some(number)).number());
    crate::apply(
        &mut s,
        &t,
        &mut x,
        &mut Replay,
        index_of(&d, chosen).unwrap(),
    );

    // Tapped at priority for {R}, as a player may (CR 605.3a).
    mana::activate(&mut s, &t, &mut Sink::off(), cavern, ManaColor::Red).unwrap();
    assert!(s.object(cavern).is_tapped());
    assert_eq!(s.player(p).mana().amounts(), [0; 6], "no unrestricted mana");
    let remembered = s.player(p).remembered_mana();
    assert_eq!(remembered.len(), 1);
    assert_eq!(remembered[0].color, ManaColor::Red);

    let red = ManaCost::new(0, [0, 0, 0, 1, 0, 0]);
    let blue = ManaCost::new(0, [0, 1, 0, 0, 0, 0]);
    let none = Bitboard::EMPTY;
    let for_goblin = Paying::casting(&s, &t, goblin, FaceIx::FRONT);
    assert!(mana::payable(&s, &t, p, red, none, for_goblin).unwrap());
    assert!(
        !mana::payable(&s, &t, p, blue, none, for_goblin).unwrap(),
        "its kind stays"
    );
    assert!(
        !mana::payable(&s, &t, p, red, none, Paying::NOTHING).unwrap(),
        "no spell"
    );
    let for_land = Paying::casting(&s, &t, cavern, FaceIx::FRONT);
    assert!(
        !mana::payable(&s, &t, p, red, none, for_land).unwrap(),
        "not a creature"
    );

    s.empty_mana_pools();
    assert!(s.player(p).remembered_mana().is_empty());
}
