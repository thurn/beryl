//! Tests of the formats' rules.

use super::*;

#[test]
fn each_getter_answers_for_both_formats() {
    for format in [Format::CONSTRUCTED, Format::SHARED_DECK] {
        assert_eq!(format.seats(), 2);
        assert_eq!(format.teams(), Teams::None);
        assert_eq!(format.starting_life(), 20);
        assert_eq!(format.opening_hand(), 7);
        assert_eq!(format.maximum_hand(), 7);
        assert_eq!(format.lands_each_turn(), 1);
        assert_eq!(format.starting_player(), StartingPlayer::PlayOrDraw);
        assert_eq!(format.first_draw(), FirstDraw::SkippedByStartingPlayer);
        assert_eq!(format.turn(), &TurnStep::ALL);
        assert_eq!(
            format.losses(),
            Losses {
                life: 0,
                empty_library: true
            }
        );
    }

    let constructed = Format::CONSTRUCTED;
    assert_eq!(constructed.zones(), ZoneRule::PerPlayer);
    assert_eq!(
        constructed.simultaneous_draws(),
        SimultaneousDraws::Together
    );
    assert_eq!(constructed.mulligan(), Mulligan::London);
    assert_eq!(
        constructed.decks(),
        &[DeckRole::Library, DeckRole::Sideboard]
    );
    assert_eq!(constructed.maximum_sideboard(), 15);

    let shared = Format::SHARED_DECK;
    assert_ne!(shared, constructed);
    assert_eq!(shared.zones(), ZoneRule::SharedLibraryAndGraveyard);
    assert_eq!(
        shared.simultaneous_draws(),
        SimultaneousDraws::OneAtATimeFromActivePlayer
    );
    assert_eq!(shared.mulligan(), Mulligan::LondonWithFreeRedraw);
    assert_eq!(shared.decks(), &[DeckRole::SharedLibrary]);
    assert_eq!(shared.maximum_sideboard(), 0);
}
