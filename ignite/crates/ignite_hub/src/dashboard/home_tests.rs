use ignite_core::protocol::tickets::Status;

use super::super::data::{Card, Forecasts, HomeCard, by_recency};
use super::{AUTOMATIC, automatic_group, page};

fn ticket(id: &str, labels: &[&str]) -> HomeCard {
    HomeCard {
        card: Card {
            id: id.to_owned(),
            parent: None,
            title: format!("ticket {id}"),
            ticket_type: "bug".to_owned(),
            status: Status::Open,
            priority: 1,
            assignee: None,
            updated_at: "2026-11-17T00:00:00Z".to_owned(),
            pins: ignite_core::pins::Pins::default(),
        },
        labels: labels.iter().map(|label| (*label).to_owned()).collect(),
        machine: None,
    }
}

#[test]
fn a_card_names_the_machine_working_it_not_the_session() {
    let mut cards = [ticket("Tw1", &[])];
    cards[0].card.status = Status::InProgress;
    cards[0].card.assignee = Some("local_0123456789abcdef".to_owned());
    cards[0].machine = Some("multivac.local".to_owned());
    let body = page("/", &cards, &Forecasts::default(), "0").body;
    assert!(body.contains("Multivac"), "{body}");
    assert!(!body.contains("session 012345"), "{body}");
}

fn group_of(labels: &[&str]) -> Option<&'static str> {
    let labels: Vec<String> = labels.iter().map(|label| (*label).to_owned()).collect();
    automatic_group(&labels).map(|index| AUTOMATIC[index].0)
}

#[test]
fn tickets_the_hub_files_are_grouped_by_the_labels_it_puts_on_them() {
    // The labels `upkeep` files with: a justiciar ticket also carries `hub` and a tier, and the
    // incident group comes first.
    assert_eq!(
        group_of(&["justiciar", "tier:opus", "hub"]),
        Some("Incidents")
    );
    assert_eq!(
        group_of(&["regression", "check:rust", "hub"]),
        Some("Regressions")
    );
    assert_eq!(
        group_of(&["flaky", "check:ui", "hub"]),
        Some("Flaky checks")
    );
    assert_eq!(
        group_of(&["machine", "needs:jarvis", "hub"]),
        Some("Machine faults")
    );
    assert_eq!(group_of(&["hub"]), Some("Filed by the hub"));
}

#[test]
fn labels_nothing_files_any_more_form_no_group() {
    assert_eq!(group_of(&["role:justiciar"]), None);
    assert_eq!(group_of(&["role:steward"]), None);
    assert_eq!(group_of(&["tier:opus"]), None);
}

#[test]
fn imported_tickets_keep_their_groups() {
    assert_eq!(group_of(&["flaky-check"]), Some("Flaky checks"));
    assert_eq!(group_of(&["ci-incident"]), Some("Incidents"));
    assert_eq!(group_of(&["b3d-sweep"]), Some("Sweep and nightly failures"));
}

#[test]
fn the_home_page_shows_justiciar_tickets_under_incidents() {
    let cards = [
        ticket("Tj1", &["justiciar", "hub"]),
        ticket("Tr1", &["regression", "hub"]),
        ticket("Tt1", &[]),
    ];
    let body = page("/", &cards, &Forecasts::default(), "0").body;
    assert!(body.contains("Filed automatically"), "{body}");
    assert!(body.contains(">Incidents"), "{body}");
    assert!(body.contains(">Regressions"), "{body}");
    assert!(!body.contains("Slow CI failures"), "{body}");
    // The plain ticket is under Tickets, and the two filed ones are not.
    assert!(body.contains("id=\"tickets\""), "{body}");
}

#[test]
fn the_most_recently_updated_ticket_comes_first_whatever_its_status() {
    let mut cards = [
        ticket("Told", &[]),
        ticket("Tnew", &[]),
        ticket("Tmid", &[]),
    ];
    cards[0].card.updated_at = "2026-11-01T00:00:00Z".to_owned();
    cards[1].card.updated_at = "2026-11-18T00:00:00Z".to_owned();
    cards[1].card.status = Status::Blocked;
    cards[2].card.updated_at = "2026-11-10T00:00:00Z".to_owned();
    cards[2].card.priority = 3;
    cards.sort_by(|left, right| by_recency(&left.card, &right.card));
    let ids: Vec<&str> = cards.iter().map(|card| card.card.id.as_str()).collect();
    assert_eq!(ids, ["Tnew", "Tmid", "Told"]);
}
