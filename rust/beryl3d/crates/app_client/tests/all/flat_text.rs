//! Text in the prompt, including mana symbols and glyphs drawn while the display scale changes.

use app_client::Input;
use glam::Vec2;
use kit_frame::frame::{Frame, Material, Role};
use kit_probe::agent::checks::text_fits;
use kit_ui::input::{AssetReady, GlyphCell, Insets, Size};

use crate::flat_layer::{client, deciding, prompt};

/// Draws frames from `now` and gives every glyph the client wants a cell in atlas page `page`,
/// as many pixels square as its size so that a cell drawn at another size shows by how much it
/// was scaled, until the client wants no more. Returns the last frame.
fn fill_sized(client: &mut app_client::Client, page: u16, now: u64) -> Frame {
    let mut filled = false;
    for tick in now.. {
        client.frame(tick);
        let glyphs: Vec<GlyphCell> = client
            .wanted_glyphs()
            .map(|key| GlyphCell {
                key,
                rect: [0, 0, key.size, key.size],
                bearing: [0, -i16::try_from(key.size).unwrap()],
            })
            .collect();
        if glyphs.is_empty() {
            assert!(filled, "the client wants glyphs");
            return client.frame(tick + 1).clone();
        }
        filled = true;
        client.handle(Input::Assets(AssetReady::Glyphs { page, glyphs }));
    }
    unreachable!("the loop returns")
}

/// The glyph records of `frame`'s flat layer, each as its transform.
fn glyph_models(frame: &Frame) -> Vec<[f32; 12]> {
    (frame.draws[frame.layer as usize..].iter())
        .filter(|draw| draw.material == Material::Glyph as u8)
        .map(|draw| draw.model)
        .collect()
}

fn corners(frame: &Frame) -> Vec<[Vec2; 4]> {
    frame
        .resolved
        .iter()
        .map(|element| element.corners)
        .collect()
}

/// Whether two transforms differ by less than `by` in every number.
fn near(a: &[f32; 12], b: &[f32; 12], by: f32) -> bool {
    a.iter().zip(b).all(|(a, b)| (a - b).abs() < by)
}

#[test]
fn glyphs_at_a_new_scale_draw_from_the_old_cells_and_move_nothing() {
    let mut client = client();
    client.handle(deciding(prompt("Pay {1}{U}")));
    let before = fill_sized(&mut client, 0, 0);
    let drawn = glyph_models(&before);
    assert!(!drawn.is_empty());

    let surface = |scale| Input::Surface {
        size: Size {
            width: 1440.0,
            height: 900.0,
        },
        scale,
        safe_area: Insets::default(),
    };
    client.handle(surface(3.0));
    // Until the new page arrives, every glyph is drawn from its cell at the old size, scaled
    // to where and how large it was. A size in pixels is a whole number, so a glyph's size in
    // points may differ by up to a pixel's rounding at either scale.
    let scaled = client.frame(10).clone();
    let old_cells = glyph_models(&scaled);
    assert_eq!(old_cells.len(), drawn.len());
    assert!(drawn.iter().zip(&old_cells).all(|(a, b)| near(a, b, 0.5)));
    assert_eq!(corners(&scaled), corners(&before));

    let after = fill_sized(&mut client, 1, 11);
    assert_eq!(corners(&after), corners(&before), "no element moves");
    let new_cells = glyph_models(&after);
    assert_eq!(new_cells.len(), drawn.len());
    // The new cells are drawn exactly where and as large as the old ones were scaled to.
    assert!(
        old_cells
            .iter()
            .zip(&new_cells)
            .all(|(a, b)| near(a, b, 1e-3))
    );
    let pages: Vec<u16> = (after.draws[after.layer as usize..].iter())
        .filter(|draw| draw.material == Material::Glyph as u8)
        .map(|draw| draw.layer)
        .collect();
    assert!(pages.contains(&1), "the new page is drawn from: {pages:?}");
}

#[test]
fn a_title_with_mana_symbols_fits_its_box() {
    let mut client = client();
    let title = "Pay {2}{W}{U} or sacrifice a creature with power {X}";
    client.handle(deciding(prompt(title)));
    let frame = client.frame(0);
    let text = (frame.resolved.iter())
        .position(|element| element.role == Role::Text && frame.name(element.name) == title)
        .expect("the title is drawn");
    assert!(frame.lines.iter().any(|line| line.element as usize == text));
    assert!(text_fits(frame, &crate::flat_layer::context()).is_empty());
}
