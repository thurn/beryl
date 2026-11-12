//! Manifest dread ([CR 701.62](https://yawgatog.com/resources/magic-rules/#R70162)).

use beryl_card::prompt;

use super::manifest::manifest;
use crate::card::{Flow, Prompt, Purpose};
use crate::event::Reason;
use crate::game::Game;
use crate::ids::Seat;
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
    let top = library::top(game.state, player, 2);
    if top.is_empty() {
        return Ok(());
    }
    zones::look_at(game, player, top);
    let prompt = Prompt::new(Purpose::ManifestDread)
        .count(2)
        .most(1)
        .places(prompt::Place::Battlefield, prompt::Place::Graveyard);
    let chosen = resolution.choose_object(game, player, top, prompt)?;
    manifest(game, chosen, player)?;
    let mut rest = top;
    rest.remove(chosen);
    zones::move_all(game, rest, Zone::Graveyard, Reason::Effect)?;
    Ok(())
}
