//! Saved cast costs retain their choices when mana payment moves before exile selection.
use beryl_cards::{CARDS, names};
use beryl_engine::card::Purpose;
use beryl_engine::{Seat, TurnStep};
use beryl_scenario::{P0, P1, Permanent, Scenario};
use beryl_session::checkpoint::Checkpoint;
use beryl_session::protocol::{Candidate, Kind};
use beryl_session::{
    CandidateIndex, Game, PlayerView, ReplayFile, SessionId, Status, StopsSetting,
};

const SESSION: SessionId = SessionId(1);
const SETUP: Scenario = Scenario::new("saved exile and mana choices")
    .at(3, P0, TurnStep::Upkeep)
    .battlefield(
        P0,
        &[
            Permanent::new(names::ISLAND),
            Permanent::new(names::ISLAND),
            Permanent::new(names::ISLAND),
            Permanent::new(names::PLAINS),
            Permanent::new(names::MANA_CRYPT),
        ],
    )
    .hand(P0, &[names::ABHORRENT_OCULUS])
    .graveyard(
        P0,
        &[
            names::PLAINS,
            names::ISLAND,
            names::OUST,
            names::REPRIEVE,
            names::COUNTERSPELL,
            names::STOCK_UP,
            names::THIRST_FOR_DISCOVERY,
        ],
    )
    .library(P0, &[names::ISLAND; 12])
    .library(P1, &[names::ISLAND; 12]);
fn decision(v: &PlayerView) -> &beryl_session::protocol::DecisionView {
    let Status::Deciding { decision } = &v.status else {
        panic!("a decision")
    };
    decision
}
fn answer(g: &mut Game, v: &PlayerView, i: usize) -> PlayerView {
    g.choose(
        g.answer(decision(v).id, CandidateIndex(u16::try_from(i).unwrap()))
            .unwrap(),
    );
    let mut last = None;
    while let Some(v) = g.next_view() {
        last = Some(v);
    }
    last.unwrap()
}
/// `file`, a save this build recorded, with each answer of the kind `now` under the name
/// `before`, which a format before version 10 had for the one question of that kind asked
/// here.
fn name_as_before_v10(file: &mut serde_json::Value, now: &str, before: &str) {
    for entry in file["log"].as_array_mut().unwrap() {
        if entry["choice"]["kind"] == now {
            entry["choice"]["kind"] = before.into();
        }
    }
}
fn setup(scenario: &Scenario) -> (Game, PlayerView) {
    let (table, state) = beryl_scenario::position(&CARDS, scenario).unwrap();
    let decks = Seat::BOTH.map(|p| {
        let mut cards = std::collections::BTreeMap::new();
        for &c in table.deck(p) {
            *cards.entry(table.def(c).name.to_owned()).or_insert(0u8) += 1;
        }
        cards.into_iter().map(|(n, c)| (c, n)).collect()
    });
    let file = ReplayFile {
        format_version: 6,
        legacy_waterbend: false,
        legacy_waterbend_mana: false,
        seed: 1,
        decks,
        human: 0,
        opponent: "random".to_owned(),
        stops: StopsSetting::EXACT,
        chooser: None,
        from: Some(Box::new(Checkpoint::of(&table, &state))),
        log: vec![],
        commit: None,
    };
    Game::from_replay(&CARDS, &file, SESSION).unwrap()
}
#[test]
fn old_cost_order_replays_complete_and_partial_casts() {
    let (mut g, mut v) = setup(&SETUP);
    while v.turn.step != beryl_session::protocol::Step::PrecombatMain {
        v = answer(&mut g, &v, 0);
    }
    let mana = decision(&v)
        .candidates
        .iter()
        .position(|c| c.label == "Tap Plains for {W}")
        .unwrap();
    v = answer(&mut g, &v, mana);
    let cast = decision(&v)
        .candidates
        .iter()
        .position(|c| matches!(c.option, Candidate::Cast { .. }))
        .unwrap();
    v = answer(&mut g, &v, cast);
    assert_eq!(decision(&v).kind, Kind::PaymentPlan);
    v = answer(&mut g, &v, 0);
    assert!(decision(&v).prompt.is(Purpose::ExileCost));
    while decision(&v).prompt.is(Purpose::ExileCost) {
        v = answer(&mut g, &v, 0);
    }
    let file = g.replay().clone();
    let current = serde_json::to_value(&file).unwrap();
    let unchanged = beryl_session::migrate::to_current(current.clone(), &CARDS).unwrap();
    assert_eq!(serde_json::to_value(&unchanged).unwrap(), current);
    assert_eq!(
        Game::replay_exactly(&CARDS, &unchanged, SESSION)
            .unwrap()
            .1
            .objects,
        v.objects
    );
    let mut old = serde_json::to_value(&file).unwrap();
    old["formatVersion"] = 6.into();
    name_as_before_v10(&mut old, "chooseObject", "exileCost");
    let log = old["log"].as_array_mut().unwrap();
    let plan = log
        .iter()
        .position(|e| e["choice"]["kind"] == "paymentPlan")
        .unwrap();
    let mut payment = log.remove(plan);
    payment["choice"]["continuation"] = true.into();
    let end = log
        .iter()
        .rposition(|e| e["choice"]["kind"] == "exileCost")
        .unwrap()
        + 1;
    log.insert(end, payment);
    let migrated = beryl_session::migrate::to_current(old.clone(), &CARDS).unwrap();
    let second =
        beryl_session::migrate::to_current(serde_json::to_value(&migrated).unwrap(), &CARDS)
            .unwrap();
    assert_eq!(migrated, second);
    let (mut replayed, again) = Game::replay_exactly(&CARDS, &migrated, SESSION).unwrap();
    assert_eq!(again.objects, v.objects);
    let undone = replayed.undo().unwrap();
    assert_eq!(decision(&undone).kind, Kind::PaymentPlan);
    assert_eq!(undone.players.you.graveyard.len(), 7);
    let (_, reloaded) = Game::replay_exactly(&CARDS, replayed.replay(), SESSION).unwrap();
    assert_eq!(decision(&reloaded).kind, Kind::PaymentPlan);
    assert_eq!(reloaded.objects, undone.objects);
    let cancelled = replayed.cancel().unwrap();
    assert!(
        cancelled
            .objects
            .iter()
            .any(|o| o.name.as_deref() == Some(names::ABHORRENT_OCULUS)
                && o.zone == beryl_session::protocol::ZoneName::Hand)
    );
    assert_eq!(cancelled.players.you.graveyard.len(), 7);
    let log = old["log"].as_array_mut().unwrap();
    let first = log
        .iter()
        .position(|e| e["choice"]["kind"] == "exileCost")
        .unwrap();
    log.truncate(first + 1);
    let migrated = beryl_session::migrate::to_current(old, &CARDS).unwrap();
    let second =
        beryl_session::migrate::to_current(serde_json::to_value(&migrated).unwrap(), &CARDS)
            .unwrap();
    assert_eq!(migrated, second);
    let (_, again) = Game::replay_exactly(&CARDS, &migrated, SESSION).unwrap();
    assert_eq!(decision(&again).kind, Kind::PaymentPlan);
    assert_eq!(again.players.you.graveyard.len(), 7);
}

#[test]
fn old_waterbend_saves_restore_a_required_implicit_tap() {
    const BEND: Scenario = Scenario::new("saved waterbend")
        .at(3, P0, TurnStep::Upkeep)
        .battlefield(
            P0,
            &[
                Permanent::new(names::AANGS_ICEBERG),
                Permanent::new(names::PLAINS),
                Permanent::new(names::PLAINS),
                Permanent::new(names::SNAPCASTER_MAGE),
            ],
        )
        .library(P0, &[names::ISLAND; 12])
        .library(P1, &[names::ISLAND; 12]);
    let (mut game, mut view) = setup(&BEND);
    while view.turn.step != beryl_session::protocol::Step::PrecombatMain {
        view = answer(&mut game, &view, 0);
    }
    let activate = decision(&view)
        .candidates
        .iter()
        .position(|c| {
            matches!(c.option, Candidate::Activate { .. }) && c.label.contains("Waterbend")
        })
        .unwrap();
    view = answer(&mut game, &view, activate);
    assert!(decision(&view).prompt.is(Purpose::Waterbend));
    assert!(!decision(&view).waterbend.as_ref().unwrap().ready);
    let after = answer(&mut game, &view, 0);
    let mut old = serde_json::to_value(game.replay()).unwrap();
    old["formatVersion"] = 7.into();
    name_as_before_v10(&mut old, "manaPayment", "waterbend");
    old["log"]
        .as_array_mut()
        .unwrap()
        .retain(|e| e["choice"]["kind"] != "waterbend");
    let migrated = beryl_session::migrate::to_current(old, &CARDS).unwrap();
    let (restored, resumed) = Game::replay_exactly(&CARDS, &migrated, SESSION).unwrap();
    assert_eq!(resumed.objects, after.objects);
    assert!(!restored.replay().legacy_waterbend);
    assert_eq!(
        Game::replay_exactly(&CARDS, restored.replay(), SESSION)
            .unwrap()
            .1
            .objects,
        after.objects
    );
}

#[test]
fn old_waterbend_saves_restore_an_automatic_mana_rock() {
    const BEND: Scenario = Scenario::new("saved rock waterbend")
        .at(3, P0, TurnStep::Upkeep)
        .battlefield(
            P0,
            &[
                Permanent::new(names::AANGS_ICEBERG),
                Permanent::new(names::PLAINS),
                Permanent::new(names::MANA_CRYPT),
            ],
        )
        .library(P0, &[names::ISLAND; 12])
        .library(P1, &[names::ISLAND; 12]);
    let (mut game, mut view) = setup(&BEND);
    while view.turn.step != beryl_session::protocol::Step::PrecombatMain {
        view = answer(&mut game, &view, 0);
    }
    let plains = decision(&view)
        .candidates
        .iter()
        .position(|c| c.label == "Tap Plains for {W}")
        .unwrap();
    view = answer(&mut game, &view, plains);
    let activate = decision(&view)
        .candidates
        .iter()
        .position(|c| {
            matches!(c.option, Candidate::Activate { .. }) && c.label.contains("Waterbend")
        })
        .unwrap();
    view = answer(&mut game, &view, activate);
    assert!(decision(&view).prompt.is(Purpose::Waterbend));
    let suggested = decision(&view).waterbend.as_ref().unwrap();
    assert!(!suggested.ready, "the rock needs to be picked manually");
    let rock = decision(&view)
        .candidates
        .iter()
        .position(|c| matches!(c.option, Candidate::Mana { .. }))
        .unwrap();
    assert_eq!(
        decision(&view).candidates[rock].label,
        "Tap Mana Crypt for {C}{C}"
    );
    let after = answer(&mut game, &view, rock);
    let mut old = serde_json::to_value(game.replay()).unwrap();
    old["formatVersion"] = 8.into();
    name_as_before_v10(&mut old, "manaPayment", "waterbend");
    old["log"]
        .as_array_mut()
        .unwrap()
        .retain(|e| e["choice"]["kind"] != "waterbend");
    let migrated = beryl_session::migrate::to_current(old, &CARDS).unwrap();
    let (restored, resumed) = Game::replay_exactly(&CARDS, &migrated, SESSION).unwrap();
    assert_eq!(resumed.objects, after.objects);
    assert!(!restored.replay().legacy_waterbend_mana);
    assert_eq!(
        Game::replay_exactly(&CARDS, restored.replay(), SESSION)
            .unwrap()
            .1
            .objects,
        after.objects
    );
}
