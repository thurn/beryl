//! The pay shape's prompts answered by clicks alone: a cost counted in the power
//! of the creatures tapped, a cost that takes permanents tapped in place of mana, mana paid
//! one source at a time, and a payment plan among plans. Each test holds the prompt's state
//! after each click of one worked example to a text written by hand from what the screen
//! shows, and checks that the clicks end where the picks a UI holding the
//! payment itself would send end.

#[path = "common/play.rs"]
mod play;

use beryl_cards::CARDS;
use beryl_cards::names::{
    AANGS_ICEBERG, CURRENCY_CONVERTER, ELITE_SPELLBINDER, ISLAND, JACE_VRYNS_PRODIGY, MANA_CRYPT,
    PLAINS, SNAPCASTER_MAGE, STAFF_OF_THE_STORYTELLER, SWIFT_RECONFIGURATION, THIEVING_SKYDIVER,
    TRINKET_MAGE,
};
use beryl_cards::tokens::TREASURE_TOKEN;
use beryl_engine::card::{Castings, Part, Purpose};
use beryl_engine::layout::Queue as _;
use beryl_engine::{
    Candidates, Choice, Decision, Opt, Payment, Profile, Replay, Scratch, SlotId as _, State, Step,
    TurnStep, apply, next,
};
use beryl_scenario::{P0, P1, Permanent, Scenario};
use beryl_session::prompt::{ButtonId, Click, Clicked, Held};
use beryl_session::protocol::{Candidate, Side, ZoneName};
use beryl_session::replay::Entry;
use beryl_session::save::Kind as SavedKind;
use beryl_session::script::Mark;
use play::{Play, card, cards};

/// The choices of the log from entry `from` on: the kind of decision each answered and
/// whether it continues the one before.
fn choices(play: &Play, from: usize) -> Vec<(SavedKind, bool)> {
    (play.game.replay().log[from..].iter())
        .filter_map(|entry| match entry {
            Entry::Choice(chosen) => Some((chosen.what.as_ref()?.kind, chosen.continuation)),
            _ => None,
        })
        .collect()
}

/// The cards the open prompt marks with `mark`, by slot, ascending.
fn marked(play: &Play, mark: Mark) -> Vec<u16> {
    let mut slots: Vec<u16> = (play.decision().prompt.marks.iter())
        .filter(|marked| marked.mark == mark)
        .filter_map(|marked| match marked.on {
            beryl_session::script::Subject::Object { slot } => Some(slot),
            _ => None,
        })
        .collect();
    slots.sort_unstable();
    slots
}

/// The one permanent of the human's named `name`.
fn permanent(play: &Play, name: &str) -> u16 {
    play.named(name, ZoneName::Battlefield, Side::You)[0]
}

/// Swift Reconfiguration in hand, to make Jace, Vryn's Prodigy a Vehicle with crew 5, and
/// three creatures to crew it: a Snapcaster Mage and a Trinket Mage of power 2 and an Elite
/// Spellbinder of power 3.
const CREW: Scenario = Scenario::new("crew")
    .at(3, P0, TurnStep::PrecombatMain)
    .permanents(
        P0,
        &[
            Permanent::new(JACE_VRYNS_PRODIGY),
            Permanent::new(SNAPCASTER_MAGE),
            Permanent::new(TRINKET_MAGE),
            Permanent::new(ELITE_SPELLBINDER),
            Permanent::new(PLAINS),
        ],
    )
    .hand(P0, &[SWIFT_RECONFIGURATION])
    .library(P0, &[ISLAND; 6])
    .library(P1, &[ISLAND; 6]);

/// Makes Jace a Vehicle and activates its crew ability, to the payment of its cost.
fn crewing() -> Play {
    let mut play = Play::scenario(&CREW);
    let jace = permanent(&play, JACE_VRYNS_PRODIGY);
    play.choose(|c| matches!(c, Candidate::Cast { .. }));
    play.choose(|c| *c == Candidate::Object { slot: jace });
    while !play.decision().prompt.is(Purpose::Crew) {
        let crew = |c: &Candidate| matches!(c, Candidate::Activate { granted: true, .. });
        if play.decision().candidates.iter().any(|c| crew(&c.option)) {
            play.choose(crew);
        } else {
            play.choose(|c| *c == Candidate::Pass);
        }
    }
    play
}

/// A crew cost is paid by picking the creatures to tap: the prompt counts what their power
/// leaves, marks what its default would tap for the rest, the creatures that could not
/// attack before those that could and as little power as covers it, and confirms by its
/// button once the power is met
/// ([CR 702.122a](https://yawgatog.com/resources/magic-rules/#R702122a): "Tap any number of
/// other untapped creatures you control with total power N or greater").
#[test]
fn a_crew_cost_is_paid_by_picking_creatures_and_confirmed() {
    let mut play = crewing();
    let mage = permanent(&play, SNAPCASTER_MAGE);
    let trinket = permanent(&play, TRINKET_MAGE);
    let binder = permanent(&play, ELITE_SPELLBINDER);
    let title = "pay crew: Tap any number of other creatures you control with total power 5 or \
                 more.";
    // The default taps five power exactly, the first of the two Mages as they lie.
    assert_eq!(
        play.prompt(),
        format!(
            "{title}\n\
             pay 5, 5 left\n\
             eligible #{trinket}\n\
             suggested {}\n\
             auto[Auto Pay]* confirm(Submit 0)\n",
            cards(&[mage, binder]),
        )
    );
    // The Vehicle itself pays nothing toward its own crew.
    let jace = permanent(&play, JACE_VRYNS_PRODIGY);
    assert!(play.on(jace).is_some_and(|s| !s.contains("paying")));
    // The Trinket Mage pays two; the Spellbinder alone covers the three left.
    assert_eq!(
        play.on(trinket).as_deref(),
        Some(
            format!(
                "{title}\n\
                 pay 5, 3 left\n\
                 eligible #{mage}\n\
                 suggested #{binder}\n\
                 paying #{trinket}\n\
                 auto[Auto Pay]* confirm(Submit 1)\n"
            )
            .as_str()
        )
    );
    assert_eq!(
        play.on(mage).as_deref(),
        Some(
            format!(
                "{title}\n\
                 pay 5, 1 left\n\
                 suggested #{binder}\n\
                 paying {}\n\
                 auto[Auto Pay]* confirm(Submit 2)\n",
                cards(&[mage, trinket]),
            )
            .as_str()
        )
    );
    // The Spellbinder meets the cost: the button confirms, and nothing more can be added.
    assert_eq!(
        play.on(binder).as_deref(),
        Some(
            format!(
                "{title}\n\
                 pay 5, 0 left\n\
                 paying {}\n\
                 auto[Auto Pay] confirm[Submit 3]*\n",
                cards(&[mage, trinket, binder]),
            )
            .as_str()
        )
    );
    // A pick is taken back by another click: two and three still pay it.
    assert_eq!(
        play.on(mage).as_deref(),
        Some(
            format!(
                "{title}\n\
                 pay 5, 0 left\n\
                 paying {}\n\
                 auto[Auto Pay] confirm[Submit 2]*\n",
                cards(&[trinket, binder]),
            )
            .as_str()
        )
    );
    assert!(play.on(mage).is_some_and(|s| s.contains("Submit 2")));

    // The picks a UI holding the payment sends: each creature as its own answer.
    let mut picked = crewing();
    let from = picked.game.replay().log.len();
    picked.choose(|c| *c == Candidate::Object { slot: trinket });
    picked.choose(|c| *c == Candidate::Object { slot: binder });
    assert_eq!(play.press(ButtonId::Confirm), None, "the button answers");
    assert_eq!(play.game.state(), picked.game.state());
    // One prompt, one answer and its continuation: an undo takes back the payment.
    assert_eq!(
        choices(&play, from)[..2],
        [
            (SavedKind::ChooseObject, false),
            (SavedKind::ChooseObject, true)
        ]
    );
    assert!(play.object(trinket).tapped && play.object(binder).tapped);
    assert!(!play.object(mage).tapped);
}

/// Auto Pay taps what the prompt marks as its default.
#[test]
fn auto_pay_crews_with_what_it_suggests() {
    let mut play = crewing();
    let mage = permanent(&play, SNAPCASTER_MAGE);
    let binder = permanent(&play, ELITE_SPELLBINDER);
    let mut picked = crewing();
    picked.choose(|c| *c == Candidate::Object { slot: mage });
    picked.choose(|c| *c == Candidate::Object { slot: binder });
    assert_eq!(play.press(ButtonId::Auto), None);
    assert_eq!(play.game.state(), picked.game.state());
}

/// Aang's Iceberg, whose ability costs waterbend {3}, with one Plains, and a Staff of the
/// Storyteller and a Jace, Vryn's Prodigy that can be tapped in place of mana.
const BEND: Scenario = Scenario::new("tap in place of mana")
    .at(3, P0, TurnStep::PrecombatMain)
    .permanents(
        P0,
        &[
            Permanent::new(AANGS_ICEBERG),
            Permanent::new(STAFF_OF_THE_STORYTELLER),
            Permanent::new(JACE_VRYNS_PRODIGY),
            Permanent::new(PLAINS),
        ],
    )
    .library(P0, &[ISLAND; 6])
    .library(P1, &[ISLAND; 6]);

/// Activates the Iceberg of `scenario`, to the payment of its cost.
fn bending(scenario: &Scenario) -> Play {
    let mut play = Play::scenario(scenario);
    let iceberg = permanent(&play, AANGS_ICEBERG);
    play.choose(|c| matches!(c, Candidate::Activate { slot, .. } if *slot == iceberg));
    assert!(play.decision().prompt.is(Purpose::Waterbend));
    play
}

/// A cost that takes permanents tapped in place of mana is paid by picking them. Auto Pay
/// taps no artifact or creature the player did not pick, so it waits until mana can pay what
/// the picks leave, and then marks the lands it would tap
/// ([CR 701.67a](https://yawgatog.com/resources/magic-rules/#R70167a): "you may tap an
/// untapped artifact or creature you control rather than pay that mana").
#[test]
fn permanents_are_tapped_in_place_of_mana_and_auto_pay_pays_the_rest() {
    let mut play = bending(&BEND);
    let staff = permanent(&play, STAFF_OF_THE_STORYTELLER);
    let jace = permanent(&play, JACE_VRYNS_PRODIGY);
    let plains = permanent(&play, PLAINS);
    let title = "pay waterbend: Tap permanents to Waterbend";
    // One Plains cannot pay {3}: Auto Pay has nothing to offer.
    let open = format!(
        "{title}\n\
         pay {{3}}, {{3}} left\n\
         eligible {}\n\
         auto(Auto Pay)\n",
        cards(&[staff, jace, plains]),
    );
    assert_eq!(play.prompt(), open);
    assert_eq!(play.press(ButtonId::Auto).as_deref(), Some(open.as_str()));
    assert_eq!(
        play.on(jace).as_deref(),
        Some(
            format!(
                "{title}\n\
                 pay {{3}}, {{2}} left\n\
                 eligible {}\n\
                 paying #{jace}\n\
                 auto(Auto Pay)\n",
                cards(&[staff, plains]),
            )
            .as_str()
        )
    );
    // With the Staff too, the Plains pays what is left.
    let ready = format!(
        "{title}\n\
         pay {{3}}, {{1}} left\n\
         suggested #{plains}\n\
         paying {}\n\
         auto[Auto Pay]*\n",
        cards(&[staff, jace]),
    );
    assert_eq!(play.on(staff).as_deref(), Some(ready.as_str()));
    // A pick is taken back by another click.
    assert!(play.on(staff).is_some_and(|s| s.contains("{2} left")));
    assert_eq!(play.on(staff).as_deref(), Some(ready.as_str()));

    // The picks a UI holding the payment sends: each permanent tapped, the first its answer
    // and the rest that answer's continuations.
    let mut picked = bending(&BEND);
    picked.choose(|c| *c == Candidate::Object { slot: jace });
    picked.follow(|c| *c == Candidate::Object { slot: staff });
    assert_eq!(play.press(ButtonId::Auto), None, "the button answers");
    assert_eq!(play.game.state(), picked.game.state());
    assert_eq!(play.game.replay().log, picked.game.replay().log);

    // The pick that pays the cost in full answers. A land picked pays with its mana, which
    // the engine takes before any permanent is tapped in its place.
    let mut play = bending(&BEND);
    for slot in [jace, staff] {
        assert!(play.on(slot).is_some());
    }
    let mut picked = bending(&BEND);
    picked.choose(|c| matches!(c, Candidate::Mana { slot, .. } if *slot == plains));
    picked.follow(|c| *c == Candidate::Object { slot: jace });
    picked.follow(|c| *c == Candidate::Object { slot: staff });
    assert_eq!(play.on(plains), None, "the last pick answers");
    assert_eq!(play.game.state(), picked.game.state());
    assert_eq!(play.game.replay().log, picked.game.replay().log);
    for slot in [staff, jace, plains] {
        assert!(play.object(slot).tapped);
    }
}

/// The Iceberg with a Mana Crypt, a tapped Currency Converter, a Treasure and three Plains.
const SOURCES: Scenario = Scenario::new("mana sources")
    .at(3, P0, TurnStep::PrecombatMain)
    .permanents(
        P0,
        &[
            Permanent::new(AANGS_ICEBERG),
            Permanent::new(MANA_CRYPT),
            Permanent::new(CURRENCY_CONVERTER).tapped(),
            Permanent::token(TREASURE_TOKEN),
            Permanent::new(PLAINS),
            Permanent::new(PLAINS),
            Permanent::new(PLAINS),
        ],
    )
    .library(P0, &[ISLAND; 6])
    .library(P1, &[ISLAND; 6]);

/// Auto Pay taps lands, and a Treasure in place of mana rather than sacrificing it, but no
/// mana rock the player did not pick; a rock picked pays with its mana.
#[test]
fn auto_pay_spares_what_its_mana_would_use_up_and_asks_for_mana_rocks() {
    let mut play = bending(&SOURCES);
    let rock = permanent(&play, MANA_CRYPT);
    let converter = permanent(&play, CURRENCY_CONVERTER);
    let treasure = permanent(&play, TREASURE_TOKEN);
    let lands = play.named(PLAINS, ZoneName::Battlefield, Side::You);
    // The Treasure and two of the Plains by default; the rock is for the player to pick.
    let suggested = marked(&play, Mark::Suggested);
    assert_eq!(suggested.len(), 3);
    assert!(suggested.contains(&treasure) && !suggested.contains(&rock));
    assert!(marked(&play, Mark::Eligible).contains(&rock));
    assert!(
        !play
            .decision()
            .prompt
            .marks
            .iter()
            .any(|m| m.on == card(converter))
    );
    // The rock's two mana and the Treasure, tapped.
    assert!(play.on(rock).is_some());
    assert_eq!(marked(&play, Mark::Paying), [rock]);
    assert_eq!(marked(&play, Mark::Suggested), [treasure]);
    let state = play.decision().prompt.state.clone();
    assert!(
        matches!(&state, Some(beryl_session::prompt::ShapeView::Pay { cost, left })
            if cost == "{3}" && left == "{1}"),
        "{state:?}"
    );
    // The picks a UI holding the payment sends: the rock's mana, then the Treasure tapped,
    // then the plan that pays the two left with the rock's mana.
    let mut picked = bending(&SOURCES);
    picked.choose(|c| matches!(c, Candidate::Mana { slot, .. } if *slot == rock));
    picked.follow(|c| *c == Candidate::Object { slot: treasure });
    picked.follow(|c| *c == Candidate::Plan { index: 0 });
    assert_eq!(
        play.on(treasure),
        None,
        "the pick that pays the cost answers"
    );
    assert_eq!(play.game.state(), picked.game.state());
    assert_eq!(play.game.replay().log, picked.game.replay().log);
    // Tapped in place of mana, the Treasure is not sacrificed.
    assert!(play.object(treasure).tapped);
    assert!(play.object(rock).tapped);
    assert!(lands.iter().all(|&slot| !play.object(slot).tapped));

    // Two Plains leave one, which the Treasure pays by default; the third Plains pays it.
    let mut play = bending(&SOURCES);
    assert!(play.on(lands[0]).is_some());
    assert!(play.on(lands[1]).is_some());
    assert_eq!(marked(&play, Mark::Paying), lands[..2]);
    assert_eq!(marked(&play, Mark::Suggested), [treasure]);
    assert_eq!(
        play.on(lands[2]),
        None,
        "the pick that pays the cost answers"
    );
    assert!(lands.iter().all(|&slot| play.object(slot).tapped));
    assert!(!play.object(rock).tapped && !play.object(treasure).tapped);
}

/// The Iceberg with one Plains and two Snapcaster Mages, alike.
const ALIKE: Scenario = Scenario::new("two alike")
    .at(3, P0, TurnStep::PrecombatMain)
    .permanents(
        P0,
        &[
            Permanent::new(AANGS_ICEBERG),
            Permanent::new(PLAINS),
            Permanent::new(SNAPCASTER_MAGE),
            Permanent::new(SNAPCASTER_MAGE),
        ],
    )
    .library(P0, &[ISLAND; 6])
    .library(P1, &[ISLAND; 6]);

/// Of two creatures alike the engine offers one for both: either can be picked, and the
/// one picked shows as paying whichever the engine taps for it.
#[test]
fn either_of_two_permanents_alike_can_be_picked_to_pay() {
    let mut play = bending(&ALIKE);
    let mages = play.named(SNAPCASTER_MAGE, ZoneName::Battlefield, Side::You);
    let plains = permanent(&play, PLAINS);
    assert_eq!(marked(&play, Mark::Eligible), [plains, mages[0], mages[1]]);
    assert!(play.on(mages[1]).is_some());
    assert_eq!(marked(&play, Mark::Paying), [mages[1]]);
    assert!(marked(&play, Mark::Suggested).is_empty());
    assert!(play.on(mages[0]).is_some());
    assert_eq!(marked(&play, Mark::Paying), mages);
    assert_eq!(marked(&play, Mark::Suggested), [plains]);
    // The picks a UI holding the payment sends: the Mage the engine offers for the two, then
    // the other.
    let mut picked = bending(&ALIKE);
    picked.choose(|c| *c == Candidate::Object { slot: mages[0] });
    picked.follow(|c| *c == Candidate::Object { slot: mages[1] });
    assert_eq!(play.press(ButtonId::Auto), None);
    assert_eq!(play.game.state(), picked.game.state());
    assert!(mages.iter().all(|&slot| play.object(slot).tapped));
}

/// Thieving Skydiver, {1}{U}, with two Islands and a Mana Crypt: the two Islands pay it, or
/// an Island and the Crypt, which leaves {C} floating.
const PLANS: Scenario = Scenario::new("payment plans")
    .at(3, P0, TurnStep::PrecombatMain)
    .hand(P0, &[THIEVING_SKYDIVER])
    .permanents(
        P0,
        &[
            Permanent::new(ISLAND),
            Permanent::new(ISLAND),
            Permanent::new(MANA_CRYPT),
        ],
    )
    .library(P0, &[PLAINS; 6])
    .library(P1, &[PLAINS; 6]);

/// Casts the Skydiver of [`PLANS`] with no kicker, to the choice of its payment plan.
fn planning() -> Play {
    let mut play = Play::scenario(&PLANS);
    play.choose(|c| matches!(c, Candidate::Cast { costs, .. } if costs.is_empty()));
    assert!(play.decision().prompt.is(Purpose::Mana));
    play
}

/// Where the engine offers payment plans, the player picks the permanents to tap: the first
/// plan is the default, a pick narrows the plans to those that tap it, and the pick that
/// makes one of them answers with it
/// ([CR 601.2g](https://yawgatog.com/resources/magic-rules/#R6012g): "If the total cost
/// includes a mana payment, the player then has a chance to activate mana abilities").
#[test]
fn a_payment_plan_is_chosen_by_picking_what_it_taps() {
    let mut play = planning();
    let islands = play.named(ISLAND, ZoneName::Battlefield, Side::You);
    let crypt = permanent(&play, MANA_CRYPT);
    let title = "pay mana: Pay {1}{U}.";
    assert_eq!(
        play.prompt(),
        format!(
            "{title}\n\
             pay {{1}}{{U}}, {{1}}{{U}} left\n\
             eligible #{crypt}\n\
             suggested {}\n\
             auto[Auto Pay]*\n",
            cards(&islands),
        )
    );
    // The plan with the Crypt taps one Island with it: the one the engine chose.
    let with_crypt = &play.decision().candidates[1].payments[0].taps;
    let island = *(with_crypt.iter())
        .find(|&&slot| slot != crypt)
        .expect("an Island pays {U}");
    assert_eq!(
        play.on(crypt).as_deref(),
        Some(
            format!(
                "{title}\n\
                 pay {{1}}{{U}}, {{1}}{{U}} left\n\
                 suggested #{island}\n\
                 paying #{crypt}\n\
                 auto[Auto Pay]*\n"
            )
            .as_str()
        )
    );
    // The picks a UI holding the payment sends: the plan, as the cast's continuation.
    let mut picked = planning();
    picked.follow(|c| *c == Candidate::Plan { index: 1 });
    assert_eq!(play.on(island), None, "the pick that makes a plan answers");
    assert_eq!(play.game.state(), picked.game.state());
    assert_eq!(play.game.replay().log, picked.game.replay().log);

    // Auto Pay takes the default.
    let mut play = planning();
    let mut picked = planning();
    picked.follow(|c| *c == Candidate::Plan { index: 0 });
    assert_eq!(play.press(ButtonId::Auto), None);
    assert_eq!(play.game.state(), picked.game.state());
    assert_eq!(play.game.replay().log, picked.game.replay().log);
}

/// Thieving Skydiver, {1}{U}, with an Island and two Plains.
const MANUAL: Scenario = Scenario::new("mana by hand")
    .at(3, P0, TurnStep::PrecombatMain)
    .hand(P0, &[THIEVING_SKYDIVER])
    .permanents(
        P0,
        &[
            Permanent::new(ISLAND),
            Permanent::new(PLAINS),
            Permanent::new(PLAINS),
        ],
    )
    .library(P0, &[ISLAND; 6])
    .library(P1, &[ISLAND; 6]);

/// The candidate of `d` that `wanted` names.
fn option(d: &Decision, wanted: impl Fn(Opt) -> bool) -> Choice {
    let Candidates::Options(options) = &d.candidates else {
        panic!("options");
    };
    let at = options.iter().position(|&o| wanted(o)).expect("offered");
    Choice(at as u16)
}

/// [`MANUAL`] under manual payment, the Skydiver cast with no kicker, at the first pick of
/// its payment. No game of the app pays this way yet, so the prompt is held apart from one.
fn by_hand() -> (beryl_engine::GameTable, State, Decision) {
    let (table, mut state) = beryl_scenario::position(&CARDS, &MANUAL).unwrap();
    state.set_profile(Profile {
        payment: Payment::Manual,
        ..state.profile()
    });
    let mut scratch = Scratch::new();
    let Step::Decide(d) = next(&mut state, &table, &mut scratch, &mut Replay) else {
        panic!("a priority");
    };
    let cast = option(
        &d,
        |o| matches!(o, Opt::Cast { costs, .. } if costs == Castings::NONE),
    );
    apply(&mut state, &table, &mut scratch, &mut Replay, cast);
    let Step::Decide(d) = next(&mut state, &table, &mut scratch, &mut Replay) else {
        panic!("a payment");
    };
    assert_eq!(d.prompt.purpose, Purpose::Mana);
    assert_eq!(d.prompt.part, Part::Pick);
    (table, state, d)
}

/// Mana paid one source at a time is paid by picking the lands: the prompt says what each
/// leaves of the cost and what the default would tap for it, and the pick that pays the last
/// of it answers. Of lands alike either can be picked. A land whose mana cannot pay what is
/// left is nothing to pick.
#[test]
fn mana_is_paid_one_source_at_a_time_by_picking_them() {
    let (table, mut state, d) = by_hand();
    let view = play::asked(&table, &state, &d);
    let named = |name: &str| {
        let mut slots: Vec<u16> = (view.objects.iter())
            .filter(|o| o.name.as_deref() == Some(name) && o.zone == ZoneName::Battlefield)
            .map(|o| o.slot)
            .collect();
        slots.sort_unstable();
        slots
    };
    let island = named(ISLAND)[0];
    let plains = named(PLAINS);
    let mut held = Held::opening(&view, &state, &table, Some(&d)).expect("a held prompt");
    let title = "pay mana: Pay {1}{U}.";
    // The Island pays {U} and the first Plains {1} by default.
    assert_eq!(
        held.view().to_string(),
        format!(
            "{title}\n\
             pay {{1}}{{U}}, {{1}}{{U}} left\n\
             eligible #{}\n\
             suggested {}\n\
             auto[Auto Pay]*\n",
            plains[1],
            cards(&[island, plains[0]]),
        )
    );
    let on = |slot: u16| Click::On { on: card(slot) };
    // The second Plains pays {1}: the Island is left to pay {U}, which no Plains can.
    let clicked = held.click(&state, &table, Some(&d), on(plains[1]));
    assert!(matches!(clicked, Clicked::Changed));
    let one = format!(
        "{title}\n\
         pay {{1}}{{U}}, {{U}} left\n\
         suggested #{island}\n\
         paying #{}\n\
         auto[Auto Pay]*\n",
        plains[1],
    );
    assert_eq!(held.view().to_string(), one);
    let clicked = held.click(&state, &table, Some(&d), on(plains[0]));
    assert!(
        matches!(clicked, Clicked::Same),
        "a Plains cannot pay {{U}}"
    );
    assert_eq!(held.view().to_string(), one);
    let Clicked::Answers(mut run) = held.click(&state, &table, Some(&d), on(island)) else {
        panic!("the pick that pays the cost answers");
    };
    // The run's picks, made as their decisions arrive: the Plains the engine offers for the
    // two, for white mana. The Island is then the one source that pays {U}, which the engine
    // takes unasked.
    let mut direct = state;
    let mut scratch = Scratch::new();
    let mut asked = d;
    let mut picks = Vec::new();
    while asked.prompt.purpose == Purpose::Mana {
        let choice = run.answer(&state, &asked).expect("a pick of the run");
        picks.push(asked.candidates.option(choice).expect("an option"));
        apply(&mut state, &table, &mut scratch, &mut Replay, choice);
        let Step::Decide(next) = next(&mut state, &table, &mut scratch, &mut Replay) else {
            panic!("the game goes on");
        };
        asked = next;
    }
    let slots: Vec<u16> = (picks.iter())
        .map(|pick| match pick {
            Opt::Mana { slot, .. } => slot.num(),
            other => panic!("a pick that is no source: {other:?}"),
        })
        .collect();
    assert_eq!(slots, [plains[0]]);
    // The same picks made by hand end in the same game.
    let (_, _, mut asked) = by_hand();
    for pick in picks {
        let choice = option(&asked, |o| o == pick);
        apply(&mut direct, &table, &mut scratch, &mut Replay, choice);
        let Step::Decide(next) = next(&mut direct, &table, &mut scratch, &mut Replay) else {
            panic!("the game goes on");
        };
        asked = next;
    }
    assert_eq!(state, direct);
}
