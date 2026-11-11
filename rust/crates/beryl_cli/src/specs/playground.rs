//! Playable card examples, separate from the scenario viewer's scripted checkpoints.
//! Every board is an independent session. Setup is scripted; after it, only the unseen
//! player is automated, so a prompt for the viewer is never answered by a replay script.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use beryl_cards::CARDS;
use beryl_engine::Seat;
use beryl_engine::card::{AbilityKind, CardDef, TargetKind, Types};
use beryl_engine::filter::Evaluate;
use beryl_scenario::{Action, Permanent, Scenario};
use beryl_search::Agent;
use beryl_session::{GameUpdate, Prepared, Sessions, StopsSetting};
use serde_json::{Value, json};

use super::{Moment, Phase, View, auto, slug};
use crate::boards::{self, Scripted, Shared};

#[cfg(test)]
mod history_tests;
mod opponent;

pub struct Variant {
    pub moment: Moment,
    pub controller: View,
    pub label: String,
    quality: usize,
    contexts: Vec<Option<opponent::Context>>,
}

pub struct Example {
    pub title: String,
    pub variants: Vec<Variant>,
}

pub struct Playground {
    pub examples: Vec<Example>,
    pub missing: Vec<String>,
}

fn leak<T>(items: Vec<T>) -> &'static [T] {
    Box::leak(items.into_boxed_slice())
}

fn actor(action: &Action) -> Option<Seat> {
    match *action {
        Action::Cast { player, .. }
        | Action::PlayLand { player, .. }
        | Action::Activate { player, .. }
        | Action::TurnFaceUp { player, .. }
        | Action::Answer { player, .. } => Some(player),
        _ => None,
    }
}

/// Existing scenario setups are reusable data, not card-name branches in the viewer.
/// Each distinct variant has its own pair of full-width boards on the scrolling page.
pub fn generate(card: &'static str) -> Playground {
    let def = CARDS
        .cards()
        .iter()
        .copied()
        .find(|d| d.name == card)
        .expect("card definition");
    let scenarios = beryl_cards::SCENARIOS
        .iter()
        .find(|(name, _)| *name == card)
        .map_or(&[][..], |(_, cases)| *cases);
    let mut groups: BTreeMap<String, Vec<Variant>> = BTreeMap::new();
    add_scenarios(def, scenarios, &mut groups);
    // Definitions supply ability setups even when the scenario suite has no activation.
    for moment in auto::playground_probes(def) {
        let title = moment.title.to_owned();
        groups.entry(title.clone()).or_insert_with(|| {
            vec![Variant {
                moment,
                controller: View::Yours,
                label: title,
                quality: 0,
                contexts: Vec::new(),
            }]
        });
    }
    for variants in groups.values_mut() {
        variants.sort_by_key(|v| std::cmp::Reverse(v.quality));
        let mut seen = BTreeSet::new();
        variants.retain(|variant| {
            interaction_signature(variant).is_none_or(|signature| seen.insert(signature))
        });
    }
    let mut examples = Vec::new();
    if let Some(variants) = groups.remove("Play from hand") {
        examples.push(Example {
            title: "Play from hand".to_owned(),
            variants,
        });
    }
    let mut missing = Vec::new();
    if examples.is_empty() {
        // Start from a permanent setup when no test has cast the subject. Move just that
        // permanent to hand and provide its printed mana cost; retain its supporting cards.
        if let Some(scenario) = scenarios.iter().find(|s| {
            s.setup.players[0]
                .battlefield
                .iter()
                .any(|p| p.card == card)
        }) {
            let mut board = auto::with_libraries(scenario).actions(&[]);
            let permanents = board.setup.players[0]
                .battlefield
                .iter()
                .copied()
                .filter(|p| p.card != card)
                .collect::<Vec<_>>();
            board = board.battlefield(Seat::P0, leak(permanents)).hand(
                Seat::P0,
                leak([board.setup.players[0].hand, &[card]].concat()),
            );
            board = auto::fund_cast(def, board);
            let action = if def.front().types.intersects(Types::LAND) {
                Action::PlayLand {
                    player: Seat::P0,
                    card,
                }
            } else {
                Action::cast(Seat::P0, card)
            };
            examples.push(Example {
                title: "Play from hand".to_owned(),
                variants: vec![Variant {
                    moment: Moment::new("playground-hand", "Play from hand", board)
                        .play(leak(vec![action])),
                    controller: View::Yours,
                    label: "Default".to_owned(),
                    quality: 0,
                    contexts: Vec::new(),
                }],
            });
        } else {
            missing.push("Play-from-hand setup unavailable".to_owned());
        }
    }
    if let Some(first) = examples.first_mut() {
        fold_loyalty_abilities(def, first, &mut groups);
    }
    examples.extend(
        groups
            .into_iter()
            .map(|(title, variants)| Example { title, variants }),
    );
    Playground { examples, missing }
}

/// Ordinary loyalty activations can be reviewed on the cast board itself. Only a setup
/// requiring extra loyalty, or exposing a different branch, needs another pair of boards.
fn fold_loyalty_abilities(
    def: &CardDef,
    first: &mut Example,
    groups: &mut BTreeMap<String, Vec<Variant>>,
) {
    groups.retain(|title, variants| {
        let [variant] = variants.as_slice() else {
            return true;
        };
        let Some(
            &action @ Action::Activate {
                ability: beryl_engine::card::AbilityIx::Own(index),
                ..
            },
        ) = variant.moment.play.first()
        else {
            return true;
        };
        let Some(ability) = def.front().abilities.get(usize::from(index)) else {
            return true;
        };
        let Some(loyalty) = ability.cost.loyalty else {
            return true;
        };
        if loyalty < 0 && loyalty.unsigned_abs() > def.front().loyalty.unwrap_or(0) {
            return true;
        }
        let base = &first.variants[0];
        let label = format!("Cast, then {title}");
        let name = Box::leak(format!("playground-hand-ability-{index}").into_boxed_str());
        let candidate = Variant {
            moment: Moment::new(
                name,
                Box::leak(label.clone().into_boxed_str()),
                base.moment.board,
            )
            .play(leak(
                base.moment
                    .play
                    .iter()
                    .copied()
                    .chain(std::iter::once(action))
                    .collect(),
            )),
            controller: base.controller,
            label,
            quality: 0,
            contexts: Vec::new(),
        };
        if interaction_signature(&candidate).is_none() {
            return true;
        }
        first.variants.push(candidate);
        false
    });
}

/// Collapse examples only when their observed UI events and choices agree. In particular,
/// identical cast actions are not enough: conditional triggers can produce different UI.
fn interaction_signature(variant: &Variant) -> Option<String> {
    let (tx, rx) = std::sync::mpsc::channel();
    let session = super::start_review(&variant.moment, move |updates, live| {
        let _ = tx.send((updates, live));
    })
    .ok()?;
    let mut seen = BTreeSet::new();
    seen.insert(format!("action:{}", json!(variant.moment.play.first())));
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(500);
    loop {
        let (updates, live) = rx
            .recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()))
            .ok()?;
        for (seat, update) in updates.iter().enumerate() {
            for object in update
                .view
                .objects
                .iter()
                .filter(|o| o.zone == beryl_session::protocol::ZoneName::Battlefield)
            {
                seen.insert(format!(
                    "{seat}:permanent:{}",
                    json!([
                        object.name,
                        object.controller,
                        object.pt,
                        object.types,
                        object.keywords,
                        object.counters,
                        object.attached_to.is_some(),
                        object.facing,
                        object.token
                    ])
                ));
            }
            for event in &update.view.events {
                let mut value = json!(event);
                strip_slots(&mut value);
                seen.insert(format!("{seat}:event:{value}"));
            }
            if let beryl_session::protocol::Status::Deciding { decision } = &update.view.status {
                if decision.kind == beryl_session::protocol::Kind::Priority {
                    for candidate in &decision.candidates {
                        seen.insert(format!(
                            "{seat}:offered:{}:{}",
                            candidate.label,
                            candidate.targets.len()
                        ));
                    }
                } else {
                    let mut ask = json!(decision.ask);
                    strip_slots(&mut ask);
                    let options: Vec<_> = decision
                        .candidates
                        .iter()
                        .map(|c| json!(c.option)["kind"].clone())
                        .collect();
                    seen.insert(format!("{seat}:ask:{ask}:{options:?}"));
                }
            }
        }
        if live {
            let decision = updates.iter().find_map(|u| match &u.view.status {
                beryl_session::protocol::Status::Deciding { decision } => Some(decision),
                _ => None,
            });
            if let Some(decision) = decision {
                if updates[0].view.stack.is_empty()
                    && decision.kind == beryl_session::protocol::Kind::Priority
                    && decision.resolving.is_none()
                {
                    return Some(format!("{seen:?}"));
                }
                let index = decision
                    .candidates
                    .iter()
                    .position(|c| matches!(c.option, beryl_session::protocol::Candidate::Pass))
                    .unwrap_or(0);
                let _ = session.choose(
                    decision.id,
                    beryl_session::protocol::CandidateIndex(u16::try_from(index).ok()?),
                );
            }
        }
        if updates.iter().any(|u| {
            matches!(
                u.view.status,
                beryl_session::protocol::Status::Over { .. }
                    | beryl_session::protocol::Status::Stopped
            )
        }) {
            return None;
        }
    }
}

fn strip_slots(value: &mut Value) {
    match value {
        Value::Object(fields) => {
            for key in ["slot", "source", "eligible", "id"] {
                fields.remove(key);
            }
            for child in fields.values_mut() {
                strip_slots(child);
            }
        }
        Value::Array(items) => {
            for child in items {
                strip_slots(child);
            }
        }
        _ => {}
    }
}

fn add_scenarios(
    def: &'static CardDef,
    scenarios: &[Scenario],
    groups: &mut BTreeMap<String, Vec<Variant>>,
) {
    let card = def.name;
    for scenario in scenarios {
        let (actions, contexts, ordinals) = opponent::plan(scenario);
        let mut points: Vec<_> = actions
            .iter()
            .enumerate()
            .filter_map(|(i, action)| auto::label_of(def, action).map(|label| (i, label)))
            .collect();
        // A static or triggered ability may be demonstrated by another card's action.
        if points.is_empty()
            && scenario
                .setup
                .players
                .iter()
                .any(|p| p.battlefield.iter().any(|p| p.card == card))
            && !actions.is_empty()
        {
            points.push((0, "Triggers & effects".to_owned()));
        }
        for (i, label) in points {
            let controller = if label == "Triggers & effects" {
                scenario
                    .setup
                    .players
                    .iter()
                    .position(|p| p.battlefield.iter().any(|p| p.card == card))
                    .unwrap_or(0)
            } else {
                actor(&actions[i]).unwrap_or(Seat::P0).index()
            };
            let from_hand = matches!(actions[i], Action::Cast { .. } | Action::PlayLand { .. })
                && scenario.setup.players[controller].hand.contains(&card);
            let title = if from_hand {
                "Play from hand".to_owned()
            } else {
                label
            };
            let variants = groups.entry(title).or_default();
            let name = Box::leak(
                format!("playground-{}-{}", slug(scenario.name), ordinals[i]).into_boxed_str(),
            );
            let mut moment = Moment::new(
                name,
                auto::heading(def, scenario),
                choice_rich(def, auto::with_libraries(scenario), View::ALL[controller])
                    .actions(leak(actions[..i].to_vec())),
            )
            .play(leak(actions[i..].to_vec()));
            moment.group = auto::heading(def, scenario);
            variants.push(Variant {
                moment,
                controller: View::ALL[controller],
                label: auto::heading(def, scenario).to_owned(),
                contexts: contexts[i..].to_vec(),
                quality: scenario
                    .setup
                    .players
                    .iter()
                    .map(|p| p.library.len().min(3))
                    .sum(),
            });
        }
    }
}

fn target_position(
    board: &Scenario,
) -> Result<(beryl_engine::GameTable, beryl_engine::State), String> {
    let (table, mut state) = beryl_scenario::position(&CARDS, board).map_err(|e| e.to_string())?;
    state.set_profile(beryl_engine::Profile {
        stops: beryl_engine::Stops::Forced,
        ..state.profile()
    });
    let _ = beryl_engine::next(
        &mut state,
        &table,
        &mut beryl_engine::Scratch::new(),
        &mut beryl_engine::Replay,
    );
    Ok((table, state))
}

fn support_cards() -> &'static [&'static CardDef] {
    static SUPPORTS: std::sync::OnceLock<Vec<&'static CardDef>> = std::sync::OnceLock::new();
    SUPPORTS.get_or_init(|| {
        let mut supports: Vec<_> = CARDS
            .cards()
            .iter()
            .copied()
            .filter(|d| {
                d.front().types.intersects(
                    Types::CREATURE
                        .union(Types::ARTIFACT)
                        .union(Types::ENCHANTMENT)
                        .union(Types::PLANESWALKER),
                ) && !d
                    .front()
                    .abilities
                    .iter()
                    .any(|a| matches!(a.kind, AbilityKind::Static | AbilityKind::Replacement))
            })
            .collect();
        supports.sort_by_key(|d| (d.front().abilities.len(), d.front().cost.generic, d.name));
        supports
    })
}

/// Populate actual target filters, rather than a card-name list of special cases. Extra
/// targets belong only to Playground; the exact scenario viewer remains unchanged.
fn choice_rich(def: &CardDef, mut board: Scenario, controller: View) -> Scenario {
    let supports = support_cards();
    for target in def
        .faces
        .iter()
        .flat_map(|f| f.abilities)
        .flat_map(|a| a.targets)
        .filter(|t| matches!(t.kind, TargetKind::Permanent | TargetKind::SpellOrPermanent))
    {
        let Ok((table, state)) = target_position(&board) else {
            continue;
        };
        let eligible: Vec<_> = state
            .battlefield()
            .iter()
            .copied()
            .filter(|&slot| {
                state.name(&table, slot) != def.name
                    && target
                        .filter
                        .matches_x(&state, &table, slot, controller.seat(), 3)
            })
            .collect();
        if eligible.len() > usize::from(target.count) {
            continue;
        }
        let owners = eligible
            .first()
            .map_or(vec![controller.other(), controller.seat()], |&slot| {
                vec![state.controller(slot)]
            });
        'support: for owner in owners {
            for support in supports {
                if support.name == def.name {
                    continue;
                }
                if board.setup.players[owner.index()]
                    .battlefield
                    .iter()
                    .any(|p| p.card == support.name)
                {
                    continue;
                }
                let mut permanents = board.setup.players[owner.index()].battlefield.to_vec();
                permanents.push(Permanent::new(support.name));
                let candidate = board.battlefield(owner, leak(permanents));
                let Ok((table, state)) = target_position(&candidate) else {
                    continue;
                };
                if state.battlefield().iter().any(|&slot| {
                    state.name(&table, slot) == support.name
                        && target
                            .filter
                            .matches_x(&state, &table, slot, controller.seat(), 3)
                }) {
                    board = candidate;
                    break 'support;
                }
            }
        }
    }
    board
}

pub fn automated(variant: &Variant, view: View) -> bool {
    variant
        .moment
        .play
        .iter()
        .find_map(actor)
        .is_some_and(|seat| seat != view.seat())
}

pub fn listing(playground: &Playground) -> Value {
    json!({ "examples": playground.examples.iter().map(|e| json!({
        "title": e.title,
        "variants": e.variants.iter().map(|v| json!({
            "name": v.moment.name, "title": v.moment.title, "group": v.moment.group,
            "plays": true, "first": v.controller.name(), "label": v.label,
        })).collect::<Vec<_>>()
    })).collect::<Vec<_>>(), "missing": playground.missing })
}

pub fn find(card: &str, name: &str) -> Option<&'static Variant> {
    if !name.starts_with("playground-") {
        return None;
    }
    super::specs()
        .iter()
        .find(|s| slug(s.card) == card)?
        .playground()
        .examples
        .iter()
        .flat_map(|e| &e.variants)
        .find(|v| v.moment.name == name)
}

/// Finish setup before creating either visible game. A cast in the setup is not finished
/// merely because its first action was taken: its targets, payment and resolution may still
/// be pending. Stop only when the intended interaction is actually offered.
fn prepare(variant: &Variant) -> Result<(beryl_engine::GameTable, beryl_engine::State), String> {
    let moment = &variant.moment;
    let progress = Shared::default();
    if let Some(Action::AdvanceTo { turn, .. }) = moment.board.actions.last() {
        boards::lock(&progress).until = Some(*turn);
    }
    let (table, mut state) =
        beryl_scenario::position(&CARDS, &moment.board).map_err(|e| e.to_string())?;
    state.set_profile(beryl_engine::Profile {
        stops: beryl_engine::Stops::Forced,
        ..state.profile()
    });
    let mut agents = [
        Scripted::new(&moment.board, Seat::P0, &table, Arc::clone(&progress))?,
        Scripted::new(&moment.board, Seat::P1, &table, Arc::clone(&progress))?,
    ];
    let mut scratch = beryl_engine::Scratch::new();
    for _ in 0..512 {
        let step = beryl_engine::next(&mut state, &table, &mut scratch, &mut beryl_engine::Replay);
        let beryl_engine::Step::Decide(decision) = step else {
            return Err("Game ended during playground setup".to_owned());
        };
        boards::lock(&progress).turn = state.turn().number;
        let ready = boards::lock(&progress).done()
            && moment.play.first().is_none_or(|a| {
                actor(a).is_none_or(|seat| seat == decision.player)
                    && match a {
                        Action::AdvanceTo { .. } => true,
                        Action::Answer { kind, .. } => {
                            *kind == decision.kind
                                && beryl_scenario::answer(&table, &state, &decision, a).is_some()
                        }
                        _ => {
                            decision.kind == beryl_engine::DecisionKind::Priority
                                && beryl_scenario::answer(&table, &state, &decision, a).is_some()
                        }
                    }
            });
        if ready {
            state.set_profile(beryl_engine::Profile::EXACT);
            return Ok((table, state));
        }
        let choice = agents[decision.player.index()].choose(
            beryl_engine::observe(&state, &table, &mut scratch, decision.player),
            &decision,
        );
        beryl_engine::apply(
            &mut state,
            &table,
            &mut scratch,
            &mut beryl_engine::Replay,
            choice,
        );
    }
    Err(format!("Could not reach {}", moment.title))
}

pub fn start(
    variant: &'static Variant,
    view: View,
    emit: impl Fn(GameUpdate, Phase) + Send + Sync + 'static,
) -> Result<Sessions, String> {
    let (table, state) = prepare(variant)?;
    let opponent = opponent::Opponent::new(variant, view.other(), &table)?;
    let stand_in = Scripted::with_actions(&[], view.seat(), &table, Shared::default())?;
    let sessions = Sessions::new(None, move |update| emit(update, Phase::Live));
    sessions
        .start_prepared(Prepared {
            opponent: Box::new(opponent),
            stand_in: Box::new(stand_in),
            scripted: true,
            hand_over: Some(Box::new(|| true)),
            table,
            state,
            human: view.seat(),
            stops: StopsSetting::SMART,
        })
        .map_err(|e| e.to_string())?;
    Ok(sessions)
}

#[cfg(test)]
mod tests {
    use super::*;
    use beryl_session::protocol::{Candidate, Side, Status, ZoneName};
    use std::sync::mpsc::channel;
    use std::time::Duration;

    fn first_decision(variant: &'static Variant, view: View) -> beryl_session::PlayerView {
        let (tx, rx) = channel();
        let _session = start(variant, view, move |u, phase| {
            let _ = tx.send((u, phase));
        })
        .unwrap();
        loop {
            let (update, phase) = rx
                .recv_timeout(Duration::from_secs(3))
                .expect("playground stalled");
            if phase == Phase::Live
                && matches!(
                    update.view.status,
                    Status::Deciding { .. } | Status::Over { .. }
                )
            {
                return update.view;
            }
        }
    }

    #[test]
    fn empty_library_manifest_example_has_no_filler() {
        let variant = find(
            "abhorrent-oculus",
            "playground-manifest-dread-with-an-empty-library-does-nothing-0",
        )
        .expect("the empty-library variant");
        let (_, state) = prepare(variant).unwrap();
        assert!(state.library(variant.controller.seat()).is_empty());
    }

    #[test]
    fn one_card_manifest_example_has_no_filler_to_choose_or_mill() {
        let generated = Box::leak(Box::new(generate("Abhorrent Oculus")));
        let variant = generated
            .examples
            .iter()
            .flat_map(|e| &e.variants)
            .find(|v| v.label == "Manifest dread with one card in the library manifests it")
            .expect("the one-card variant");
        let (table, state) = prepare(variant).unwrap();
        let library = state.library(variant.controller.seat());
        assert_eq!(library.len(), 1);
        assert_eq!(state.name(&table, library[0]), "Island");
    }

    #[test]
    fn first_example_offers_the_card_in_hand() {
        for spec in super::super::specs() {
            let card = spec.card;
            let generated = Box::leak(Box::new(generate(card)));
            assert!(
                generated.missing.is_empty(),
                "{card}: {:?}",
                generated.missing
            );
            let first = &generated.examples[0];
            assert_eq!(first.title, "Play from hand", "{card}");
            let variant = &first.variants[0];
            let view = first_decision(variant, variant.controller);
            let card = view
                .objects
                .iter()
                .find(|o| {
                    o.name.as_deref() == Some(card)
                        && o.zone == ZoneName::Hand
                        && o.controller == Side::You
                })
                .expect(card);
            let Status::Deciding { decision } = &view.status else {
                panic!("game ended")
            };
            assert!(decision.candidates.iter().any(|c| matches!(c.option, Candidate::Cast { slot, .. } | Candidate::PlayLand { slot } if slot == card.slot)), "card is not playable: {:?}", decision.candidates);
        }
    }

    #[test]
    fn every_prepared_example_reaches_its_interaction() {
        let mut failures = Vec::new();
        for spec in super::super::specs() {
            for example in &spec.playground().examples {
                for variant in &example.variants {
                    match prepare(variant) {
                        Err(error) => {
                            failures.push(format!("{} / {}: {error}", spec.card, variant.label));
                        }
                        Ok((table, state)) => {
                            // A generic nonland target filler once put Counterspell on the battlefield.
                            for &slot in state.battlefield() {
                                assert!(
                                    !state
                                        .derived(slot)
                                        .types
                                        .intersects(Types::INSTANT.union(Types::SORCERY)),
                                    "{} / {}: {} is not a permanent",
                                    spec.card,
                                    variant.label,
                                    state.name(&table, slot)
                                );
                            }
                        }
                    }
                }
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    #[test]
    fn opponent_play_leaves_the_viewers_search_interactive() {
        let generated = Box::leak(Box::new(generate("Winds of Abandon")));
        let variant = &generated.examples[0].variants[0];
        let view = View::ALL[1 - variant.controller.seat().index()];
        let (tx, rx) = channel();
        let session = start(variant, view, move |u, _| {
            let _ = tx.send(u);
        })
        .unwrap();
        for _ in 0..50 {
            let update = rx
                .recv_timeout(Duration::from_secs(3))
                .expect("search never reached");
            let Status::Deciding { decision } = update.view.status else {
                continue;
            };
            if matches!(
                decision.ask,
                beryl_session::protocol::AskView::Search { .. }
            ) {
                assert!(decision.candidates.len() > 1, "search needs a real choice");
                return;
            }
            let pass = decision
                .candidates
                .iter()
                .position(|c| matches!(c.option, Candidate::Pass))
                .expect("unexpected prompt before search");
            session
                .choose(
                    decision.id,
                    beryl_session::protocol::CandidateIndex(u16::try_from(pass).unwrap()),
                )
                .unwrap();
        }
        panic!("search never offered");
    }
}
