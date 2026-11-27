//! The flat layer's icons, rasterized by the glyph worker: what a host asks for and what comes
//! back.

#[path = "../../../../test_profiles.rs"]
mod test_profiles;

use test_profiles::SHIPPED;

use std::collections::BTreeSet;
use std::sync::mpsc;

use beryl_assets::glyphs::{GlyphWorker, IconSet, prepare_latin};
use beryl_assets::icons::icons_rasterized_on_this_thread;
use beryl_assets::packer::{ATLAS_PAGES, PAGE_SIZE};
use beryl_assets::{Prepared, Upload};
use beryl_host::spawn_glyph_worker;
use game_hud::art::{
    self,
    orb::{self, FIRST_ICON},
};
use kit_base::atlas::{AssetReady, GlyphCell, GlyphKey, IconCell, IconId};
use kit_base::data::profiles::Profiles;
use kit_probe::driver::atlas::stub_icons;
use kit_text::text::fonts::Font;
use kit_ui::art::icons::ALL;
use kit_ui::widgets::icon_shadows;
use kit_ui::widgets::icons::{GEAR, STEPS, STEPS_OUTLINED, atlas_size};
use swash::FontRef;

/// Reads the pages queued by a worker after flushing or joining it.
fn pages(results: &mpsc::Receiver<Prepared>) -> Vec<(Vec<u8>, Vec<IconCell>)> {
    let mut pages = Vec::new();
    for prepared in results.try_iter() {
        let Upload::AtlasPage { page, pixels } = prepared.upload else {
            panic!("the glyph worker sent a face");
        };
        let AssetReady::Icons {
            page: ready_page,
            cells,
        } = prepared.ready
        else {
            panic!("only icons were asked for");
        };
        assert_eq!(page, ready_page);
        pages.push((pixels, cells));
    }
    pages
}

fn profiles() -> Profiles {
    crate::test_data::data().profiles.clone()
}

/// Every icon at `scale`, asked for alone.
fn rasterize(scale: f32) -> Vec<(Vec<u8>, Vec<IconCell>)> {
    let (out, results) = mpsc::channel();
    let worker = spawn_glyph_worker(out, || {}, Some(&profiles()));
    worker.icons(scale);
    drop(worker);
    pages(&results)
}

fn texels(pixels: &[u8], cell: &IconCell) -> Vec<u8> {
    let [x, y, width, height] = cell.rect.map(usize::from);
    let stride = usize::from(PAGE_SIZE);
    (y..y + height)
        .flat_map(|row| pixels[row * stride + x..row * stride + x + width].to_vec())
        .collect()
}

#[test]
fn every_icon_id_has_artwork_in_the_order_of_the_ids() {
    // The ids from `FIRST_ICON` on are the layers of the life orbs' art.
    assert_eq!(ALL.len(), usize::from(FIRST_ICON));
    let last_orb = orb::icon(orb::ORIENTATIONS[1], orb::LAYERS.len() - 1);
    assert_eq!(last_orb.0 + 1, FIRST_ICON + orb::ICONS);
    for (id, artwork) in (0..FIRST_ICON).zip(&ALL) {
        assert_eq!(artwork.icon, IconId(id), "the artwork at index {id}");
        assert_eq!(artwork.size, atlas_size(artwork.icon), "{:?}", artwork.icon);
    }
}

#[test]
fn every_icon_rasterizes_to_a_cell_partly_inked_on_another_thread() {
    let pages = rasterize(2.0);
    assert_eq!(
        icons_rasterized_on_this_thread(),
        0,
        "an icon was rasterized on the asking thread"
    );
    let cells: Vec<(&[u8], &IconCell)> = pages
        .iter()
        .flat_map(|(pixels, cells)| cells.iter().map(move |cell| (pixels.as_slice(), cell)))
        .collect();
    let sharp = cells.iter().filter(|(_, cell)| cell.shadow == 0);
    let mut icons: Vec<IconId> = sharp.map(|(_, cell)| cell.icon).collect();
    icons.sort_unstable();
    // Every widget icon and the game's layered artwork, packed tallest first.
    let profiles = profiles();
    let layers = art::atlas_cells(&profiles).map(|cell| cell.icon);
    let mut expected: Vec<IconId> = (0..FIRST_ICON).map(IconId).chain(layers).collect();
    expected.sort_unstable();
    assert_eq!(icons, expected);
    // And a cell of each shadow of each chain of drop shadows an icon is drawn with.
    let mut shadows: Vec<(IconId, u8)> = (cells.iter())
        .filter(|(_, cell)| cell.shadow > 0)
        .map(|(_, cell)| {
            let (chain, stage) = icon_shadows::decode(cell.shadow).expect("a listed shadow");
            let side =
                icon_shadows::stage_side(cell.size, icon_shadows::shadows(chain), stage, 2.0);
            assert_eq!(
                cell.rect[2..],
                [side, side],
                "{:?} {}",
                cell.icon,
                cell.shadow
            );
            (cell.icon, cell.shadow)
        })
        .collect();
    shadows.sort_unstable();
    let mut listed: Vec<(IconId, u8)> = icon_shadows::cells().collect();
    listed.sort_unstable();
    assert_eq!(shadows, listed);
    for (pixels, cell) in cells {
        let texels = texels(pixels, cell);
        let inked = texels.iter().filter(|&&texel| texel > 0).count();
        assert!(inked > 0, "{:?} is blank", cell.icon);
        assert!(
            inked < texels.len(),
            "{:?} covers its whole cell",
            cell.icon
        );
        if let Some(layer) = cell.icon.0.checked_sub(FIRST_ICON) {
            let layer = &orb::LAYERS[usize::from(layer) % orb::LAYERS.len()];
            let [width, height] = [usize::from(cell.rect[2]), usize::from(cell.rect[3])];
            let at = |x, y| texels[y * width + x];
            let edge = (0..height).flat_map(|y| [at(0, y), at(width - 1, y)]);
            // The next band covers the vertical fade's cut edge.
            let across = (0..width)
                .filter(|_| !matches!(layer.ramp, orb::Ramp::Down(_)))
                .flat_map(|x| [at(x, 0), at(x, height - 1)]);
            assert!(
                edge.chain(across).all(|value| value < 16),
                "{cell:?} has ink at its edge"
            );
            assert!(
                texels.iter().any(|&value| value > 32),
                "{cell:?} has no ink"
            );
        }
    }
}

#[test]
fn every_shape_of_every_widget_icon_draws_ink() {
    let mut icons = Vec::new();
    for artwork in &ALL {
        for shape in artwork.shapes {
            icons.push(kit_base::atlas::artwork::Artwork {
                icon: IconId(u16::try_from(icons.len()).unwrap()),
                shapes: std::slice::from_ref(shape),
                ..*artwork
            });
        }
    }
    let count = icons.len();
    let (out, results) = mpsc::channel();
    let worker = GlyphWorker::spawn(
        out,
        || {},
        || {},
        IconSet {
            icons,
            ..IconSet::default()
        },
    );
    worker.icons(2.0);
    drop(worker);
    let mut drawn = 0;
    for (pixels, cells) in pages(&results) {
        for cell in cells {
            assert!(
                texels(&pixels, &cell).iter().any(|&value| value > 0),
                "path {} drew no ink",
                cell.icon.0
            );
            drawn += 1;
        }
    }
    assert_eq!(drawn, count);
}

#[test]
fn the_gear_at_scale_two_is_twice_the_side_of_the_gear_at_scale_one() {
    let side = |scale: f32| {
        let pages = rasterize(scale);
        let gear = pages
            .iter()
            .flat_map(|(_, cells)| cells)
            .find(|cell| cell.icon == GEAR)
            .copied()
            .expect("the gear is sent");
        assert_eq!(gear.size, 22, "an icon button's glyph is 22 points");
        assert_eq!(gear.rect[2], gear.rect[3], "a cell is square");
        gear.rect[2]
    };
    assert_eq!(side(1.0), 22);
    assert_eq!(side(2.0), 44);
}

#[test]
fn a_new_scale_adds_new_cells_and_the_same_scale_adds_none() {
    let (out, results) = mpsc::channel();
    let worker = spawn_glyph_worker(out, || {}, Some(&profiles()));
    worker.icons(1.0);
    worker.flush();
    let cells = || {
        results
            .try_iter()
            .flat_map(|prepared| match prepared.ready {
                AssetReady::Icons { cells, page } => {
                    cells.into_iter().map(move |cell| (page, cell))
                }
                _ => panic!("only icons were asked for"),
            })
            .filter(|(_, cell)| cell.shadow == 0 && cell.icon.0 < FIRST_ICON)
            .collect::<Vec<_>>()
    };
    let old = cells();
    assert_eq!(old.len(), ALL.len());
    worker.icons(1.0);
    worker.flush();
    assert!(
        results.try_recv().is_err(),
        "the same scale is not sent again"
    );
    worker.icons(2.0);
    worker.flush();
    let new = cells();
    assert_eq!(new.len(), ALL.len());
    // The old cells stay where they were, since the client draws them until the new arrive.
    for (new_page, new) in new {
        let (old_page, old) = old
            .iter()
            .find(|(_, old)| old.icon == new.icon)
            .expect("the icon was delivered at scale 1");
        assert_eq!(new.rect[2], old.rect[2] * 2, "{:?}", new.icon);
        let apart = new_page != *old_page
            || new.rect[1] >= old.rect[1] + old.rect[3]
            || new.rect[0] >= old.rect[0] + old.rect[2]
            || old.rect[1] >= new.rect[1] + new.rect[3]
            || old.rect[0] >= new.rect[0] + new.rect[2];
        assert!(apart, "{:?} was drawn over its old cell", new.icon);
    }
}

#[test]
fn every_icon_and_its_shadows_fit_in_the_atlas_at_a_phones_scale() {
    // At three pixels a point, packed tallest first, the icons, their drop shadows' cells and
    // the orbs' layers take five of the atlas's seven pages, which leaves two for glyphs.
    // Packed in the order of their ids, the orbs' layers and the icons alone took six.
    let pages = rasterize(3.0);
    assert!(pages.len() <= 5, "the icons took {} pages", pages.len());
}

#[test]
fn the_stub_and_worker_deliver_the_same_icon_sizes() {
    let sizes = |cells: Vec<IconCell>| {
        let mut sizes: Vec<_> = cells
            .into_iter()
            .map(|cell| {
                (
                    cell.icon,
                    cell.shadow,
                    cell.size,
                    [cell.rect[2], cell.rect[3]],
                )
            })
            .collect();
        sizes.sort_unstable();
        sizes
    };
    for scale in [1.0, 2.0, 3.0] {
        let AssetReady::Icons { cells: stub, .. } =
            stub_icons(scale, art::atlas_cells(&profiles()))
        else {
            panic!("the stub supplies icon cells");
        };
        let real = rasterize(scale)
            .into_iter()
            .flat_map(|(_, cells)| cells)
            .collect();
        assert_eq!(sizes(stub), sizes(real), "display scale {scale}");
    }
}

fn open_sans(character: char, size: u16) -> GlyphKey {
    let font = FontRef::from_index(Font::OpenSans.file().data, 0).unwrap();
    GlyphKey {
        blur: 0,
        font: Font::OpenSans as u8,
        weight: 400,
        glyph: font.charmap().map(character),
        size,
    }
}

#[test]
fn an_edited_profile_adds_its_orb_cells_at_the_current_display_scale() {
    let profiles = profiles();
    let (out, results) = mpsc::channel();
    let worker = spawn_glyph_worker(out, || {}, Some(&profiles));
    worker.icons(1.0);
    worker.flush();
    let keys = |cells: &[IconCell]| {
        cells
            .iter()
            .map(|cell| (cell.icon, cell.size))
            .collect::<Vec<_>>()
    };
    let previous: BTreeSet<_> = pages(&results)
        .iter()
        .flat_map(|(_, cells)| keys(cells))
        .collect();

    let (name, text) = SHIPPED[1];
    let regular = text.replace(r#""orb_scale": 0.95693785"#, r#""orb_scale": 0.75"#);
    assert_ne!(regular, text);
    let updated = Profiles::parse(
        &[SHIPPED[0], (name, &regular)],
        crate::test_data::data().card,
    )
    .unwrap();
    let expected: BTreeSet<_> = orb::atlas_cells(&updated)
        .map(|cell| (cell.icon, cell.size))
        .filter(|key| !previous.contains(key))
        .collect();
    assert!(!expected.is_empty());

    worker.layers(1.0, orb::atlas_artwork(&updated));
    worker.flush();
    let delivered: BTreeSet<_> = pages(&results)
        .iter()
        .flat_map(|(_, cells)| keys(cells))
        .collect();
    assert_eq!(delivered, expected);

    worker.layers(1.0, orb::atlas_artwork(&profiles));
    worker.flush();
    assert!(pages(&results).is_empty());
}

/// What the worker sent for the glyphs and icons a host asks for before its first frame, at
/// `scale`, followed by a mana symbol and the letters of "flip" asked for as laid-out text
/// asks: each page sent, its glyph cells and its icon cells.
fn prepared_then_demanded(scale: f32) -> (Vec<u16>, Vec<GlyphCell>, usize, Vec<GlyphKey>) {
    let (out, results) = mpsc::channel();
    let worker = spawn_glyph_worker(out, || {}, Some(&profiles()));
    worker.icons(scale);
    worker.prepare(prepare_latin(scale));
    let size = (32.0 * scale) as u16;
    let mana = FontRef::from_index(Font::Symbols.file().data, 0).expect("the Mana font parses");
    let mut demanded = vec![GlyphKey {
        blur: 0,
        font: Font::Symbols as u8,
        weight: 400,
        glyph: mana.charmap().map('\u{e601}'),
        size,
    }];
    demanded.extend("flip".chars().map(|character| open_sans(character, size)));
    worker.request(demanded.clone());
    worker.flush();
    let (mut pages, mut glyphs, mut icons) = (Vec::new(), Vec::new(), 0);
    for prepared in results.try_iter() {
        let Upload::AtlasPage { page, .. } = prepared.upload else {
            panic!("the glyph worker sent a face");
        };
        pages.push(page);
        match prepared.ready {
            AssetReady::Glyphs { glyphs: cells, .. } => glyphs.extend(cells),
            AssetReady::Icons { cells, .. } => icons += cells.len(),
            other @ AssetReady::Face { .. } => panic!("a glyph worker sent {other:?}"),
        }
    }
    (pages, glyphs, icons, demanded)
}

/// On a Retina display (scale 2) and a phone (scale 3), the Latin glyphs a host asks for before
/// its first frame filled more pages than the atlas holds. The renderer refused the pages past
/// its last, so every icon and the glyphs laid-out text asked for afterwards, such as mana
/// symbols and letters of "You won the flip", never drew. The test asks as hosts do, icons
/// first: the worker takes whatever requests are waiting as one batch, so a request's place in
/// the atlas must not depend on how the messages were split into batches.
#[test]
fn every_icon_and_demanded_glyph_fits_the_atlas_at_every_display_scale() {
    for scale in [1.0, 2.0, 3.0] {
        let (pages, glyphs, icons, demanded) = prepared_then_demanded(scale);
        assert!(
            pages.iter().all(|&page| page < ATLAS_PAGES),
            "at scale {scale} the worker sent pages {pages:?}, past the atlas's {ATLAS_PAGES}"
        );
        // The icons, a cell for each layer of the life orbs' art, and the cells icons' drop
        // shadows are drawn from.
        assert_eq!(
            icons,
            ALL.len() + art::atlas_cells(&profiles()).count() + icon_shadows::cells().count(),
            "every icon at scale {scale}"
        );
        for key in demanded {
            assert!(
                glyphs.iter().any(|cell| cell.key == key),
                "{key:?} at scale {scale}"
            );
        }
    }
}

#[test]
fn every_stoppable_step_icon_has_more_filled_area_when_on_than_off() {
    // Untap (0) and cleanup (11) are never stops, so their filled forms are unused.
    for scale in [1.0, 2.0, 3.0] {
        let pages = rasterize(scale);
        let cells: Vec<(&[u8], &IconCell)> = pages
            .iter()
            .flat_map(|(pixels, cells)| cells.iter().map(move |cell| (pixels.as_slice(), cell)))
            .collect();
        let cell_texels = |icon: IconId| -> Vec<u8> {
            let (pixels, cell) = cells
                .iter()
                .find(|(_, cell)| cell.shadow == 0 && cell.icon == icon)
                .copied()
                .unwrap_or_else(|| panic!("{icon:?} at scale {scale}"));
            texels(pixels, cell)
        };
        for step in 1..=10 {
            let on_texels = cell_texels(STEPS[step]);
            let off_texels = cell_texels(STEPS_OUTLINED[step]);
            let on: u64 = on_texels.iter().copied().map(u64::from).sum();
            let off: u64 = off_texels.iter().copied().map(u64::from).sum();
            let newly_filled = on_texels
                .iter()
                .zip(&off_texels)
                .filter(|&(&a, &b)| a >= 128 && b < 128)
                .count();
            assert!(
                on > off && newly_filled * 12 >= on_texels.len(),
                "step {step} at scale {scale}: filled ink ({on}) vs outlined ink ({off}), newly_filled {newly_filled}/{}",
                on_texels.len()
            );
        }
    }
}
