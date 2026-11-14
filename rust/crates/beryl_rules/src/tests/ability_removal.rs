//! Ability-removing effects, colors, base power and toughness, and switching, in their
//! layers ([CR 613.1]): "all applicable continuous effects are applied in a series of layers
//! in the following order". And an effect with parts in several layers, which keeps applying
//! once it has begun ([CR 613.6]).
//!
//! [CR 613.1]: https://yawgatog.com/resources/magic-rules/#R6131
//! [CR 613.6]: https://yawgatog.com/resources/magic-rules/#R6136

use beryl_card::{AbilityIx, Amount, Colors, Keywords, ManaColor, ManaSet, Objects, Trigger};

use crate::card::{Ability, CardDef, Face, LayerSpec, Modification, Types};
use crate::event::EventKind;
use crate::filter::Filter;
use crate::fixed::Full;
use crate::ids::{Seat, Slot};
use crate::state::{Continuous, Duration, LayerChange, TurnStep, Zone};
use crate::{CardTable, GameTable, Profile, State, layers, reference};

use super::LAND;
use super::events::find;

static CREATURES: Objects = Objects::All {
    zone: Zone::Battlefield,
    filter: Filter::Types(Types::CREATURE),
};

static ONE: Amount = Amount::Fixed(1);
static FOUR: Amount = Amount::Fixed(4);

/// An enchantment's face with these abilities.
const fn enchantment(abilities: &'static [Ability]) -> Face {
    Face {
        types: Types::ENCHANTMENT,
        abilities,
        ..Face::EMPTY
    }
}

/// "All creatures lose all abilities and have base power and toughness 1/1": one effect, in
/// layers 6 and 7b.
static HUMBLE: CardDef = CardDef {
    name: "Test Humble",
    faces: &[enchantment(&[Ability::static_ability(&LayerSpec {
        affected: CREATURES,
        condition: None,
        modifications: &[
            Modification::LoseAllAbilities,
            Modification::SetBasePt(&ONE, &ONE),
        ],
    })])],
    tokens: &[],
};

/// "Enchantments are creatures in addition to their other types and have base power and
/// toughness 4/4": one effect, in layers 4 and 7b.
static OPALESCENT: CardDef = CardDef {
    name: "Test Opalescent",
    faces: &[enchantment(&[Ability::static_ability(&LayerSpec {
        affected: Objects::All {
            zone: Zone::Battlefield,
            filter: Filter::Types(Types::ENCHANTMENT),
        },
        condition: None,
        modifications: &[
            Modification::AddTypes(Types::CREATURE),
            Modification::SetBasePt(&FOUR, &FOUR),
        ],
    })])],
    tokens: &[],
};

/// "Creatures lose all abilities", in layer 6 alone.
static PLAIN: CardDef = CardDef {
    name: "Test Plain",
    faces: &[enchantment(&[Ability::static_ability(&LayerSpec {
        affected: CREATURES,
        condition: None,
        modifications: &[Modification::LoseAllAbilities],
    })])],
    tokens: &[],
};

/// A 2/2 flier that taps for {W}, triggers as it enters, and gets +1/+1 from its own static
/// ability, a 3/3: each kind of ability Humility removes.
static KNIGHT: CardDef = CardDef {
    name: "Test Knight",
    faces: &[Face {
        types: Types::CREATURE,
        power: Some(2),
        toughness: Some(2),
        keywords: Keywords::FLYING,
        abilities: &[
            Ability::tap_for_mana(ManaSet::one(ManaColor::White)),
            Ability::triggered(&Trigger::ENTERS),
            Ability::static_ability(&LayerSpec {
                affected: Objects::This,
                condition: None,
                modifications: &[Modification::ModifyPt(1, 1)],
            }),
        ],
        ..Face::EMPTY
    }],
    tokens: &[],
};

/// A white 1/3 that is blue instead, gains black, loses its own second ability, and has its
/// power and toughness switched: "This creature is blue, and black in addition to its other
/// colors. It loses its other ability and has its power and toughness switched."
static ODDITY: CardDef = CardDef {
    name: "Test Oddity",
    faces: &[Face {
        types: Types::CREATURE,
        power: Some(1),
        toughness: Some(3),
        colors: Colors::WHITE,
        abilities: &[
            Ability::static_ability(&LayerSpec {
                affected: Objects::This,
                condition: None,
                modifications: &[
                    Modification::SetColors(Colors::BLUE),
                    Modification::AddColors(Colors::BLACK),
                    Modification::LoseAbility(AbilityIx::new(1)),
                    Modification::SwitchPt,
                ],
            }),
            Ability::tap_for_mana(ManaSet::one(ManaColor::White)),
        ],
        ..Face::EMPTY
    }],
    tokens: &[],
};

/// A creature of seventeen abilities, more than a [`crate::layout::Small`] mask names.
static HYDRA: CardDef = CardDef {
    name: "Test Hydra",
    faces: &[Face {
        types: Types::CREATURE,
        power: Some(1),
        toughness: Some(1),
        abilities: &[Ability::tap_for_mana(ManaSet::one(ManaColor::White)); 17],
        ..Face::EMPTY
    }],
    tokens: &[],
};

static CARDS: CardTable = CardTable::new(&[
    &LAND,
    &HUMBLE,
    &OPALESCENT,
    &PLAIN,
    &KNIGHT,
    &ODDITY,
    &HYDRA,
]);

/// A game in the first player's main phase, each deck holding two of each card.
fn game() -> (GameTable, State) {
    let deck: &[(u8, &str)] = &[
        (2, "Test Humble"),
        (2, "Test Opalescent"),
        (2, "Test Plain"),
        (2, "Test Knight"),
        (2, "Test Oddity"),
        (2, "Test Hydra"),
        (8, "Test Land"),
    ];
    let t = GameTable::new(&CARDS, [deck, deck]).unwrap();
    let mut s = State::new(&t, 3, Profile::EXACT).unwrap();
    s.board().begin_at(1, Seat::P0, TurnStep::PrecombatMain);
    (t, s)
}

/// Puts `player`'s `card` onto the battlefield, with a timestamp after every permanent's
/// before it, and refreshes the layers, checking them against a recompute from scratch.
fn enters(t: &GameTable, s: &mut State, player: Seat, card: &str) -> Slot {
    let slot = find(s, t, player, card);
    s.board().place(slot, Zone::Battlefield).unwrap();
    refresh(t, s);
    slot
}

fn refresh(t: &GameTable, s: &mut State) {
    layers::refresh(s, t, &mut crate::event::Sink::off()).unwrap();
    reference::check_references(s, t).unwrap();
}

fn pt(s: &State, slot: Slot) -> (i8, i8) {
    let d = s.derived(slot);
    (d.power, d.toughness)
}

/// Starts a resolved effect of `change` on the permanent in `slot`, as if from its own card,
/// with the next timestamp.
fn start(t: &GameTable, s: &mut State, slot: Slot, change: LayerChange) {
    let (card, _) = s.copiable(slot).record(t);
    let on = s.object_ref(slot);
    let effect = Continuous::Layer(on, change);
    s.start_effect(card, effect, Duration::Indefinite).unwrap();
    refresh(t, s);
}

/// A creature that loses all its abilities has none of any kind: its keywords, its mana
/// ability, its triggered ability and its static ability are gone ([CR 613.1f]), and the
/// layers say so to every reader through [`State::has_ability`] and the mask of what the
/// battlefield listens to. Its base power and toughness are 1/1 ([CR 613.4b]).
///
/// [CR 613.1f]: https://yawgatog.com/resources/magic-rules/#R6131f
/// [CR 613.4b]: https://yawgatog.com/resources/magic-rules/#R6134b
#[test]
fn a_creature_that_loses_all_abilities_has_none() {
    let (t, mut s) = game();
    let knight = enters(&t, &mut s, Seat::P0, "Test Knight");
    assert_eq!(pt(&s, knight), (3, 3));
    assert!(s.has_ability(knight, AbilityIx::new(0)));
    assert!(s.listening().unwrap().hears(EventKind::Enters));
    let humble = enters(&t, &mut s, Seat::P0, "Test Humble");
    assert_eq!(pt(&s, knight), (1, 1));
    assert!(!s.derived(knight).keywords.contains(Keywords::FLYING));
    for n in 0..3 {
        assert!(!s.has_ability(knight, AbilityIx::new(n)));
    }
    assert!(!s.has_every_ability(knight));
    // Humility is no creature: it keeps its own ability.
    assert!(s.has_ability(humble, AbilityIx::new(0)));
    assert!(!s.listening().unwrap().hears(EventKind::Enters));
}

/// An effect that grants an ability is applied before or after an ability-removing effect by
/// timestamp ([CR 613.7]): "An effect with an earlier timestamp is applied before an effect
/// with a later timestamp." So a keyword granted before "loses all abilities" is lost, and
/// one granted after it stays (Dress Down's ruling: "If an effect grants a creature an
/// ability after Dress Down has entered the battlefield, it won't lose that ability.").
///
/// [CR 613.7]: https://yawgatog.com/resources/magic-rules/#R6137
#[test]
fn a_grant_after_the_removal_stays_and_one_before_it_is_lost() {
    let (t, mut s) = game();
    let knight = enters(&t, &mut s, Seat::P0, "Test Knight");
    start(
        &t,
        &mut s,
        knight,
        LayerChange::AddKeywords(Keywords::VIGILANCE),
    );
    enters(&t, &mut s, Seat::P0, "Test Plain");
    let keywords = s.derived(knight).keywords;
    assert!(!keywords.contains(Keywords::VIGILANCE));
    assert!(!keywords.contains(Keywords::FLYING));
    start(
        &t,
        &mut s,
        knight,
        LayerChange::AddKeywords(Keywords::FIRST_STRIKE),
    );
    assert!(s.derived(knight).keywords.contains(Keywords::FIRST_STRIKE));
    // Dress Down sets no power: the Knight is its printed 2/2, its own +1/+1 gone.
    assert_eq!(pt(&s, knight), (2, 2));
}

/// Humility and Opalescence, as Humility's ruling has them: "If Humility has the latest
/// timestamp, then all creatures are 1/1 with no abilities. ... If Humility has the earliest
/// timestamp, then everything is 4/4." Opalescence's effect began in layer 4, so its 7b part
/// applies though Humility removed its ability in layer 6 ([CR 613.6]), and the two 7b parts
/// apply in timestamp order.
///
/// [CR 613.6]: https://yawgatog.com/resources/magic-rules/#R6136
#[test]
fn an_effect_begun_in_an_earlier_layer_keeps_applying_once_its_ability_is_lost() {
    let (t, mut s) = game();
    let opalescent = enters(&t, &mut s, Seat::P0, "Test Opalescent");
    let humble = enters(&t, &mut s, Seat::P0, "Test Humble");
    assert_eq!(pt(&s, opalescent), (1, 1));
    assert_eq!(pt(&s, humble), (1, 1));
    assert!(!s.has_ability(humble, AbilityIx::new(0)));

    let (t, mut s) = game();
    let humble = enters(&t, &mut s, Seat::P0, "Test Humble");
    let opalescent = enters(&t, &mut s, Seat::P0, "Test Opalescent");
    let knight = enters(&t, &mut s, Seat::P0, "Test Knight");
    assert_eq!(pt(&s, opalescent), (4, 4));
    assert_eq!(pt(&s, humble), (4, 4));
    // The Knight is no enchantment: Opalescence's set never held it.
    assert_eq!(pt(&s, knight), (1, 1));
}

/// A creature's static ability lost in layer 6 never begins in layer 7c: the permanent no
/// longer has it when that layer is reached. And a static ability removing one of its own
/// face's abilities, setting and adding colors in layer 5, and switching power and toughness
/// in layer 7d, after the counters of 7c ([CR 613.4d]): "Effects that switch a creature's
/// power and toughness are applied."
///
/// [CR 613.4d]: https://yawgatog.com/resources/magic-rules/#R6134d
#[test]
fn colors_one_lost_ability_and_switching_apply_in_their_layers() {
    let (t, mut s) = game();
    let oddity = enters(&t, &mut s, Seat::P0, "Test Oddity");
    let d = s.derived(oddity);
    assert_eq!(d.colors, Colors::BLUE.union(Colors::BLACK));
    assert_eq!((d.power, d.toughness), (3, 1));
    assert!(s.has_ability(oddity, AbilityIx::new(0)));
    assert!(!s.has_ability(oddity, AbilityIx::new(1)));
    start(&t, &mut s, oddity, LayerChange::ModifyPt(2, 0));
    // 1/3, +2/+0 in layer 7c, switched in 7d.
    assert_eq!(pt(&s, oddity), (3, 3));
    let on = s.object_ref(oddity);
    s.add_counters(on, crate::state::CounterKind::PlusOne, 1, true)
        .unwrap();
    refresh(&t, &mut s);
    assert_eq!(pt(&s, oddity), (4, 4));
}

/// A face with more abilities than the layout's mask can name stalls the recompute when it
/// loses all of them, for a layout with a wider mask to play on.
#[test]
fn losing_more_abilities_than_the_mask_names_stalls() {
    let (t, mut s) = game();
    enters(&t, &mut s, Seat::P0, "Test Hydra");
    let plain = find(&s, &t, Seat::P0, "Test Plain");
    s.board().place(plain, Zone::Battlefield).unwrap();
    let refreshed = layers::refresh(&mut s, &t, &mut crate::event::Sink::off());
    assert_eq!(refreshed, Err(Full));
}
