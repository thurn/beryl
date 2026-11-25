//! What a face-down permanent shows each player (architecture §13): to both a 2/2 creature with
//! no name (CR 708.2a), and to its controller alone the card it is
//! ([CR 708.5](https://yawgatog.com/resources/magic-rules/#R7085)): "At any time, you may look
//! at a face-down spell you control on the stack or a face-down permanent you control ... You
//! can't look at face-down cards in any other zone or face-down spells or permanents controlled
//! by another player."

use crate::common::fixtures::{COPY_FACE_DOWN_PERMANENT, OCULUS_MANIFESTS_DREAD};
use beryl_cards::{CARDS, props};
use beryl_engine::{Observation, Seat, SlotId, Zone};
use beryl_session::StopsSetting;
use beryl_session::protocol::{
    CardType, Facing, Moment, ObjectId, ObjectView, PowerToughness, Side, player_view,
};

/// The face-down permanent each player sees once Abhorrent Oculus has manifested
/// `props::VANILLA`, as player 0 (its controller) and player 1.
fn seen() -> [ObjectView; 2] {
    let scenario = OCULUS_MANIFESTS_DREAD;
    let mut seen = None;
    beryl_scenario::visit(&CARDS, scenario, |t, s, _| {
        let view = |viewer| {
            let v = player_view(
                t,
                Observation::new(s, viewer),
                Moment::Thinking {
                    resolving: None,
                    decision: None,
                },
                StopsSetting::SMART,
                &[],
            );
            v.objects.into_iter().find(|o| o.facing == Facing::Down)
        };
        if let (Some(mine), Some(theirs)) = (view(Seat::P0), view(Seat::P1)) {
            seen = Some([mine, theirs]);
        }
    })
    .unwrap_or_else(|e| panic!("{e}"));
    seen.expect("a face-down permanent")
}

#[test]
fn a_face_down_permanent_is_a_nameless_two_two_to_its_opponent() {
    let [mine, theirs] = seen();
    let two_two = Some(PowerToughness {
        power: 2,
        toughness: 2,
    });
    assert_eq!(mine.slot, theirs.slot);
    assert_eq!(mine.name.as_deref(), Some(props::VANILLA.card));
    assert_eq!(theirs.name, None);
    assert_eq!(theirs.controller, Side::Opponent);
    for o in [&mine, &theirs] {
        assert_eq!(o.pt, two_two);
        assert_eq!(o.types, [CardType::Creature]);
    }
}

/// Fractured Identity's copy of the face-down creature is a face-up token, since being
/// face down is a status a copy doesn't copy
/// ([CR 707.2](https://yawgatog.com/resources/magic-rules/#R7072)), of the face-down values
/// ([CR 708.2](https://yawgatog.com/resources/magic-rules/#R7082)): "Any listed characteristics
/// are the copiable values of that object's characteristics." Both players see the same
/// nameless 2/2, and neither the state nor either view of it holds that creature's name.
#[test]
fn a_copy_of_a_face_down_permanent_is_a_face_up_nameless_two_two() {
    let scenario = COPY_FACE_DOWN_PERMANENT;
    let mut last = None;
    beryl_scenario::visit(&CARDS, scenario, |t, s, _| last = Some((t.clone(), *s)))
        .unwrap_or_else(|e| panic!("{e}"));
    let (t, s) = last.expect("a decision");
    let copy = s
        .battlefield()
        .iter()
        .copied()
        .find(|&slot| s.object(slot).is_token())
        .expect("the copy");
    let snapcaster = t.index_of(CARDS.get(props::VANILLA.card).unwrap()).unwrap();
    assert_eq!(
        s.copiable(copy).record(&t),
        (
            t.face_down_copy::<beryl_engine::Small>(),
            beryl_engine::card::FaceIx::FRONT
        )
    );
    assert_ne!(s.copiable(copy).record(&t).0, snapcaster);
    assert_eq!(s.card_behind(copy), None);
    for viewer in Seat::BOTH {
        let obs = Observation::new(&s, viewer);
        assert_eq!(
            obs.card(copy),
            Some(t.face_down_copy::<beryl_engine::Small>()),
            "{viewer:?}"
        );
        let view = player_view(
            &t,
            obs,
            Moment::Thinking {
                resolving: None,
                decision: None,
            },
            StopsSetting::SMART,
            &[],
        );
        let o = view
            .objects
            .iter()
            .find(|o| o.slot == ObjectId(copy.num()))
            .expect("the copy's view");
        assert_eq!(o.zone, Zone::Battlefield.into());
        assert_eq!(o.facing, Facing::Up, "{viewer:?}");
        assert!(o.token);
        assert_eq!(o.name.as_deref(), Some(""), "{viewer:?}");
        assert_eq!(
            o.pt,
            Some(PowerToughness {
                power: 2,
                toughness: 2
            })
        );
        assert_eq!(o.types, [CardType::Creature]);
    }
}

#[test]
fn morphs_hide_the_card_on_the_stack_and_in_saved_targets() {
    use beryl_cards::names::{EXALTED_ANGEL, GATHAN_RAIDERS, LIGHTNING_BOLT, MOUNTAIN, PLAINS};
    use beryl_engine::card::{CastingOption, FaceIx};
    use beryl_engine::rng::SplitMix64;
    use beryl_engine::{Choice, DecisionKind, Opt, Replay, Scratch, Step, next};
    use beryl_scenario::prelude::{ME, Resolve, THEM};
    use beryl_scenario::{P0, P1, Scenario};
    use beryl_session::save::Chosen;

    const SCENARIO: Scenario = Scenario::new("The opponent targets the second face-down creature")
        .hand(P0, &[EXALTED_ANGEL, GATHAN_RAIDERS])
        .library(P0, &[GATHAN_RAIDERS, EXALTED_ANGEL, MOUNTAIN])
        .battlefield(P0, &[PLAINS, PLAINS, PLAINS, PLAINS, PLAINS, PLAINS])
        .hand(P1, &[LIGHTNING_BOLT])
        .battlefield(P1, &[MOUNTAIN])
        .actions(&[
            ME.casts_face(EXALTED_ANGEL, FaceIx::DOWN),
            Resolve,
            ME.casts_face(GATHAN_RAIDERS, FaceIx::DOWN),
            Resolve,
            ME.passes(),
            THEM.casts(LIGHTNING_BOLT),
            THEM.targets(GATHAN_RAIDERS),
            ME.has(Zone::Graveyard, GATHAN_RAIDERS, 1),
        ]);
    let mut rng = SplitMix64::new(81);
    let mut stack_checks = 0;
    let mut recorded = None;
    beryl_scenario::visit(&CARDS, &SCENARIO, |t, s, d| {
        let obs = Observation::new(s, P1);
        for entry in obs.stack().iter().filter(|e| e.face() == FaceIx::DOWN) {
            let slot = entry.spell().unwrap().slot;
            assert_eq!(obs.card(slot), None);
            assert_eq!(entry.card(), t.face_down_copy::<beryl_engine::Small>());
            assert_eq!(s.mana_value(t, slot), 0);
            assert_eq!(Observation::new(s, P0).card(slot), s.card_behind(slot));
            let det = obs.determinize(t, &mut rng);
            assert!(
                det.card_def(t, slot)
                    .unwrap()
                    .front()
                    .casting
                    .iter()
                    .any(|o| matches!(o, CastingOption::Morph(_)))
            );
            assert_eq!(Observation::new(&det, P1).stack(), obs.stack());
            stack_checks += 1;
        }
        if d.kind != DecisionKind::Target || d.player != P1 {
            return;
        }
        let down: Vec<_> = s
            .battlefield()
            .iter()
            .copied()
            .filter(|&slot| s.object(slot).face() == FaceIx::DOWN)
            .collect();
        assert_eq!(down.len(), 2);
        assert_hidden_face_down_permanents(t, obs);
        let targets: Vec<_> = (0..u16::try_from(d.len()).unwrap())
            .map(Choice)
            .filter_map(|choice| match d.candidates.option(choice) {
                Some(Opt::Object(slot)) if down.contains(&slot) => Some((choice, slot)),
                _ => None,
            })
            .collect();
        assert_eq!([targets[0].1, targets[1].1], down.as_slice());
        let chosen = targets[1].0;
        let saved = Chosen::new(t, s, d, chosen);
        let json = serde_json::to_string(&saved).unwrap();
        assert!(
            !json.contains(EXALTED_ANGEL) && !json.contains(GATHAN_RAIDERS),
            "{json}"
        );
        let mut det = obs.determinize(t, &mut rng);
        let Step::Decide(again) = next(&mut det, t, &mut Scratch::new(), &mut Replay) else {
            panic!("redealing must leave the target decision open");
        };
        assert_eq!(again, *d);
        recorded = Some((t.clone(), saved));
    })
    .unwrap_or_else(|e| panic!("{e}"));
    assert!(stack_checks >= 2);
    let (table, saved) = recorded.expect("the opponent targeted a face-down creature");
    assert_saved_face_down_target(&SCENARIO, &table, &saved);
}

/// A card cast face down through Illusionary Mask must be a creature card, which everyone
/// knows, so the search's determinization of the opponent's view deals only creature cards
/// into its slot, on the stack and on the battlefield, though most of the rest of its owner's
/// deck is not creatures.
#[test]
fn a_card_cast_face_down_through_a_mask_is_dealt_as_a_creature() {
    use beryl_cards::names::{ILLUSIONARY_MASK, LIGHTNING_BOLT, MOUNTAIN, PLAINS};
    use beryl_engine::DecisionKind;
    use beryl_engine::card::{FaceIx, Types};
    use beryl_engine::rng::SplitMix64;
    use beryl_scenario::prelude::{ME, Resolve};
    use beryl_scenario::{P0, P1, Scenario};

    /// A 2/10 creature card for {2}{W}{W}.
    const INDOMITABLE_ANCIENTS: &str = "Indomitable Ancients";
    const SCENARIO: Scenario = Scenario::new("Illusionary Mask casts a creature face down")
        .battlefield(P0, &[ILLUSIONARY_MASK, PLAINS, PLAINS, PLAINS, PLAINS])
        .hand(P0, &[INDOMITABLE_ANCIENTS])
        .library(
            P0,
            &[LIGHTNING_BOLT, MOUNTAIN, LIGHTNING_BOLT, MOUNTAIN, PLAINS],
        )
        .battlefield(P1, &[MOUNTAIN])
        .actions(&[
            ME.activates(ILLUSIONARY_MASK, 0),
            ME.x(4),
            Resolve,
            ME.chooses(DecisionKind::CastOffer, INDOMITABLE_ANCIENTS),
            Resolve,
            ME.face_down(INDOMITABLE_ANCIENTS, 1),
            ME.passes(),
        ]);
    /// Deals the opponent's view of each face-down card at each decision.
    struct Deals {
        rng: SplitMix64,
        checked: [u32; 2],
    }
    impl beryl_scenario::DecisionVisitor for Deals {
        fn decision<L: beryl_engine::Layout>(
            &mut self,
            t: &beryl_engine::GameTable,
            s: &beryl_engine::State<L>,
            _: &beryl_engine::Decision<L>,
        ) {
            let obs = Observation::new(s, P1);
            let on_stack = s
                .stack()
                .iter()
                .filter(|e| e.face() == FaceIx::DOWN)
                .filter_map(beryl_engine::state::StackEntry::spell)
                .map(|spell| (0, spell.slot));
            let on_battlefield = s
                .battlefield()
                .iter()
                .filter(|&&slot| s.object(slot).face() == FaceIx::DOWN)
                .map(|&slot| (1, slot));
            for (zone, slot) in on_stack.chain(on_battlefield).collect::<Vec<_>>() {
                for _ in 0..16 {
                    let det = obs.determinize(t, &mut self.rng);
                    let card = det.card_def(t, slot).unwrap();
                    assert!(
                        card.front().types.intersects(Types::CREATURE),
                        "dealt {} face down",
                        card.name
                    );
                }
                self.checked[zone] += 1;
            }
        }
    }
    let mut deals = Deals {
        rng: SplitMix64::new(5),
        checked: [0; 2],
    };
    beryl_scenario::visit_with_layout(&CARDS, &SCENARIO, &mut deals)
        .unwrap_or_else(|e| panic!("{e}"));
    let checked = deals.checked;
    assert!(checked.iter().all(|&n| n > 0), "{checked:?}");
}

fn assert_saved_face_down_target(
    scenario: &beryl_scenario::Scenario,
    table: &beryl_engine::GameTable,
    saved: &beryl_session::save::Chosen,
) {
    use beryl_engine::card::FaceIx;
    use beryl_engine::state::{CastStep, Pending};
    use beryl_engine::{Opt, Replay, Scratch, Step, next};
    use beryl_scenario::Slots;
    use beryl_session::perturb;

    for slots in Slots::ALL {
        let mut tested = false;
        beryl_scenario::watch_with(&CARDS, scenario, slots, |s, _| {
            if !matches!(s.pending(), Pending::Casting(c) if matches!(c.step, CastStep::Target { .. })) {
                return;
            }
            let mut state = *s;
            let Step::Decide(d) = next(&mut state, table, &mut Scratch::new(), &mut Replay) else {
                panic!("the target choice must still be open");
            };
            let second = state.battlefield().iter().copied()
                .filter(|&slot| state.object(slot).face() == FaceIx::DOWN).nth(1).unwrap();
            for (_, _, perturbation) in perturb::Perturbation::EACH {
                let choice = perturb::with(perturbation, || saved.find(table, &state, &d))
                    .expect("the saved answer has the same target");
                assert_eq!(d.candidates.option(choice), Some(Opt::Object(second)));
            }
            tested = true;
        }).unwrap_or_else(|e| panic!("{e}"));
        assert!(tested);
    }
}

fn assert_hidden_face_down_permanents(
    t: &beryl_engine::GameTable,
    obs: Observation<'_, beryl_engine::Small>,
) {
    let view = player_view(
        t,
        obs,
        Moment::Thinking {
            resolving: None,
            decision: None,
        },
        StopsSetting::SMART,
        &[],
    );
    let shown: Vec<_> = view
        .objects
        .iter()
        .filter(|o| o.facing == Facing::Down)
        .collect();
    assert_eq!(shown.len(), 2);
    for shown in shown {
        assert_eq!(shown.name, None);
        assert_eq!(shown.types, [CardType::Creature]);
        assert_eq!(
            shown.pt,
            Some(PowerToughness {
                power: 2,
                toughness: 2
            })
        );
    }
}

#[test]
fn a_face_down_cast_hides_which_revealed_hand_card_was_chosen() {
    use beryl_cards::names::{EXALTED_ANGEL, GATHAN_RAIDERS, PLAINS};
    use beryl_engine::card::FaceIx;
    use beryl_engine::{Choice, Opt, Replay, Scratch, Step, ZoneKey, apply, next};
    use beryl_scenario::{P0, P1, Scenario};

    let scenario = Scenario::new("Cast one of two previously seen cards face down")
        .hand(P0, &[EXALTED_ANGEL, GATHAN_RAIDERS])
        .battlefield(P0, &[PLAINS; 3]);
    let (table, mut state) = beryl_scenario::position(&CARDS, &scenario).unwrap();
    let hand = state.zone(ZoneKey::Of(P0, Zone::Hand));
    for name in [EXALTED_ANGEL, GATHAN_RAIDERS] {
        let slot = hand
            .filter(|slot| state.name(&table, slot) == name)
            .sole()
            .unwrap();
        state.board().set_known(P1, slot, true);
    }
    let mut scratch = Scratch::new();
    let Step::Decide(decision) = next(&mut state, &table, &mut scratch, &mut Replay) else {
        panic!("the caster must have priority");
    };
    let choice = (0..u16::try_from(decision.len()).unwrap())
        .map(Choice)
        .find(|&choice| {
            matches!(
                decision.candidates.option(choice),
                Some(Opt::Cast {
                    face: FaceIx::DOWN,
                    ..
                })
            )
        })
        .expect("a face-down cast");
    apply(&mut state, &table, &mut scratch, &mut Replay, choice);
    let spell = state
        .stack()
        .last()
        .and_then(beryl_engine::state::StackEntry::spell)
        .unwrap();
    assert!(Observation::new(&state, P0).knows(spell.slot));
    assert!(hand.all(|slot| !Observation::new(&state, P1).knows(slot)));
}
