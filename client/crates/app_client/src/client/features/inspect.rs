//! An inspection view of the mounted features and the parts composed directly into the HUD.

use app_screens::screens::Screen;
use beryl_protocol::prompt::shape_name;
use game_hud::hud::phase;
use game_hud::screens::{game, opening};
use kit_scene::Shortcut;

use super::{Placement, REGISTRY};

/// The location, source and declared keyboard bindings of a client feature.
#[derive(Clone, Debug)]
pub struct FeatureInfo {
    pub id: &'static str,
    pub placement: String,
    pub source: &'static str,
    pub shortcuts: &'static [Shortcut],
}

/// Lists every mounted feature without constructing its state.
pub fn registered_features() -> impl ExactSizeIterator<Item = FeatureInfo> {
    REGISTRY.iter().map(|entry| FeatureInfo {
        id: entry.id,
        placement: match entry.placement {
            Some(Placement::Screen(screen)) => format!("screen {}", screen.name()),
            Some(Placement::Sheet(sheet)) => format!("sheet {}", sheet.name()),
            Some(Placement::Shape(shape)) => format!("prompt shape {}", shape_name(shape)),
            Some(Placement::GameLog) => "game log".to_owned(),
            Some(Placement::GameOver) => "game over".to_owned(),
            Some(Placement::Floating) => "HUD float".to_owned(),
            Some(Placement::PromptBar) => "prompt bar".to_owned(),
            Some(Placement::Candidates) => "prompt candidates".to_owned(),
            Some(Placement::TableMenu) => "table menu".to_owned(),
            Some(Placement::TableOverlay) => "table overlay".to_owned(),
            Some(Placement::TableLog) => "table log".to_owned(),
            Some(Placement::TableProposals) => "table proposals".to_owned(),
            Some(Placement::TableTools) => "table tools".to_owned(),
            None => "HUD control".to_owned(),
        },
        source: entry.source,
        shortcuts: entry.shortcuts,
    })
}

/// Lists features built directly by the game screen composition, with source paths
/// supplied by the modules that build their views.
pub fn composed_features() -> impl ExactSizeIterator<Item = FeatureInfo> {
    [
        (Screen::Game.name(), "screen game", game::SOURCE),
        ("opening", "game overlay", opening::SOURCE),
        ("phase-indicator", "compact HUD", phase::SOURCE),
        ("menu-button", "compact HUD", phase::SOURCE),
    ]
    .into_iter()
    .map(|(id, placement, source)| FeatureInfo {
        id,
        placement: placement.to_owned(),
        source,
        shortcuts: &[],
    })
}
