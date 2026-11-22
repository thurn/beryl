//! Inputs to the furniture and overlays shown over a game.

use beryl_protocol::prompt::PromptView;
use beryl_protocol::script::{Subject, Turn};
use beryl_protocol::{PlayerView, StopsSetting};

use super::placement_scene::PlacementScene;
use crate::inspect::Viewed;
use crate::inspect::revealed::RevealedFace;
use crate::navigation::Sheet;
use game_table::faces::FaceList;
use game_table::profiles::Profile;
use game_table::table::Window;

/// The game view, playback state and UI state that the HUD reads.
#[derive(Clone, Copy, Debug)]
pub struct HudIn<'a> {
    pub data: &'a kit_base::data::Data,
    pub window: Window,
    pub profile: Profile<'a>,
    pub view: Option<&'a PlayerView>,
    pub played: Option<&'a PlayerView>,
    pub turn: Option<Turn>,
    pub prompt: Option<&'a PromptView>,
    /// The stops and full control preferences used to draw the game controls.
    pub stops: StopsSetting,
    pub sheet: Option<Sheet>,
    pub faces: Option<&'a FaceList>,
    pub viewing: Option<Viewed>,
    pub revealed: &'a [RevealedFace],
    pub carried: Option<Subject>,
    pub spinning: bool,
    pub placement_scene: Option<&'a PlacementScene>,
    /// The mode chooser's tray and scale when the app fits it to another stage.
    pub ways_tray: Option<(kit_base::geometry::Rect, f32)>,
}
