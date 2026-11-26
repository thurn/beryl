//! State-based actions, shuffles, filters and mana abilities on hand-built positions.

use super::{DUAL, LANDS, MIXED, begin, decide, table};
use crate::card::{ManaColor, Players, Purpose, Types};
use crate::filter::{Evaluate, Filter};
use crate::game::Game;
use crate::ids::{CardIx, Seat, Slot};
use crate::replacement::event::Sink;
use crate::resolution::Resolution;
use crate::state::{AbilityRef, PendingTrigger, Zone, ZoneKey};
use crate::zone::Place;
use crate::{Choice, DecisionKind, Decklist, Outcome, Profile, Replay, Scratch, State, apply};
use beryl_kernel::two_player::TwoPlayerSeat as _;

const LEGENDS: &Decklist = &[(40, "Test Legend")];

/// Two legends from the starting player's library onto the battlefield.
fn two_legends(s: &mut State) -> (Seat, Slot, Slot) {
    let p = s.turn().starting_player();
    let lib = s.library(p);
    let (a, b) = (lib[0], lib[1]);
    s.move_object(
        &mut crate::replacement::event::Sink::off(),
        a,
        Zone::Battlefield,
        Place::Top,
    )
    .unwrap();
    s.move_object(
        &mut crate::replacement::event::Sink::off(),
        b,
        Zone::Battlefield,
        Place::Top,
    )
    .unwrap();
    (p, a, b)
}

#[test]
fn the_legend_rule_asks_which_to_keep_when_the_two_differ() {
    let t = table([LEGENDS, LEGENDS]);
    let mut s = State::new(&t, 1, Profile::EXACT).unwrap();
    begin(&t, &mut s);
    let (p, a, b) = two_legends(&mut s);
    s.board().set_tapped(b, true);
    let d = decide(&t, &mut s);
    assert_eq!(
        (d.kind, d.prompt.purpose, d.player, d.len()),
        (DecisionKind::ChooseObject, Purpose::LegendRule, p, 2)
    );
    let keep_b = (0..2)
        .find(|&i| d.candidates.object(Choice(i)) == Some(b))
        .unwrap();
    apply(&mut s, &t, &mut Scratch::new(), &mut Replay, Choice(keep_b));
    let d = decide(&t, &mut s);
    assert_eq!(d.kind, DecisionKind::Priority);
    assert!(s.battlefield().contains(&b));
    assert!(!s.battlefield().contains(&a));
    assert_eq!(s.object(a).zone(), Zone::Graveyard);
    assert_eq!(s.graveyard(p).last(), Some(&a));
}

#[test]
fn the_legend_rule_keeps_one_of_two_interchangeable_legends_silently() {
    let t = table([LEGENDS, LEGENDS]);
    let mut s = State::new(&t, 1, Profile::EXACT).unwrap();
    begin(&t, &mut s);
    let (p, a, b) = two_legends(&mut s);
    assert_eq!(decide(&t, &mut s).kind, DecisionKind::Priority);
    assert_eq!(s.zone(ZoneKey::Of(p, Zone::Battlefield)).count(), 1);
    assert_eq!(s.zone(ZoneKey::Of(p, Zone::Graveyard)).count(), 1);
    assert!(s.battlefield().contains(&a.min(b)));
}

#[test]
fn the_legend_rule_tells_apart_legends_with_different_owners() {
    let t = table([LEGENDS, LEGENDS]);
    let mut s = State::new(&t, 1, Profile::EXACT).unwrap();
    begin(&t, &mut s);
    let p = s.turn().starting_player();
    let mine = s.library(p)[0];
    let stolen = s.library(p.opponent())[0];
    s.move_object(
        &mut crate::replacement::event::Sink::off(),
        mine,
        Zone::Battlefield,
        Place::Top,
    )
    .unwrap();
    s.move_object(
        &mut crate::replacement::event::Sink::off(),
        stolen,
        Zone::Battlefield,
        Place::Top,
    )
    .unwrap();
    s.board().set_controller(stolen, p);
    // The one put into the graveyard goes to its owner's: a different game.
    let d = decide(&t, &mut s);
    assert_eq!(
        (d.kind, d.prompt.purpose, d.player, d.len()),
        (DecisionKind::ChooseObject, Purpose::LegendRule, p, 2)
    );
}

#[test]
fn a_card_put_into_a_hand_is_known_to_its_owner_alone() {
    let t = table([MIXED, MIXED]);
    let mut s = State::new(&t, 3, Profile::EXACT).unwrap();
    begin(&t, &mut s);
    let p = s.turn().starting_player();
    let q = p.opponent();
    let from_library = s.library(p)[0];
    assert!(!s.knows(p, from_library));
    s.move_object(
        &mut crate::replacement::event::Sink::off(),
        from_library,
        Zone::Hand,
        Place::Top,
    )
    .unwrap();
    assert!(s.knows(p, from_library) && !s.knows(q, from_library));
    // A card returned from the battlefield was seen by everyone on its way.
    let from_battlefield = s.library(p)[0];
    s.move_object(
        &mut crate::replacement::event::Sink::off(),
        from_battlefield,
        Zone::Battlefield,
        Place::Top,
    )
    .unwrap();
    s.move_object(
        &mut crate::replacement::event::Sink::off(),
        from_battlefield,
        Zone::Hand,
        Place::Top,
    )
    .unwrap();
    assert!(s.knows(p, from_battlefield) && s.knows(q, from_battlefield));
}

#[test]
fn a_player_at_zero_life_loses_and_two_at_once_draw() {
    let t = table([LANDS, LANDS]);
    for (lives, outcome) in [
        ([0, 20], Outcome::Win(Seat::P1)),
        ([20, 0], Outcome::Win(Seat::P0)),
        ([20, -3], Outcome::Win(Seat::P0)),
        ([0, 0], Outcome::Draw),
    ] {
        let mut s = State::new(&t, 1, Profile::EXACT).unwrap();
        begin(&t, &mut s);
        for (seat, life) in Seat::BOTH.into_iter().zip(lives) {
            s.board().set_life(seat, life);
        }
        assert!(crate::sba::check(&mut s, &t, &mut Sink::off()).unwrap());
        assert_eq!(s.outcome(), Some(outcome));
    }
}

/// The card behind a slot of a deck.
fn card(s: &State, slot: Slot) -> CardIx {
    s.card_behind(slot).expect("a card")
}

#[test]
fn a_shuffle_rebinds_identities_and_clears_knowledge() {
    let t = table([MIXED, MIXED]);
    let mut s = State::new(&t, 9, Profile::EXACT).unwrap();
    begin(&t, &mut s);
    let p = s.turn().starting_player();
    // Any order: every card is checked.
    let hand: Vec<Slot> = crate::tests::fixture_slots(s.zone(ZoneKey::Of(p, Zone::Hand))).collect();
    for &slot in &hand {
        assert!(s.knows(p, slot));
        s.move_object(
            &mut crate::replacement::event::Sink::off(),
            slot,
            Zone::Library,
            Place::Top,
        )
        .unwrap();
    }
    let before: Vec<(Slot, CardIx, u16)> = s
        .library(p)
        .iter()
        .map(|&slot| (slot, card(&s, slot), s.object(slot).incarnation()))
        .collect();
    s.shuffle_library(p).unwrap();
    let after = |slot: Slot| (card(&s, slot), s.object(slot).incarnation());
    let mut cards_before: Vec<CardIx> = before.iter().map(|b| b.1).collect();
    let mut cards_after: Vec<CardIx> = before.iter().map(|b| after(b.0).0).collect();
    assert_ne!(cards_before, cards_after, "identities were dealt anew");
    cards_before.sort_unstable();
    cards_after.sort_unstable();
    assert_eq!(cards_before, cards_after, "the same cards");
    for &(slot, _, incarnation) in &before {
        assert!(!s.knows(Seat::P0, slot) && !s.knows(Seat::P1, slot));
        assert_eq!(after(slot).1, incarnation.wrapping_add(1), "a new object");
    }
    let mut slots: Vec<Slot> = s.library(p).to_vec();
    slots.sort_unstable();
    let mut slots_before: Vec<Slot> = before.iter().map(|b| b.0).collect();
    slots_before.sort_unstable();
    assert_eq!(slots, slots_before);
}

#[test]
fn filters_select_by_type_and_owner() {
    let t = table([MIXED, MIXED]);
    let mut s = State::new(&t, 9, Profile::EXACT).unwrap();
    begin(&t, &mut s);
    let p = s.turn().starting_player();
    let hand = s.zone(ZoneKey::Of(p, Zone::Hand));
    let lands = Filter::Types(Types::LAND).select(&s, &t, hand, p, &crate::card::Words::NONE);
    let others =
        Filter::Not(&Filter::Types(Types::LAND)).select(&s, &t, hand, p, &crate::card::Words::NONE);
    assert_eq!((lands | others), hand);
    assert!((lands & others).is_empty());
    assert!(lands.all(|slot| s.definition_name(&t, slot) == "Test Land"));
    assert_eq!(
        Filter::Whose(&Players::You).select(&s, &t, hand, p, &crate::card::Words::NONE),
        hand
    );
    assert!(
        Filter::Whose(&Players::EachOpponent)
            .select(&s, &t, hand, p, &crate::card::Words::NONE)
            .is_empty()
    );
    assert_eq!(
        Filter::Whose(&Players::EachOpponent).select(
            &s,
            &t,
            hand,
            p.opponent(),
            &crate::card::Words::NONE
        ),
        hand
    );
    // "A player's": whoever reads it.
    for you in [p, p.opponent()] {
        assert_eq!(
            Filter::Whose(&Players::EachPlayer).select(
                &s,
                &t,
                hand,
                you,
                &crate::card::Words::NONE
            ),
            hand
        );
    }
}

#[test]
fn a_dual_land_has_a_mana_ability_for_each_basic_land_type() {
    let t = table([&[(40, "Test Dual")], LANDS]);
    let mut s = State::new(&t, 1, Profile::EXACT).unwrap();
    let source = s.object_ref(Slot::new(0));
    let mut sink = Sink::off();
    for (ability, color) in DUAL
        .front()
        .abilities
        .iter()
        .zip([ManaColor::White, ManaColor::Blue])
    {
        let trigger = PendingTrigger {
            of: AbilityRef {
                source: Some(source),
                ..PendingTrigger::default().of
            },
            ..PendingTrigger::default()
        };
        let mut resolution = Resolution::of_trigger(&trigger, ability);
        resolution.mana = color;
        let mut game = Game::new(&mut s, &t, &mut sink);
        crate::interpret::resolve(&mut game, &mut resolution).unwrap();
    }
    assert_eq!(s.player(Seat::P0).mana().amounts(), [1, 1, 0, 0, 0, 0]);
}

/// Combat requirements, and where the declarations find them
/// ([`crate::continuous::combat_rules::combat_rules_on`]).
mod combat_rules {
    use beryl_card::{
        Ability, CardDef, Cost, Face, Filter, Governed, LayerSpec, Modification, Objects, Players,
        RuleChange, SpecIx, TargetSpec, Types, When,
    };
    use beryl_kernel::format::Format;

    use crate::continuous::combat_rules::{combat_rules_for, combat_rules_on};
    use crate::continuous::{self, layers};
    use crate::ids::{Seat, Slot};
    use crate::replacement::event::Sink;
    use crate::state::{Duration, Rules, TurnStep, Zone};
    use crate::tests::LAND;
    use crate::tests::events::{find, with_game};
    use crate::{CardTable, GameTable, Profile, Scratch, State};

    const fn rule(what: Governed) -> RuleChange {
        RuleChange {
            players: Players::You,
            what,
            when: When::AsUsual,
        }
    }

    /// "All creatures able to block this creature do so."
    static TAUNT: RuleChange = rule(Governed::MustBlock(Objects::This));
    /// "All creatures able to block enchanted creature do so."
    static LURE: RuleChange = rule(Governed::MustBlock(Objects::AttachedTo));
    /// "Target creature must be blocked this turn if able."
    static PREY: RuleChange = rule(Governed::MustBeBlocked(Objects::Target(SpecIx::new(0))));
    static ANY_CREATURE: [TargetSpec; 1] = [TargetSpec::permanent(Filter::Types(Types::CREATURE))];

    const fn card(name: &'static str, faces: &'static [Face]) -> CardDef {
        CardDef {
            name,
            shape: beryl_card::definition::Shape::Single,
            faces,
            tokens: &[],
        }
    }

    const fn creature(abilities: &'static [Ability]) -> Face {
        Face {
            types: Types::CREATURE,
            power: Some(2),
            toughness: Some(2),
            abilities,
            ..Face::EMPTY
        }
    }

    const fn enchantment(abilities: &'static [Ability]) -> Face {
        Face {
            types: Types::ENCHANTMENT,
            abilities,
            ..Face::EMPTY
        }
    }

    static TAUNTER: CardDef = card("Test Taunter", &[creature(&[Ability::rule_change(&TAUNT)])]);
    static BEAR: CardDef = card("Test Bear", &[creature(&[])]);
    /// An enchantment whose rule is about what it is attached to, as an Aura's is.
    static LURING: CardDef = card(
        "Test Luring",
        &[enchantment(&[Ability::rule_change(&LURE)])],
    );
    /// "{0}: Target creature must be blocked this turn if able."
    static HUNTER: CardDef = card(
        "Test Hunter",
        &[creature(&[Ability::activated(Cost::NONE)
            .targeting(&ANY_CREATURE)
            .starting(&PREY)])],
    );
    /// "All creatures lose all abilities."
    static HUMBLE: CardDef = card(
        "Test Humble",
        &[enchantment(&[Ability::static_ability(&LayerSpec {
            affected: Objects::All {
                zone: Zone::Battlefield,
                filter: Filter::Types(Types::CREATURE),
            },
            condition: None,
            modifications: &[Modification::LoseAllAbilities],
        })])],
    );

    /// "No more than one creature can attack each combat."
    static ONE_ATTACKER: RuleChange = RuleChange {
        players: Players::EachPlayer,
        ..rule(Governed::CombatLimit {
            attackers: Some(1),
            blockers: None,
        })
    };
    static ARBITER: CardDef = card(
        "Test Arbiter",
        &[enchantment(&[Ability::rule_change(&ONE_ATTACKER)])],
    );

    static CARDS: CardTable =
        CardTable::new(&[&LAND, &TAUNTER, &BEAR, &LURING, &HUNTER, &HUMBLE, &ARBITER]);

    /// A game in its first main phase with one of each card on its first player's side of
    /// the battlefield, the luring enchantment attached to the first bear.
    fn game() -> (GameTable, State, [Slot; 6]) {
        let deck: &[(u8, &str)] = &[
            (1, "Test Taunter"),
            (2, "Test Bear"),
            (1, "Test Luring"),
            (1, "Test Hunter"),
            (1, "Test Humble"),
            (1, "Test Arbiter"),
            (9, "Test Land"),
        ];
        let t = GameTable::new(Format::CONSTRUCTED, &CARDS, [deck, deck]).unwrap();
        let mut s = State::new(&t, 3, Profile::EXACT).unwrap();
        s.board().begin_at(1, Seat::P0, TurnStep::PrecombatMain);
        let taunter = find(&s, &t, Seat::P0, "Test Taunter");
        s.board().place(taunter, Zone::Battlefield).unwrap();
        let bear = find(&s, &t, Seat::P0, "Test Bear");
        s.board().place(bear, Zone::Battlefield).unwrap();
        let other = find(&s, &t, Seat::P0, "Test Bear");
        s.board().place(other, Zone::Battlefield).unwrap();
        let luring = find(&s, &t, Seat::P0, "Test Luring");
        s.board().place(luring, Zone::Battlefield).unwrap();
        s.board().attach(luring, bear);
        let hunter = find(&s, &t, Seat::P0, "Test Hunter");
        s.board().place(hunter, Zone::Battlefield).unwrap();
        let humble = find(&s, &t, Seat::P0, "Test Humble");
        layers::refresh(&mut s, &t, &mut Sink::off()).unwrap();
        (t, s, [taunter, bear, other, luring, hunter, humble])
    }

    fn only(t: &GameTable, rule: &'static RuleChange) -> Rules {
        Rules::NONE.with(t.rule_id(rule).unwrap())
    }

    /// A creature's own requirement is about it, and an attached permanent's about the
    /// creature it is attached to and no other.
    #[test]
    fn a_combat_rule_is_found_on_its_creature_and_on_the_creature_it_is_attached_to() {
        let (t, s, [taunter, bear, other, luring, hunter, _]) = game();
        assert_eq!(combat_rules_on(&s, &t, taunter), only(&t, &TAUNT));
        assert_eq!(combat_rules_on(&s, &t, bear), only(&t, &LURE));
        assert_eq!(combat_rules_on(&s, &t, other), Rules::NONE);
        assert_eq!(combat_rules_on(&s, &t, luring), Rules::NONE);
        assert_eq!(combat_rules_on(&s, &t, hunter), Rules::NONE);
    }

    /// A rule an effect started about a creature until end of turn is in force this turn,
    /// and gone at the next (see CR 611.2a).
    #[test]
    fn a_combat_rule_an_effect_starts_lasts_as_long_as_it_says() {
        let (t, mut s, [_, _, other, _, hunter, _]) = game();
        let mut x = Scratch::new();
        let permanent = s.permanent_at(other).unwrap();
        with_game(&t, &mut s, &mut x, hunter, |game, resolution| {
            continuous::change_rule_about(game, resolution, &PREY, permanent, Duration::EndOfTurn)
                .unwrap();
        });
        assert_eq!(combat_rules_on(&s, &t, other), only(&t, &PREY));
        assert_eq!(combat_rules_on(&s, &t, hunter), Rules::NONE);
        s.expire(Duration::EndOfTurn);
        layers::refresh(&mut s, &t, &mut x.events).unwrap();
        assert_eq!(combat_rules_on(&s, &t, other), Rules::NONE);
    }

    /// A creature that lost its abilities has no rule of its own (see CR 613.1f).
    #[test]
    fn a_creature_that_lost_its_abilities_has_no_combat_rule_of_its_own() {
        let (t, mut s, [taunter, bear, .., humble]) = game();
        s.board().place(humble, Zone::Battlefield).unwrap();
        layers::refresh(&mut s, &t, &mut Sink::off()).unwrap();
        assert_eq!(combat_rules_on(&s, &t, taunter), Rules::NONE);
        // The enchantment is no creature, so it keeps its rule about the bear.
        assert_eq!(combat_rules_on(&s, &t, bear), only(&t, &LURE));
    }

    /// A limit on attackers for each player is a rule of both players while its permanent is
    /// on the battlefield.
    #[test]
    fn a_combat_limit_is_a_rule_of_each_player() {
        let (t, mut s, _) = game();
        assert_eq!(combat_rules_for(&s, &t, Seat::P1), Rules::NONE);
        let arbiter = find(&s, &t, Seat::P0, "Test Arbiter");
        s.board().place(arbiter, Zone::Battlefield).unwrap();
        layers::refresh(&mut s, &t, &mut Sink::off()).unwrap();
        for player in Seat::BOTH {
            assert_eq!(combat_rules_for(&s, &t, player), only(&t, &ONE_ATTACKER));
        }
    }

    /// A game whose cards have no combat rule asks for none.
    #[test]
    fn a_game_without_combat_rules_finds_none() {
        let t = crate::tests::table([crate::tests::LANDS, crate::tests::LANDS]);
        let mut s = State::new(&t, 1, Profile::EXACT).unwrap();
        crate::tests::begin(&t, &mut s);
        let land = s.library(Seat::P0)[0];
        assert_eq!(combat_rules_on(&s, &t, land), Rules::NONE);
        assert_eq!(combat_rules_for(&s, &t, Seat::P0), Rules::NONE);
    }
}
