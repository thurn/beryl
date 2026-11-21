//! How the opening hand dims the life totals drawn over its backdrop.

use glam::Vec2;

/// How much of what lies under the opening's backdrop it hides at `y` points down a window
/// `height` points tall, from 0 to 1: its gradient's opacity there
/// (`tokens::gradient::OPENING_BACKDROP`, which runs from the window's top to its bottom). A
/// float drawn over the backdrop, such as a life total, takes the rest as its opacity, so that
/// it looks dimmed as the web's overlay dims the orb under it.
#[must_use]
pub(crate) fn backdrop_cover(y: f32, height: f32) -> f32 {
    let resolved = kit_base::tokens::gradient::OPENING_BACKDROP.resolve(Vec2::new(1.0, height));
    let at = (y / height.max(1.0)).clamp(0.0, 1.0);
    let alpha = |stop: usize| f32::from(resolved.colors[stop][3]) / f32::from(u8::MAX);
    let offsets = resolved.offsets;
    let Some(next) = offsets.iter().position(|&offset| offset >= at) else {
        return alpha(offsets.len() - 1);
    };
    if next == 0 {
        return alpha(0);
    }
    let (from, to) = (offsets[next - 1], offsets[next]);
    let share = if to > from {
        (at - from) / (to - from)
    } else {
        1.0
    };
    alpha(next - 1) + (alpha(next) - alpha(next - 1)) * share
}
