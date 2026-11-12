//! Manifest dread ([CR 701.62](https://yawgatog.com/resources/magic-rules/#R70162)).

use beryl_card::{Players, prompt};

use super::manifest::manifest;
use crate::bitboard::Bitboard;
use crate::card::{Flow, Prompt, Purpose, Stop};
use crate::choices;
use crate::event::Reason;
use crate::game::Game;
use crate::ids::Seat;
use crate::interpret::{Outcome, Run};
use crate::layout::Layout;
use crate::resolution::Resolution;
use crate::state::Zone;
use crate::{library, zones};

/// `player` manifests dread
/// ([CR 701.62a](https://yawgatog.com/resources/magic-rules/#R70162a)): "'Manifest dread'
/// means 'Look at the top two cards of your library. Manifest one of them, then put the
/// cards you looked at that were not manifested this way into your graveyard.'" With one
/// card in the library, that one is manifested; with none, nothing happens.
pub fn manifest_dread<L: Layout>(
    game: &mut Game<'_, L>,
    resolution: &mut Resolution<'_, L>,
    player: Seat,
) -> Flow {
    manifest_one_of_two(game, resolution, player).map(|_| ())
}

/// [`manifest_dread`], returning the card manifested.
fn manifest_one_of_two<L: Layout>(
    game: &mut Game<'_, L>,
    resolution: &mut Resolution<'_, L>,
    player: Seat,
) -> Result<Option<L::Slot>, Stop> {
    let top = library::top(game.state, player, 2);
    if top.is_empty() {
        return Ok(None);
    }
    zones::look_at(game, player, top);
    let prompt = Prompt::new(Purpose::ManifestDread)
        .count(2)
        .most(1)
        .places(prompt::Place::Battlefield, prompt::Place::Graveyard);
    let chosen = choices::pick(game, resolution, player, top, prompt)?;
    manifest(game, chosen, player)?;
    let mut rest = top;
    rest.remove(chosen);
    zones::move_all(game, rest, Zone::Graveyard, Reason::Effect)?;
    Ok(Some(chosen))
}

/// [`Effect::ManifestDread`](beryl_card::Effect::ManifestDread): "[Player] manifests dread"
/// (CR 701.62a), each in turn order. The outcome is the permanents manifested.
pub(crate) fn instruction<L: Layout>(
    run: &mut Run<'_, '_, L>,
    players: Players,
) -> Result<Outcome<L>, Stop> {
    let mut manifested = Bitboard::EMPTY;
    for player in run.in_turn_order(players) {
        let (game, resolution) = run.resolving();
        if let Some(card) = manifest_one_of_two(game, resolution, player)? {
            manifested.insert(card);
        }
    }
    Ok(run.now_in(manifested, Zone::Battlefield))
}
