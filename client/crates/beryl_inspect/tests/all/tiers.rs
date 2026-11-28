//! On every gated board the visual tiers share layout and motion, including arrow paths and
//! endpoints. The tiers draw arrows with different primitives (docs/beryl-3d.md, "The Enhanced
//! tier").

use beryl_inspect::gate::{BOARDS, Playback};
use kit_frame::frame::{Draw, Frame, Material, Vfx, sprites};
use kit_ui::input::Tier;

/// Baseline arrows sample the core and lit columns of the ribbon cell. Its other columns
/// supply a generic glow sprite used by effects that should still compare across tiers.
fn baseline_line(draw: &Draw) -> bool {
    let [x, y, width, height] = draw.uv;
    let [cell_x, cell_y, _, cell_height] = sprites::RIBBON_CELL.rect;
    let first = cell_x + sprites::RIBBON_LIT_FIRST;
    let end = first + sprites::RIBBON_LIT_COLUMNS;
    draw.material == Material::Glow as u8
        && draw.layer == sprites::RIBBON_CELL.page
        && (draw.uv == sprites::RIBBON_CORE.rect
            || (y == cell_y
                && height == cell_height
                && (first..end).contains(&x)
                && width <= end - x))
}

fn enhanced_line(draw: &Draw) -> bool {
    draw.material == Material::Vfx as u8
        && (draw.uv[0] == Vfx::LineRibbon as u16 || draw.uv[0] == Vfx::LineHead as u16)
}

fn shared_draws(frame: &Frame) -> Vec<&Draw> {
    let mut draws: Vec<&Draw> = frame
        .draws
        .iter()
        .enumerate()
        .filter_map(|(index, draw)| {
            if frame.tier_only.get(index).copied().unwrap_or(false) {
                return None;
            }
            let keep = match frame.tier {
                Tier::Baseline => {
                    draw.material != Material::ContactShadow as u8 && !baseline_line(draw)
                }
                Tier::Enhanced => {
                    draw.material != Material::Particle as u8
                        && draw.material != Material::Vfx as u8
                        && !enhanced_line(draw)
                }
            };
            keep.then_some(draw)
        })
        .collect();
    // `batch_bands` may reorder non-overlapping shared records differently around tier-only
    // records (`BoxShadow` vs `Material::Vfx`) in the same band.
    let key = |d: &Draw| {
        (
            d.material,
            d.flags,
            d.layer,
            d.uv,
            d.tint,
            d.model.map(f32::to_bits),
            d.params.map(f32::to_bits),
        )
    };
    draws.sort_unstable_by_key(|d| key(d));
    draws
}

#[test]
fn every_gated_board_resolves_and_draws_the_same_in_both_tiers() {
    for name in BOARDS {
        let mut baseline = Playback::new(name)
            .expect("a gated board")
            .with_tier(Tier::Baseline);
        let mut enhanced = Playback::new(name)
            .expect("a gated board")
            .with_tier(Tier::Enhanced);
        while let Some(moment) = baseline.moment() {
            assert_eq!(enhanced.moment(), Some(moment), "{name}");
            baseline.handle();
            enhanced.handle();
            let frame = moment.frame;
            let (base, enhanced_frame) = (baseline.frame(), enhanced.frame());
            assert_eq!(base.tier, Tier::Baseline);
            assert_eq!(enhanced_frame.tier, Tier::Enhanced);
            assert!(
                base.resolved == enhanced_frame.resolved,
                "{name} frame {frame}: the resolved elements differ"
            );
            assert!(
                base.names == enhanced_frame.names,
                "{name} frame {frame}: the names differ"
            );
            let base_draws = shared_draws(base);
            let enhanced_draws = shared_draws(enhanced_frame);
            if let Some(index) = (0..base_draws.len().max(enhanced_draws.len()))
                .find(|&index| base_draws.get(index) != enhanced_draws.get(index))
            {
                panic!(
                    "{name} frame {frame}: record {index} differs\n  Baseline: {:?}\n  Enhanced: {:?}",
                    base_draws.get(index),
                    enhanced_draws.get(index),
                );
            }
        }
        assert_eq!(enhanced.moment(), None, "{name}");
    }
}

/// A step down in the middle of a `vanish` keeps its timing and the positions of its effects.
#[test]
fn a_step_down_in_the_middle_of_a_vanish_draws_what_baseline_draws() {
    step_down_during(|frame| frame.load.particles > 0, "particles");
}

/// A step down while an arrow is visible switches it to its Baseline ribbon immediately.
#[test]
fn a_step_down_while_a_line_is_visible_draws_what_baseline_draws() {
    step_down_during(
        |frame| {
            frame
                .draws
                .iter()
                .any(|draw| enhanced_line(draw) && draw.tint[3] > 0)
        },
        "a visible line",
    );
}

/// Compare every frame after a tier change with the same playback run in Baseline from the start.
fn step_down_during(effect_is_visible: impl Fn(&Frame) -> bool, effect: &str) {
    let mut compared_after_step = false;
    for name in BOARDS {
        let mut baseline = Playback::new(name)
            .expect("a gated board")
            .with_tier(Tier::Baseline);
        let mut stepped = Playback::new(name)
            .expect("a gated board")
            .with_tier(Tier::Enhanced);
        let mut stepped_at = None;
        while let Some(moment) = baseline.moment() {
            assert_eq!(stepped.moment(), Some(moment), "{name}");
            baseline.handle();
            stepped.handle();
            let frame = moment.frame;
            let base = baseline.frame();
            let drawn = stepped.frame();
            if stepped_at.is_some() {
                assert_eq!(drawn.tier, Tier::Baseline, "{name} frame {frame}");
                assert!(
                    base.resolved == drawn.resolved && base.names == drawn.names,
                    "{name} frame {frame}: the elements differ after the step down"
                );
                assert!(
                    base.draws == drawn.draws,
                    "{name} frame {frame}: the records differ after the step down"
                );
                compared_after_step = true;
            } else if effect_is_visible(drawn) {
                stepped_at = Some(frame);
                stepped.set_tier(Tier::Baseline);
            }
        }
    }
    assert!(
        compared_after_step,
        "no gated board stepped down during {effect}"
    );
}
