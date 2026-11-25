//! Playable card examples, separate from the scenario viewer's scripted checkpoints.
//! Every board is an independent session. Setup is scripted; after it, only the unseen
//! player is automated, so a prompt for the viewer is never answered by a replay script.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use beryl_engine::Seat;
use beryl_engine::card::{AbilityKind, CardDef, FaceIx, TargetKind, Types, Words};
use beryl_engine::filter::Evaluate;
use beryl_scenario::{Action, Permanent, Scenario};
use beryl_search::{Agent, AgentDecision};
use beryl_session::{GameUpdate, Prepared, Sessions, StopsSetting};
use serde_json::{Value, json};

use super::{Moment, Phase, View, auto, slug};
use crate::Catalog;
use crate::boards::{self, Scripted, Shared};

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
        | Action::Companion { player }
        | Action::Answer { player, .. } => Some(player),
        _ => None,
    }
}

/// Existing scenario setups are reusable data, not card-name branches in the viewer.
/// Each distinct variant has its own pair of full-width boards on the scrolling page.
pub fn generate(catalog: &'static Catalog, card: &'static str) -> Playground {
    let def = catalog
        .cards
        .cards()
        .iter()
        .copied()
        .find(|d| d.name == card)
        .expect("card definition");
    let scenarios = super::scenarios(catalog, def);
    let mut groups: BTreeMap<String, Vec<Variant>> = BTreeMap::new();
    add_scenarios(catalog, def, scenarios, &mut groups);
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
            interaction_signature(catalog, variant).is_none_or(|signature| seen.insert(signature))
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
        if let Some(scenario) = scenarios
            .iter()
            .find(|s| s.setup.players[0].all_permanents().any(|p| p.card == card))
        {
            let mut board = auto::with_libraries(scenario).actions(&[]);
            let permanents = board.setup.players[0]
                .all_permanents()
                .filter(|p| p.card != card)
                .collect::<Vec<_>>();
            board = board.replace_permanents(Seat::P0, leak(permanents)).hand(
                Seat::P0,
                leak([board.setup.players[0].hand, &[card]].concat()),
            );
            board = auto::fund_cast(catalog, def, board);
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
        fold_loyalty_abilities(catalog, def, first, &mut groups);
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
    catalog: &'static Catalog,
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
                ability: beryl_engine::card::ObjectAbility::Own(index),
                ..
            },
        ) = variant.moment.play.first()
        else {
            return true;
        };
        let Some(ability) = def.front().abilities.get(index.index()) else {
            return true;
        };
        let Some(loyalty) = ability.cost().loyalty_change() else {
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
        if interaction_signature(catalog, &candidate).is_none() {
            return true;
        }
        first.variants.push(candidate);
        false
    });
}

/// Collapse examples only when their observed UI events and choices agree. In particular,
/// identical cast actions are not enough: conditional triggers can produce different UI.
fn interaction_signature(catalog: &'static Catalog, variant: &Variant) -> Option<String> {
    let (tx, rx) = std::sync::mpsc::channel();
    let session = super::start_review(catalog, &variant.moment, move |updates, live| {
        let _ = tx.send((updates, live));
    })
    .ok()?;
    let mut seen = BTreeSet::new();
    seen.insert(format!(
        "action:{}",
        json!(variant.moment.play.first().map(beryl_scenario::Json))
    ));
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
                    let mut ask = json!(decision.prompt);
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
            for key in ["slot", "source", "about", "id"] {
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
    catalog: &'static Catalog,
    def: &'static CardDef,
    scenarios: &[Scenario],
    groups: &mut BTreeMap<String, Vec<Variant>>,
) {
    let card = def.name;
    for scenario in scenarios {
        let (actions, contexts, ordinals) = opponent::plan(catalog, scenario);
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
                .any(|p| p.all_permanents().any(|p| p.card == card))
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
                    .position(|p| p.all_permanents().any(|p| p.card == card))
                    .unwrap_or(0)
            } else {
                actor(&actions[i]).unwrap_or(Seat::P0).index()
            };
            // A prepared spell is cast from its own copy even if the card starts in hand.
            let can_play_from_hand = match actions[i] {
                Action::Cast { face, .. } => face != FaceIx::BACK || def.shape.can_cast_back(),
                Action::PlayLand { .. } => true,
                _ => false,
            };
            let from_hand =
                can_play_from_hand && scenario.setup.players[controller].hand.contains(&card);
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
                choice_rich(
                    catalog,
                    def,
                    auto::with_libraries(scenario),
                    View::ALL[controller],
                )
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
    catalog: &'static Catalog,
    board: &Scenario,
) -> Result<(beryl_engine::GameTable, beryl_engine::State), String> {
    let (table, mut state) =
        beryl_scenario::position(catalog.cards, board).map_err(|e| e.to_string())?;
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

fn support_cards(catalog: &'static Catalog) -> &'static [&'static CardDef] {
    static SUPPORTS: std::sync::OnceLock<Vec<&'static CardDef>> = std::sync::OnceLock::new();
    SUPPORTS.get_or_init(|| {
        let mut supports: Vec<_> = catalog
            .cards
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
                    .any(|a| matches!(a.kind(), AbilityKind::Static | AbilityKind::Replacement))
            })
            .collect();
        supports.sort_by_key(|d| (d.front().abilities.len(), d.front().cost.generic, d.name));
        supports
    })
}

/// Populate actual target filters, rather than a card-name list of special cases. Extra
/// targets belong only to Playground; the exact scenario viewer remains unchanged.
fn choice_rich(
    catalog: &'static Catalog,
    def: &CardDef,
    mut board: Scenario,
    controller: View,
) -> Scenario {
    let supports = support_cards(catalog);
    for target in def
        .faces
        .iter()
        .flat_map(|f| f.abilities)
        .flat_map(beryl_engine::card::Ability::targets)
        .filter(|t| {
            matches!(
                t.kind,
                TargetKind::Permanent
                    | TargetKind::SpellOrPermanent
                    | TargetKind::PermanentOrPlayer
            )
        })
    {
        let Ok((table, state)) = target_position(catalog, &board) else {
            continue;
        };
        let eligible: Vec<_> = state
            .battlefield()
            .iter()
            .copied()
            .filter(|&slot| {
                state.name(&table, slot) != def.name
                    && target.filter.matches_x(
                        &state,
                        &table,
                        slot,
                        controller.seat(),
                        &Words::NONE,
                        3,
                    )
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
                    .all_permanents()
                    .any(|p| p.card == support.name)
                {
                    continue;
                }
                let mut permanents: Vec<Permanent> = board.setup.players[owner.index()]
                    .all_permanents()
                    .collect();
                permanents.push(Permanent::new(support.name));
                let candidate = board.replace_permanents(owner, leak(permanents));
                let Ok((table, state)) = target_position(catalog, &candidate) else {
                    continue;
                };
                if state.battlefield().iter().any(|&slot| {
                    state.name(&table, slot) == support.name
                        && target.filter.matches_x(
                            &state,
                            &table,
                            slot,
                            controller.seat(),
                            &Words::NONE,
                            3,
                        )
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

pub fn find(catalog: &'static Catalog, card: &str, name: &str) -> Option<&'static Variant> {
    if !name.starts_with("playground-") {
        return None;
    }
    super::spec(catalog, card)?
        .playground()
        .examples
        .iter()
        .flat_map(|e| &e.variants)
        .find(|v| v.moment.name == name)
}

/// Finish setup before creating either visible game. A cast in the setup is not finished
/// merely because its first action was taken: its targets, payment and resolution may still
/// be pending. Stop only when the intended interaction is actually offered.
pub fn prepare(
    catalog: &'static Catalog,
    variant: &Variant,
) -> Result<(beryl_engine::GameTable, beryl_engine::State), String> {
    let moment = &variant.moment;
    let progress = Shared::default();
    if let Some(Action::AdvanceTo { turn, .. }) = moment.board.actions.last() {
        boards::lock(&progress).until = Some(*turn);
    }
    let (table, mut state) =
        beryl_scenario::position(catalog.cards, &moment.board).map_err(|e| e.to_string())?;
    // A mana ability the variant taps at priority is offered only to a player the profile
    // names, as the scenario runner does for that action.
    let taps_mana = (moment.play.first())
        .and_then(|action| beryl_scenario::mana_activation(&table, &state, action));
    state.set_profile(beryl_engine::Profile {
        stops: beryl_engine::Stops::Forced,
        taps_mana,
        ..state.profile()
    });
    let mut agents = [
        Scripted::new(&moment.board, Seat::P0, &table, Arc::clone(&progress))?,
        Scripted::new(&moment.board, Seat::P1, &table, Arc::clone(&progress))?,
    ];
    let mut scratch = beryl_engine::Scratch::new();
    let mut asked = [AgentDecision(0); 2];
    for _ in 0..512 {
        let step = beryl_engine::next(&mut state, &table, &mut scratch, &mut beryl_engine::Replay);
        let beryl_engine::Step::Decide(decision) = step else {
            return Err("Game ended during playground setup".to_owned());
        };
        boards::lock(&progress).turn = state.turn().number();
        let ready = boards::lock(&progress).done()
            && moment.play.first().is_none_or(|a| {
                actor(a).is_none_or(|seat| seat == decision.decider)
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
            state.set_profile(beryl_engine::Profile {
                taps_mana,
                ..beryl_engine::Profile::EXACT
            });
            return Ok((table, state));
        }
        let choice = agents[decision.decider.index()].choose(
            beryl_engine::observe(&state, &table, &mut scratch, decision.decider),
            &decision,
            asked[decision.decider.index()].take(),
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
    catalog: &'static Catalog,
    variant: &'static Variant,
    view: View,
    emit: impl Fn(GameUpdate, Phase) + Send + Sync + 'static,
) -> Result<Sessions, String> {
    let (table, state) = prepare(catalog, variant)?;
    let opponent = opponent::Opponent::new(variant, view.other(), &table)?;
    let stand_in = Scripted::with_actions(&[], view.seat(), &table, Shared::default())?;
    let sessions = Sessions::new(catalog.cards, None, move |update| {
        emit(update, Phase::Live);
    });
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
