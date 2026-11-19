//! The flat layer's icons, rasterized by the glyph worker: what a host asks for and what comes
//! back.

use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use beryl_assets::glyphs::GlyphWorker;
use beryl_assets::icons::{ALL, icons_rasterized_on_this_thread};
use beryl_assets::packer::PAGE_SIZE;
use beryl_assets::{Prepared, Upload};
use beryl_scene::art::orb;
use beryl_scene::input::{AssetReady, IconCell};
use beryl_scene::widgets::IconId;
use beryl_scene::widgets::icon_shadows;
use beryl_scene::widgets::icons::{COUNT, GEAR, ORB};

/// Only catches a hung worker; a page normally arrives in milliseconds.
const HANG: Duration = Duration::from_mins(1);

/// Every page the worker sends until it has nothing more, with its pixels and icons.
fn pages(results: &Receiver<Prepared>) -> Vec<(Vec<u8>, Vec<IconCell>)> {
    let mut pages = Vec::new();
    while let Ok(prepared) = results.recv_timeout(HANG) {
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

/// Every icon at `scale`, asked for alone.
fn rasterize(scale: f32) -> Vec<(Vec<u8>, Vec<IconCell>)> {
    let (out, results) = mpsc::channel();
    let worker = GlyphWorker::spawn(out, || {}, || {});
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
    // The ids from `ORB` on are the layers of the life orbs' art (`beryl_assets::orb`).
    assert_eq!(ALL.len(), usize::from(ORB));
    assert_eq!(COUNT, ORB + orb::ICONS);
    for (id, artwork) in (0..ORB).zip(&ALL) {
        assert_eq!(artwork.icon, IconId(id), "the artwork at index {id}");
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
    // Every icon, and the orbs' layers at each scale they are drawn at, packed tallest first.
    let orbs = beryl_assets::orb::cells().map(|cell| orb::icon(orb::SIDES[cell.seat], cell.at));
    let mut expected: Vec<IconId> = (0..ORB).map(IconId).chain(orbs).collect();
    expected.sort_unstable();
    assert_eq!(icons, expected);
    // And a cell of each shadow of each chain of drop shadows an icon is drawn with.
    let mut shadows: Vec<(IconId, u8)> = (cells.iter())
        .filter(|(_, cell)| cell.shadow > 0)
        .map(|(_, cell)| {
            let (chain, stage) = icon_shadows::decode(cell.shadow).expect("a listed shadow");
            let side = icon_shadows::stage_side(cell.size, icon_shadows::shadows(chain), stage, 2.0);
            assert_eq!(cell.rect[2..], [side, side], "{:?} {}", cell.icon, cell.shadow);
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
    }
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
    let worker = GlyphWorker::spawn(out, || {}, || {});
    worker.icons(1.0);
    worker.flush();
    let first = results.try_recv().expect("the icons at scale 1");
    worker.icons(1.0);
    worker.flush();
    assert!(
        results.try_recv().is_err(),
        "the same scale is not sent again"
    );
    worker.icons(2.0);
    worker.flush();
    let second = results.try_recv().expect("the icons at scale 2");
    let (
        AssetReady::Icons {
            cells: old,
            page: old_page,
        },
        AssetReady::Icons {
            cells: new,
            page: new_page,
        },
    ) = (first.ready, second.ready)
    else {
        panic!("only icons were asked for");
    };
    // The old cells stay where they were, since the client draws them until the new arrive.
    let sharp = |cell: &&IconCell| cell.shadow == 0 && cell.icon.0 < ORB;
    for new in new.iter().filter(sharp) {
        let Some(old) = old.iter().filter(sharp).find(|old| old.icon == new.icon) else {
            continue;
        };
        assert_eq!(new.rect[2], old.rect[2] * 2, "{:?}", new.icon);
        let apart = new_page != old_page
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

