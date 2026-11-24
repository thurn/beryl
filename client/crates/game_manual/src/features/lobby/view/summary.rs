//! Seats, score and the host's invite in the manual table lobby.

use super::{BODY, Fit, Message};
use beryl_protocol::ipc::LobbyView;
use kit_base::tokens::{color, space};
use kit_ui::element::{Align, Element, row, text};
use kit_ui::widgets::button;

/// The match (`LobbySummary.tsx`), once a second seat exists: the invite code, each seat's name
/// and deck, whether the guest is ready, the score, and who won the last game.
pub(super) fn render(view: &LobbyView, fit: Fit) -> Option<Element> {
    if view.seats.len() < 2 {
        return None;
    }
    let mut match_section = fit.section("Match");
    if let Some(code) = &view.invite {
        match_section = match_section.child(
            row()
                .gap(space::M)
                .align_self(Align::Stretch)
                .align_items(Align::Center)
                .child(
                    text(&format!("Invite code {code}"))
                        .style(BODY)
                        .color(color::TYPE)
                        .ellipsis()
                        .grow(1.0),
                )
                .child(button("Copy").on_press(Message::CopyInvite)),
        );
    }
    for seat in &view.seats {
        let name = seat.name.as_deref().unwrap_or("Guest");
        let deck = if seat.loaded {
            format!("{} main · {} sideboard", seat.main_count, seat.side_count)
        } else {
            "No deck loaded".to_owned()
        };
        let ready = if seat.seat.0 == 1 && seat.ready {
            " · Ready"
        } else {
            ""
        };
        match_section = match_section.child(
            text(&format!("{name}: {deck}{ready}"))
                .style(BODY)
                .color(color::TYPE),
        );
    }
    let score: Vec<String> = view
        .seats
        .iter()
        .map(|seat| seat.wins.to_string())
        .collect();
    match_section = match_section.child(
        text(&format!("Match score {}", score.join("–")))
            .style(BODY)
            .color(color::TYPE),
    );
    if view.game_number > 1 {
        let winner = (view.seats.iter()).find(|seat| Some(seat.seat) == view.last_winner);
        let result = winner.map_or_else(
            || "No result".to_owned(),
            |seat| {
                let name = seat.name.as_deref().unwrap_or("Guest");
                format!("{name} won game {}", view.game_number - 1)
            },
        );
        match_section = match_section.child(text(&result).style(BODY).color(color::TEXT_DIM));
    }
    if view.seats.get(1).is_some_and(|seat| seat.name.is_none()) {
        match_section = match_section.child(
            text("Waiting for guest…")
                .style(BODY)
                .color(color::TEXT_DIM),
        );
    }
    Some(match_section)
}
