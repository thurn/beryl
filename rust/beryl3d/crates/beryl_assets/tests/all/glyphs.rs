//! The glyph rasterizer, through its worker: what a host sends and what comes back.

use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use beryl_assets::glyphs::{
    FIRST_PAGE, GlyphWorker, glyphs_rasterized_on_this_thread, prepare_latin,
};
use beryl_assets::packer::{ATLAS_PAGES, PAGE_SIZE};
use beryl_assets::{Prepared, Upload};
use beryl_scene::input::{AssetReady, GlyphCell, GlyphKey};
use beryl_scene::text::fonts::Font;
use beryl_scene::widgets::icon_shadows;
use swash::FontRef;

/// Only catches a hung worker; a page normally arrives in milliseconds.
const HANG: Duration = Duration::from_mins(1);

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

/// The next page the worker sends, with its pixels and its glyphs.
fn next(results: &Receiver<Prepared>) -> (Vec<u8>, Vec<GlyphCell>) {
    let prepared = results.recv_timeout(HANG).expect("the worker sends a page");
    let Upload::AtlasPage { page, pixels } = prepared.upload else {
        panic!("the glyph worker sent a face");
    };
    let AssetReady::Glyphs {
        page: ready_page,
        glyphs,
    } = prepared.ready
    else {
        panic!("a glyph worker sends glyphs");
    };
    assert_eq!(page, ready_page);
    assert_eq!(
        pixels.len(),
        usize::from(PAGE_SIZE) * usize::from(PAGE_SIZE)
    );
    (pixels, glyphs)
}

/// The coverage of texel `(x, y)` of `cell`'s rectangle.
fn coverage(pixels: &[u8], cell: &GlyphCell, x: u16, y: u16) -> u8 {
    let [left, top, ..] = cell.rect;
    pixels[usize::from(top + y) * usize::from(PAGE_SIZE) + usize::from(left + x)]
}

#[test]
fn a_glyph_asked_for_twice_is_rasterized_once() {
    let (out, results) = mpsc::channel();
    let worker = GlyphWorker::spawn(out, || {}, || {});
    let p = open_sans('P', 28);
    let q = open_sans('Q', 28);
    worker.request([p, p]);
    let (_, first) = next(&results);
    assert_eq!(first.iter().map(|cell| cell.key).collect::<Vec<_>>(), [p]);
    worker.request([p, q]);
    let (_, second) = next(&results);
    assert_eq!(second.iter().map(|cell| cell.key).collect::<Vec<_>>(), [q]);
    worker.request([p]);
    drop(worker);
    assert!(
        results.try_recv().is_err(),
        "nothing new, so no page is sent"
    );
}

#[test]
fn the_capital_p_is_inked_in_its_stem_and_empty_in_its_bowl() {
    let (out, results) = mpsc::channel();
    let worker = GlyphWorker::spawn(out, || {}, || {});
    worker.request([open_sans('P', 28)]);
    let (pixels, cells) = next(&results);
    let [cell] = cells[..] else {
        panic!("one glyph was asked for");
    };
    let [.., width, height] = cell.rect;
    assert!(width > 8 && height > 16, "a 28-pixel capital: {cell:?}");
    assert!(cell.bearing[1] < 0, "a capital rises above the baseline");
    // The stem runs down the left side, below the bowl; the bowl's counter is the empty
    // middle of the upper half.
    let stem = coverage(&pixels, &cell, width * 3 / 20, height * 3 / 4);
    let bowl = coverage(&pixels, &cell, width / 2, height / 4);
    assert!(stem > 200, "stem coverage {stem}");
    assert_eq!(bowl, 0, "bowl coverage");
}

#[test]
fn glyphs_that_overflow_a_page_go_on_to_the_next() {
    let (out, results) = mpsc::channel();
    let worker = GlyphWorker::spawn(out, || {}, || {});
    let capitals: Vec<GlyphKey> = ('A'..='Z').map(|c| open_sans(c, 400)).collect();
    worker.request(capitals.clone());
    drop(worker);
    let mut sent = Vec::new();
    let mut pages = Vec::new();
    for prepared in &results {
        let AssetReady::Glyphs { page, glyphs } = prepared.ready else {
            panic!("a glyph worker sends glyphs");
        };
        pages.push(page);
        sent.extend(glyphs.iter().map(|cell| cell.key));
    }
    assert!(
        pages.len() > 1,
        "26 capitals at 400 pixels fill a page: {pages:?}"
    );
    // Page 0 holds the shapes of light, so glyphs start on the page after it.
    let numbers = FIRST_PAGE..FIRST_PAGE + pages.len() as u16;
    assert_eq!(pages, numbers.collect::<Vec<_>>());
    assert_eq!(
        sent, capitals,
        "each glyph is sent once, in the order asked"
    );
}

#[test]
fn latin_glyphs_cover_every_type_style() {
    let keys = prepare_latin(2.0);
    let printable = usize::from(b'~' - b' ') + 1;
    assert_eq!(
        keys.len(),
        beryl_scene::tokens::type_style::ALL.len() * printable,
        "each bundled text font has all of ASCII, at every type style's size"
    );
    let title = beryl_scene::tokens::type_style::TITLE;
    assert!(
        keys.iter()
            .any(|key| key.font == title.font.face(title.weight)
                && key.weight == title.weight
                && key.size == 52)
    );
}

#[test]
fn a_space_is_sent_with_an_empty_rectangle() {
    let (out, results) = mpsc::channel();
    let worker = GlyphWorker::spawn(out, || {}, || {});
    let space = open_sans(' ', 28);
    worker.request([space]);
    let (_, cells) = next(&results);
    assert_eq!(cells.len(), 1);
    assert_eq!(cells[0].key, space);
    assert_eq!(cells[0].rect[2..], [0, 0]);
}

#[test]
fn a_flush_returns_once_every_page_asked_for_is_sent() {
    let (out, results) = mpsc::channel();
    let worker = GlyphWorker::spawn(out, || {}, || {});
    worker.request(prepare_latin(1.0));
    worker.flush();
    let pages: Vec<Prepared> = results.try_iter().collect();
    assert!(
        !pages.is_empty(),
        "no page had arrived when the flush returned"
    );
    // Asking again for glyphs already sent sends nothing, and the flush still returns.
    worker.request(prepare_latin(1.0));
    worker.flush();
    assert_eq!(results.try_iter().count(), 0);
}

#[test]
fn a_rescale_batch_is_rasterized_off_the_thread_that_asks() {
    let (out, results) = mpsc::channel();
    let worker = GlyphWorker::spawn(out, || {}, || {});
    worker.request(prepare_latin(1.0));
    // The display's scale changes: every glyph is asked for again at the new size. Asking only
    // sends the keys to the worker, so the thread that draws frames rasterizes nothing.
    worker.request(prepare_latin(2.0));
    assert_eq!(glyphs_rasterized_on_this_thread(), 0);
    worker.flush();
    assert_eq!(glyphs_rasterized_on_this_thread(), 0);
    assert!(results.try_iter().count() > 0);
}

#[test]
fn a_mana_symbol_is_inked() {
    let (out, results) = mpsc::channel();
    let worker = GlyphWorker::spawn(out, || {}, || {});
    let font = FontRef::from_index(Font::Mana.file().data, 0).expect("the Mana font parses");
    let glyph = font.charmap().map('\u{e601}');
    assert_ne!(glyph, 0, "the Mana font has the blue symbol");
    let key = GlyphKey {
        blur: 0,
        font: Font::Mana as u8,
        weight: 400,
        glyph,
        size: 32,
    };
    worker.request([key]);
    let (_, cells) = next(&results);
    assert_eq!(cells[0].key, key);
    assert!(
        cells[0].rect[2] > 0 && cells[0].rect[3] > 0,
        "{:?}",
        cells[0]
    );
}

/// What the worker sent for the glyphs and icons a host asks for before its first frame, at
/// `scale`, followed by a mana symbol and the letters of "flip" asked for as laid-out text
/// asks: each page sent, its glyph cells and its icon cells.
fn prepared_then_demanded(scale: f32) -> (Vec<u16>, Vec<GlyphCell>, usize, Vec<GlyphKey>) {
    let (out, results) = mpsc::channel();
    let worker = GlyphWorker::spawn(out, || {}, || {});
    worker.prepare(prepare_latin(scale));
    worker.icons(scale);
    let size = (32.0 * scale) as u16;
    let mana = FontRef::from_index(Font::Mana.file().data, 0).expect("the Mana font parses");
    let mut demanded = vec![GlyphKey {
        blur: 0,
        font: Font::Mana as u8,
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
            other => panic!("a glyph worker sent {other:?}"),
        }
    }
    (pages, glyphs, icons, demanded)
}

/// On a Retina display (scale 2) and a phone (scale 3), the Latin glyphs a host asks for before
/// its first frame filled more pages than the atlas holds. The renderer refused the pages past
/// its last, so every icon and the glyphs laid-out text asked for afterwards, such as mana
/// symbols and letters of "You won the flip", never drew.
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
            beryl_assets::icons::ALL.len()
                + beryl_assets::orb::cells().count()
                + icon_shadows::cells().count(),
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
