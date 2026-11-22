//! An inspection view of the same registry that mounts and routes features.

use kit_scene::Shortcut;

use super::{Placement, REGISTRY};

/// The location, source and declared keyboard bindings of a mounted feature.
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
            Some(Placement::Shape(shape)) => format!("prompt shape {shape}"),
            Some(Placement::GameLog) => "game log".to_owned(),
            Some(Placement::GameOver) => "game over".to_owned(),
            Some(Placement::Floating) => "HUD float".to_owned(),
            Some(Placement::PromptBar) => "prompt bar".to_owned(),
            Some(Placement::Candidates) => "prompt candidates".to_owned(),
            Some(Placement::TableMenu) => "table menu".to_owned(),
            Some(Placement::TableOverlay) => "table overlay".to_owned(),
            Some(Placement::TableLog) => "table log".to_owned(),
            Some(Placement::TableProposals) => "table proposals".to_owned(),
            None => "HUD control".to_owned(),
        },
        source: entry.source,
        shortcuts: entry.shortcuts,
    })
}
