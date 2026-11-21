//! The shade along the table's far edge: the web interface's `.far-shade`
//! (`ui/src/screens/styles/table.css`), which darkens the cloth under the opponent's hand and
//! life total. The web draws it as an element over the cloth and under every card, a radial
//! gradient on its stage, so Beryl 3D paints it into the cloth itself (`Draw::with_far_shade`):
//! the cards, drawn over the cloth, cover it as the web's cover its element.
//!
//! The phone layout's stage (`PhoneStageBoard`) draws no such shade.

use glam::Vec2;

use super::Window;

/// The web stage's height, in stage pixels (`STAGE.height` in `ui/src/game/perspective.ts`).
const STAGE_HEIGHT: f32 = 900.0;
/// The narrowest and widest the web's stage is drawn (`STAGE_WIDTHS` in
/// `ui/src/game/perspective.ts`): a window of another shape shows the cloth past the stage's
/// edges.
const STAGE_LEAST: f32 = 1440.0;
const STAGE_MOST: f32 = 2134.0;
/// Where `.far-shade` centres its gradient, in stage pixels from the stage's top left corner.
pub const CENTRE: Vec2 = Vec2::new(800.0, 48.0);
/// The gradient's radii across and down, in stage pixels (`ellipse 300px 85px`). The shader
/// holds their ratio (`FAR_SHADE_ASPECT`) and takes the first.
pub const RADII: Vec2 = Vec2::new(300.0, 85.0);

/// Where the far shade lies on a window: its centre and the radius across of its ellipse, in
/// physical pixels from the window's top left corner. None on the phone layout.
///
/// The web scales its stage whole into the middle of the window (`fitStage` in
/// `ui/src/game/Stage.tsx`): as wide as the window's shape within `STAGE_LEAST` and
/// `STAGE_MOST`, and 900 stage pixels high, by the smaller of the two ratios that fit it.
#[must_use]
pub fn place(window: Window) -> Option<(Vec2, f32)> {
    if window.phone() {
        return None;
    }
    let size = Vec2::new(window.size.width, window.size.height);
    let stage_width = (STAGE_HEIGHT * size.x / size.y).clamp(STAGE_LEAST, STAGE_MOST);
    let scale = (size.x / stage_width).min(size.y / STAGE_HEIGHT);
    let bleed = (size / scale - Vec2::new(stage_width, STAGE_HEIGHT)) / 2.0;
    let physical = scale * window.scale;
    Some(((CENTRE + bleed) * physical, RADII.x * physical))
}

#[cfg(test)]
mod tests {
    use super::*;
    use kit_ui::input::{Insets, Size};

    fn window(width: f32, height: f32, scale: f32) -> Window {
        Window {
            size: Size { width, height },
            scale,
            safe_area: Insets::default(),
        }
    }

    /// On the base stage's own shape the shade sits where the CSS puts it, doubled at scale 2.
    #[test]
    fn on_the_base_stage_the_shade_is_where_the_web_puts_it() {
        let (centre, radius) = place(window(1600.0, 900.0, 2.0)).expect("a desktop window");
        assert!(centre.abs_diff_eq(Vec2::new(1600.0, 96.0), 1e-3));
        assert!((radius - 600.0).abs() < 1e-3);
    }

    /// A window wider than the widest stage leaves cloth at its sides, and the shade stays over
    /// the stage's point 800, which then lies left of the window's middle.
    #[test]
    fn past_the_widest_stage_the_shade_keeps_its_stage_point() {
        let (centre, radius) = place(window(3000.0, 900.0, 1.0)).expect("a desktop window");
        assert!((radius - 300.0).abs() < 1e-3);
        assert!(centre.abs_diff_eq(Vec2::new(800.0 + (3000.0 - 2134.0) / 2.0, 48.0), 1e-3));
    }

    /// A window narrower than the narrowest stage scales the stage down and leaves cloth above
    /// and below it.
    #[test]
    fn under_the_narrowest_stage_the_shade_scales_down() {
        let (centre, radius) = place(window(1080.0, 900.0, 1.0)).expect("a desktop window");
        let scale = 1080.0 / 1440.0;
        assert!((radius - 300.0 * scale).abs() < 1e-3);
        let bleed_y = (900.0 / scale - 900.0) / 2.0;
        assert!((centre.y - (48.0 + bleed_y) * scale).abs() < 1e-3);
        assert!((centre.x - (800.0 + 0.0) * scale).abs() < 1e-3);
    }

    #[test]
    fn the_phone_draws_no_shade() {
        assert_eq!(place(window(393.0, 759.0, 3.0)), None);
    }
}
