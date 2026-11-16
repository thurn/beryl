//! Panels, glyphs and icons of the flat layer: text rasterized by `beryl_assets`' glyph worker
//! draws its ink where the glyph has it and nothing in its counters, an icon from the same
//! worker draws its tint on its ink and nothing in its holes, and a panel draws its fill and
//! border but leaves the corners outside its radius untouched. Every test runs on a device with
//! wgpu's default limits and on one with its downlevel limits.

use std::sync::mpsc;

use beryl_assets::glyphs::GlyphWorker;
use beryl_assets::packer::PAGE_SIZE;
use beryl_assets::{Prepared, Upload};
use beryl_render::{Limits, Renderer, Target};
use beryl_scene::frame::{Band, Draw, Frame, FrameBuilder, Sprite};
use beryl_scene::input::{AssetReady, GlyphCell, GlyphKey};
use beryl_scene::table::{CARD_HEIGHT, CARD_WIDTH};
use beryl_scene::text::fonts::Font;
use beryl_scene::widgets::icons::GEAR;
use glam::camera::rh::{proj::directx, view::look_at_mat4};
use glam::{Affine3A, Mat4, Quat, Vec2, Vec3};

const BOTH: [Limits; 2] = [Limits::Default, Limits::Downlevel];
/// The target's side in pixels.
const SIDE: u32 = 200;
/// The size of the text, in pixels.
const TEXT_SIZE: u16 = 64;

/// A camera that draws the point (x, y) at pixel (x + 100, 100 - y), so that one unit is one
/// pixel.
fn camera() -> Mat4 {
    directx::orthographic(-100.0, 100.0, -100.0, 100.0, 0.0, 2000.0)
        * look_at_mat4(Vec3::new(0.0, 0.0, 1000.0), Vec3::ZERO, Vec3::Y)
}

/// The transform that stretches the unit quad over the pixels from `corner` (top left) and
/// `size` large.
fn over(corner: Vec2, size: Vec2) -> Affine3A {
    let centre = corner + size / 2.0;
    Affine3A::from_scale_rotation_translation(
        size.extend(1.0),
        Quat::IDENTITY,
        Vec3::new(centre.x - 100.0, 100.0 - centre.y, 10.0),
    )
}

/// Draws `records`, each with its band, and returns the target's RGBA pixels.
fn draw(renderer: &mut Renderer, records: &[(Band, Draw)]) -> Vec<u8> {
    let mut frame = Frame::with_capacity(0, records.len(), 0);
    let mut builder = FrameBuilder::new(&mut frame);
    builder.camera(camera());
    for &(band, record) in records {
        builder.draw(band, record);
    }
    let frame = builder.finish();
    let mut target = Target::offscreen(renderer, SIDE, SIDE);
    renderer.render(frame, &mut target);
    renderer.read(&target)
}

fn at(pixels: &[u8], x: u32, y: u32) -> [u8; 3] {
    let i = ((y * SIDE + x) * 4) as usize;
    [pixels[i], pixels[i + 1], pixels[i + 2]]
}

fn assert_near(found: [u8; 3], expected: [u8; 3], tolerance: u8, what: &str) {
    assert!(
        found
            .iter()
            .zip(expected)
            .all(|(&a, b)| a.abs_diff(b) <= tolerance),
        "{what}: found {found:?}, expected {expected:?}"
    );
}

/// A background covering the whole target, drawn opaque, of `rgb`.
fn background(rgb: [u8; 3]) -> (Band, Draw) {
    let size = Vec3::new(400.0 / CARD_WIDTH, 400.0 / CARD_HEIGHT, 1.0);
    let transform = Affine3A::from_scale_rotation_translation(size, Quat::IDENTITY, Vec3::ZERO);
    (
        Band::Opaque,
        Draw::table(transform, [rgb[0], rgb[1], rgb[2], 255]),
    )
}

/// The glyphs of `text` in Open Sans at `TEXT_SIZE` pixels, rasterized by the glyph worker: the
/// page they are on and each glyph's cell, in order.
fn rasterize(text: &str) -> (Prepared, Vec<GlyphCell>) {
    let font = swash::FontRef::from_index(Font::OpenSans.file().data, 0).expect("Open Sans");
    let keys: Vec<GlyphKey> = text
        .chars()
        .map(|character| GlyphKey {
            font: Font::OpenSans as u8,
            weight: 400,
            glyph: font.charmap().map(character),
            size: TEXT_SIZE,
        })
        .collect();
    let (out, pages) = mpsc::channel();
    let worker = GlyphWorker::spawn(out, || {}, || {});
    worker.request(keys.clone());
    worker.flush();
    let page = pages.try_recv().expect("the worker sent a page");
    let AssetReady::Glyphs { glyphs, .. } = &page.ready else {
        panic!("a glyph worker sends glyphs");
    };
    let cells = keys
        .iter()
        .map(|key| *glyphs.iter().find(|cell| cell.key == *key).expect("a cell"))
        .collect();
    (page, cells)
}

#[test]
fn white_text_on_a_black_panel_is_inked_in_the_stem_and_empty_in_the_bowl() {
    let (page, cells) = rasterize("Pay");
    let Upload::AtlasPage {
        page: number,
        pixels: atlas,
    } = &page.upload
    else {
        panic!("a glyph worker sends glyph pages");
    };
    let coverage = |cell: &GlyphCell, x: u16, y: u16| {
        atlas
            [usize::from(cell.rect[1] + y) * usize::from(PAGE_SIZE) + usize::from(cell.rect[0] + x)]
    };
    // The P's stem: the most inked texel of its middle row. Its bowl: the middle of the
    // empty run between the stem and the bowl's right side, a quarter of the way down.
    let p = cells[0];
    let [_, _, width, height] = p.rect;
    let stem = (0..width)
        .max_by_key(|&x| coverage(&p, x, height / 2))
        .expect("the P has width");
    let row = height / 4;
    let inked: Vec<bool> = (0..width).map(|x| coverage(&p, x, row) > 128).collect();
    let left = inked
        .iter()
        .position(|&ink| !ink)
        .expect("the bowl is empty somewhere");
    let right = left
        + inked[left..]
            .iter()
            .position(|&ink| ink)
            .expect("the bowl closes");
    let bowl = usize::midpoint(left, right) as u16;
    assert_eq!(coverage(&p, bowl, row), 0, "the bowl's middle has no ink");

    // The panel from (10, 40) to (190, 160); the text's pen starts at (30, 120) on the baseline.
    let panel = (
        Band::Layer,
        Draw::panel(
            over(Vec2::new(10.0, 40.0), Vec2::new(180.0, 120.0)),
            [0, 0, 0, 255],
            [0, 0, 0, 255],
            Vec2::new(180.0, 120.0),
            8.0,
            0.0,
        ),
    );
    let mut records = vec![background([30, 60, 50]), panel];
    let mut pen = 30.0;
    let mut corners = Vec::new();
    for cell in &cells {
        let corner = Vec2::new(
            pen + f32::from(cell.bearing[0]),
            120.0 + f32::from(cell.bearing[1]),
        );
        corners.push(corner);
        let size = Vec2::new(f32::from(cell.rect[2]), f32::from(cell.rect[3]));
        let sprite = Sprite {
            page: *number,
            rect: cell.rect,
        };
        records.push((
            Band::Layer,
            Draw::glyph(over(corner, size), [255; 4], sprite),
        ));
        pen += size.x + 4.0;
    }
    for limits in BOTH {
        let mut renderer = Renderer::headless(limits).expect("a GPU adapter and device");
        assert!(
            renderer.upload_page(&page),
            "{limits:?}: the page was not uploaded"
        );
        let pixels = draw(&mut renderer, &records);
        let screen = |texel: u16, axis: f32| (axis + f32::from(texel)) as u32;
        let what = format!("{limits:?}");
        assert_near(
            at(
                &pixels,
                screen(stem, corners[0].x),
                screen(height / 2, corners[0].y),
            ),
            [255; 3],
            8,
            &format!("{what}: the P's stem"),
        );
        assert_near(
            at(
                &pixels,
                screen(bowl, corners[0].x),
                screen(row, corners[0].y),
            ),
            [0; 3],
            8,
            &format!("{what}: the P's bowl"),
        );
    }
}

#[test]
fn a_panel_draws_its_fill_and_border_and_leaves_its_corners_untouched() {
    let table = [30, 60, 50];
    let fill = [200, 40, 40];
    let border = [240, 220, 90];
    // A panel from (40, 40) to (160, 160), with corners of radius 30 and a border 6 wide.
    let panel = (
        Band::Layer,
        Draw::panel(
            over(Vec2::splat(40.0), Vec2::splat(120.0)),
            [fill[0], fill[1], fill[2], 255],
            [border[0], border[1], border[2], 255],
            Vec2::splat(120.0),
            30.0,
            6.0,
        ),
    );
    for limits in BOTH {
        let mut renderer = Renderer::headless(limits).expect("a GPU adapter and device");
        let pixels = draw(&mut renderer, &[background(table), panel]);
        let what = format!("{limits:?}");
        assert_eq!(
            at(&pixels, 41, 41),
            table,
            "{what}: the corner outside the radius"
        );
        assert_eq!(
            at(&pixels, 158, 158),
            table,
            "{what}: the corner outside the radius"
        );
        assert_near(at(&pixels, 100, 100), fill, 1, &format!("{what}: the fill"));
        assert_near(
            at(&pixels, 100, 42),
            border,
            1,
            &format!("{what}: the top border"),
        );
        assert_near(
            at(&pixels, 42, 100),
            border,
            1,
            &format!("{what}: the left border"),
        );
        assert_eq!(at(&pixels, 100, 30), table, "{what}: outside the panel");
    }
}

#[test]
fn the_gear_icon_is_tinted_on_its_teeth_and_empty_in_its_hole() {
    // The gear at four pixels to a point: an 88-pixel cell.
    let (out, pages) = mpsc::channel();
    let worker = GlyphWorker::spawn(out, || {}, || {});
    worker.icons(4.0);
    worker.flush();
    let page = pages.try_recv().expect("the worker sent the icons' page");
    let (
        AssetReady::Icons { cells, .. },
        Upload::AtlasPage {
            page: number,
            pixels: atlas,
        },
    ) = (&page.ready, &page.upload)
    else {
        panic!("only icons were asked for");
    };
    let gear = *cells
        .iter()
        .find(|cell| cell.icon == GEAR)
        .expect("the gear");
    let [x, y, side, _] = gear.rect;
    let coverage = |dx: u16, dy: u16| {
        atlas[usize::from(y + dy) * usize::from(PAGE_SIZE) + usize::from(x + dx)]
    };
    // A texel the gear covers fully, found along its middle row, and the hole at its centre.
    let middle = side / 2;
    let inked = (0..side)
        .find(|&dx| coverage(dx, middle) == 255)
        .expect("the gear's middle row crosses a tooth");
    assert_eq!(coverage(middle, middle), 0, "the hole is empty");

    // The cell drawn at one texel to a pixel, its top left corner at (50, 50), in gold.
    let gold = [228, 162, 66];
    let table = [30, 60, 50];
    let corner = Vec2::splat(50.0);
    let sprite = Sprite {
        page: *number,
        rect: gear.rect,
    };
    let icon = (
        Band::Layer,
        Draw::icon(
            over(corner, Vec2::splat(f32::from(side))),
            [gold[0], gold[1], gold[2], 255],
            sprite,
        ),
    );
    for limits in BOTH {
        let mut renderer = Renderer::headless(limits).expect("a GPU adapter and device");
        assert!(
            renderer.upload_page(&page),
            "{limits:?}: the page was not uploaded"
        );
        let pixels = draw(&mut renderer, &[background(table), icon]);
        let screen = |texel: u16| 50 + u32::from(texel);
        assert_near(
            at(&pixels, screen(inked), screen(middle)),
            gold,
            8,
            &format!("{limits:?}: a tooth"),
        );
        assert_near(
            at(&pixels, screen(middle), screen(middle)),
            table,
            8,
            &format!("{limits:?}: the hole"),
        );
    }
}
