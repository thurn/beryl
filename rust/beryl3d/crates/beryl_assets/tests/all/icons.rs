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
    let icons: Vec<IconId> = cells.iter().map(|(_, cell)| cell.icon).collect();
    // Every icon, and then the orbs' layers at each scale they are drawn at.
    let orbs = beryl_assets::orb::cells().map(|cell| orb::icon(orb::SIDES[cell.seat], cell.at));
    assert_eq!(icons, (0..ORB).map(IconId).chain(orbs).collect::<Vec<_>>());
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
    let (AssetReady::Icons { cells: old, .. }, AssetReady::Icons { cells: new, .. }) =
        (first.ready, second.ready)
    else {
        panic!("only icons were asked for");
    };
    // The old cells stay where they were, since the client draws them until the new arrive.
    for (old, new) in old.iter().zip(&new) {
        assert_eq!(old.icon, new.icon);
        assert_eq!(new.rect[2], old.rect[2] * 2, "{:?}", new.icon);
        let apart =
            new.rect[1] >= old.rect[1] + old.rect[3] || new.rect[0] >= old.rect[0] + old.rect[2];
        assert!(apart, "{:?} was drawn over its old cell", new.icon);
    }
}
