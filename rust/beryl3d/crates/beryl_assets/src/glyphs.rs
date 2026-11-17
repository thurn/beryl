//! The glyph rasterizer (docs/beryl-3d.md, "Text": glyphs are drawn from an atlas; "Hosts":
//! `AssetReady::Glyphs`). A [`GlyphWorker`] owns a thread that rasterizes each glyph the host
//! asks for with `swash`, once, into an 8-bit coverage bitmap, packs it into a single-channel
//! atlas page and sends each new or changed page as a [`Prepared`]. The caller's thread only
//! sends keys, so the frame thread never rasterizes. The same thread rasterizes the flat
//! layer's icons into the same pages ([`GlyphWorker::icons`], [`crate::icons`]).

use std::cell::Cell;
use std::collections::BTreeSet;
use std::sync::mpsc::{self, Receiver, Sender, SyncSender};
use std::thread::{self, JoinHandle};

use beryl_scene::input::{AssetReady, GlyphCell, GlyphKey};
use beryl_scene::text::fonts::ALL;
use beryl_scene::text::glyph_key;
use beryl_scene::tokens::type_style;
use swash::FontRef;
use swash::scale::{Render, ScaleContext, Source};
use swash::zeno::Format;

use crate::icons;
use crate::packer::{ATLAS_PAGES, Full, PAGE_SIZE, Shelves};
use crate::{Prepared, Upload};

/// The atlas page the worker fills first. Page 0 holds the shapes of light ([`crate::light`]).
pub const FIRST_PAGE: u16 = 1;

/// The atlas's last page, past which the worker never fills one: the renderer refuses a page
/// past it, and the client would never learn of the glyphs on it.
const LAST_PAGE: u16 = ATLAS_PAGES - 1;

/// How many of the atlas's last pages glyphs asked for ahead of time ([`GlyphWorker::prepare`])
/// leave free for the icons and for glyphs laid-out text asks for. At three pixels to a point a
/// page holds about two hundred glyphs of a body style, so three hold every glyph of a game's
/// text with room to spare.
const DEMAND_PAGES: u16 = 3;

/// The keys of the printable ASCII characters, space to tilde, of every type style in
/// `beryl_scene::tokens` at `scale` physical pixels per point, for a host to hand to
/// [`GlyphWorker::prepare`] before its first frame. A character a font lacks is left out.
#[must_use]
pub fn prepare_latin(scale: f32) -> Vec<GlyphKey> {
    let mut keys = Vec::new();
    for style in type_style::ALL {
        let Some(font) = FontRef::from_index(style.font.file().data, 0) else {
            continue;
        };
        let charmap = font.charmap();
        for character in ' '..='~' {
            let glyph = charmap.map(character);
            if glyph != 0 {
                keys.push(glyph_key(style, scale, glyph));
            }
        }
    }
    keys
}

thread_local! {
    static GLYPHS_RASTERIZED: Cell<u64> = const { Cell::new(0) };
}

/// How many glyphs this thread has rasterized. Only the glyph worker rasterizes, so on any
/// other thread, such as the one that draws frames, this stays zero.
#[must_use]
pub fn glyphs_rasterized_on_this_thread() -> u64 {
    GLYPHS_RASTERIZED.with(Cell::get)
}

/// A thread that rasterizes glyphs. Dropping it stops the thread once it has finished what it
/// was asked.
#[derive(Debug)]
pub struct GlyphWorker {
    requests: Option<Sender<Message>>,
    thread: Option<JoinHandle<()>>,
}

impl GlyphWorker {
    /// Starts the thread, which first calls `start` to set its scheduling class (`beryl_host`'s
    /// `asset_thread`), since it would otherwise inherit its maker's. It sends each finished
    /// page to `out` and then calls `wake`, so that a host whose event loop is idle draws the
    /// new glyphs at once.
    #[must_use]
    pub fn spawn(out: Sender<Prepared>, wake: impl Fn() + Send + 'static, start: fn()) -> Self {
        let (requests, inbox) = mpsc::channel();
        let thread = thread::Builder::new()
            .name("beryl-glyphs".into())
            .spawn(move || {
                start();
                run(&inbox, &out, &wake);
            })
            .expect("the glyph worker's thread starts");
        Self {
            requests: Some(requests),
            thread: Some(thread),
        }
    }

    /// Asks for glyphs. A glyph asked for before, in this call or an earlier one, is not
    /// rasterized or sent again.
    pub fn request(&self, keys: impl IntoIterator<Item = GlyphKey>) {
        let keys: Vec<GlyphKey> = keys.into_iter().collect();
        if keys.is_empty() {
            return;
        }
        if let Some(requests) = &self.requests {
            // The worker only stops when this sender is dropped, or when the host has dropped
            // the receiver of finished pages and wants none.
            let _ = requests.send(Message::Keys(keys));
        }
    }

    /// Asks for glyphs ahead of time, so that text drawn in the first frames has them. Unlike
    /// [`GlyphWorker::request`], it fills only the pages that leave the atlas's last
    /// `DEMAND_PAGES` free, after the icons and glyphs asked for with it: at two pixels to a
    /// point and more, the Latin glyphs of every type style would fill more pages than the atlas
    /// holds. A glyph left out is rasterized when it is requested.
    pub fn prepare(&self, keys: impl IntoIterator<Item = GlyphKey>) {
        let keys: Vec<GlyphKey> = keys.into_iter().collect();
        if keys.is_empty() {
            return;
        }
        if let Some(requests) = &self.requests {
            // As in `request`: the worker stops only when nobody wants its pages.
            let _ = requests.send(Message::Prepare(keys));
        }
    }

    /// Asks for every icon of `beryl_scene::widgets::icons` rasterized at `scale` physical
    /// pixels per point, which a host does before its first frame and again whenever the
    /// display's scale changes. The cells come in an `AssetReady::Icons` that replaces the
    /// client's cells of the old scale, which it draws scaled until then. Asking for the scale
    /// asked for last does nothing.
    pub fn icons(&self, scale: f32) {
        if let Some(requests) = &self.requests {
            // As in `request`: the worker stops only when nobody wants its pages.
            let _ = requests.send(Message::Icons(scale));
        }
    }

    /// Waits until every glyph and icon asked for so far has been rasterized and its page sent, so
    /// that a host drawing a picture of one moment, such as the headless host, can take every
    /// page it needs before it draws. A host drawing frames as they come never calls it.
    pub fn flush(&self) {
        let Some(requests) = &self.requests else {
            return;
        };
        let (done, wait) = mpsc::sync_channel(1);
        if requests.send(Message::Flush(done)).is_ok() {
            // An error means the worker stopped, and has nothing left to send.
            let _ = wait.recv();
        }
    }
}

/// What a host sends the worker.
#[derive(Debug)]
enum Message {
    /// Glyphs to rasterize.
    Keys(Vec<GlyphKey>),
    /// Glyphs to rasterize ahead of time, while pages short of the last `DEMAND_PAGES` have room.
    Prepare(Vec<GlyphKey>),
    /// Every icon, at this scale.
    Icons(f32),
    /// Answer once every glyph asked for before this has been sent.
    Flush(SyncSender<()>),
}

impl Drop for GlyphWorker {
    fn drop(&mut self) {
        self.requests = None;
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// The worker's loop: takes every request waiting, rasterizes them together so that a burst
/// of requests uploads each page once, sends the pages, and then answers the flushes that were
/// waiting with them. The icons come first and glyphs asked for ahead of time last, so that
/// what is on screen has room in the atlas before what may be.
fn run(inbox: &Receiver<Message>, out: &Sender<Prepared>, wake: &dyn Fn()) {
    let mut atlas = Atlas::new();
    while let Ok(first) = inbox.recv() {
        let mut keys = Vec::new();
        let mut ahead = Vec::new();
        let mut scale = None;
        let mut flushes = Vec::new();
        for message in std::iter::once(first).chain(inbox.try_iter()) {
            match message {
                Message::Keys(more) => keys.extend(more),
                Message::Prepare(more) => ahead.extend(more),
                Message::Icons(asked) => scale = Some(asked),
                Message::Flush(done) => flushes.push(done),
            }
        }
        let mut prepared = Vec::new();
        if let Some(scale) = scale {
            prepared.extend(atlas.add_icons(scale));
        }
        prepared.extend(atlas.add(&keys, LAST_PAGE));
        prepared.extend(atlas.add(&ahead, LAST_PAGE - DEMAND_PAGES));
        let sent = !prepared.is_empty();
        for page in prepared {
            if out.send(page).is_err() {
                return;
            }
        }
        if sent {
            wake();
        }
        for done in flushes {
            let _ = done.send(());
        }
    }
}

/// The atlas's pages: every glyph rasterized so far and the page being filled.
struct Atlas {
    context: ScaleContext,
    done: BTreeSet<GlyphKey>,
    /// The number of the page being filled.
    page: u16,
    pixels: Vec<u8>,
    shelves: Shelves,
    /// The scale the icons were last rasterized at.
    icon_scale: Option<f32>,
}

impl Atlas {
    fn new() -> Self {
        Self {
            context: ScaleContext::new(),
            done: BTreeSet::new(),
            page: FIRST_PAGE,
            pixels: blank_page(),
            shelves: Shelves::new(PAGE_SIZE),
            icon_scale: None,
        }
    }

    /// Rasterizes each glyph of `keys` not rasterized before, on pages up to `last`, and returns
    /// the pages that changed, each with the glyphs added to it. A page that fills up is sent
    /// and the next one started. A glyph whose outline is empty, such as a space, is sent with
    /// an empty rectangle. A glyph too large for a page, or of a font that does not exist, is
    /// never sent, and draws nothing. Once page `last` is full the rest of `keys` is left out,
    /// to be rasterized if asked for again with a later `last`.
    fn add(&mut self, keys: &[GlyphKey], last: u16) -> Vec<Prepared> {
        let mut prepared = Vec::new();
        if self.page > last {
            return prepared;
        }
        let mut cells = Vec::new();
        for (index, &key) in keys.iter().enumerate() {
            if self.done.contains(&key) {
                continue;
            }
            let Some(bitmap) = self.rasterize(key) else {
                self.done.insert(key);
                continue;
            };
            let (width, height) = (bitmap.width, bitmap.height);
            let placed = match self.shelves.insert(width, height) {
                Err(Full) if self.page < last => {
                    let glyphs = std::mem::take(&mut cells);
                    prepared.push(self.finish_page(|page| AssetReady::Glyphs { page, glyphs }));
                    self.shelves.insert(width, height)
                }
                Err(Full) => {
                    if last == LAST_PAGE {
                        eprintln!(
                            "beryl_assets: the atlas's {ATLAS_PAGES} pages are full, so {} glyphs \
                             asked for draw nothing",
                            keys.len() - index
                        );
                    }
                    break;
                }
                placed => placed,
            };
            self.done.insert(key);
            let Ok(corner) = placed else {
                continue;
            };
            self.blit(&bitmap, corner);
            cells.push(GlyphCell {
                key,
                rect: [corner[0], corner[1], bitmap.width, bitmap.height],
                bearing: bitmap.bearing,
            });
        }
        if !cells.is_empty() {
            let page = self.page;
            prepared.push(self.this_page(AssetReady::Glyphs {
                page,
                glyphs: cells,
            }));
        }
        prepared
    }

    /// Rasterizes every icon at `scale` into fresh space of the pages, leaving the cells of
    /// any earlier scale in place, since the client draws those until these arrive, and
    /// returns the pages that changed, each with the icons added to it.
    fn add_icons(&mut self, scale: f32) -> Vec<Prepared> {
        if self.icon_scale == Some(scale) {
            return Vec::new();
        }
        self.icon_scale = Some(scale);
        let mut prepared = Vec::new();
        let mut cells = Vec::new();
        for artwork in &icons::ALL {
            let bitmap = icons::rasterize(artwork, scale);
            let side = bitmap.width;
            let placed = self.shelves.insert(side, side).or_else(|Full| {
                if self.page == LAST_PAGE {
                    return Err(Full);
                }
                let cells = std::mem::take(&mut cells);
                prepared.push(self.finish_page(|page| AssetReady::Icons { page, cells }));
                self.shelves.insert(side, side)
            });
            // An icon too large for an empty page, or with no room left in the atlas, is never
            // sent, and draws nothing.
            let Ok(corner) = placed else {
                continue;
            };
            self.blit(&bitmap, corner);
            cells.push(icons::cell(artwork, [corner[0], corner[1], side, side]));
        }
        if !cells.is_empty() {
            let page = self.page;
            prepared.push(self.this_page(AssetReady::Icons { page, cells }));
        }
        prepared
    }

    /// The page being filled as it is now, with `ready` naming what was added to it.
    fn this_page(&self, ready: AssetReady) -> Prepared {
        Prepared {
            upload: Upload::AtlasPage {
                page: self.page,
                pixels: self.pixels.clone(),
            },
            ready,
        }
    }

    /// Hands over the full page with what `ready` makes of its number (the cells on it not yet
    /// sent), and starts the next.
    fn finish_page(&mut self, ready: impl FnOnce(u16) -> AssetReady) -> Prepared {
        let page = self.page;
        self.page += 1;
        self.shelves = Shelves::new(PAGE_SIZE);
        Prepared {
            upload: Upload::AtlasPage {
                page,
                pixels: std::mem::replace(&mut self.pixels, blank_page()),
            },
            ready: ready(page),
        }
    }

    /// Draws the glyph `key` as coverage, without hinting, with the variable font's weight
    /// axis set to the key's weight.
    fn rasterize(&mut self, key: GlyphKey) -> Option<Bitmap> {
        GLYPHS_RASTERIZED.with(|count| count.set(count.get() + 1));
        let file = ALL.get(usize::from(key.font))?;
        let font = FontRef::from_index(file.data, 0)?;
        let mut scaler = self
            .context
            .builder(font)
            .size(f32::from(key.size))
            .hint(false)
            .variations([("wght", f32::from(key.weight))])
            .build();
        let image = Render::new(&[Source::Outline])
            .format(Format::Alpha)
            .render(&mut scaler, key.glyph)?;
        let placement = image.placement;
        // The packer leaves a texel's gap beside every glyph, so a glyph as wide as a page has no room.
        let too_large = |length: u32| length >= u32::from(PAGE_SIZE);
        if too_large(placement.width) || too_large(placement.height) {
            return None;
        }
        Some(Bitmap {
            width: placement.width as u16,
            height: placement.height as u16,
            // swash measures the top edge up from the baseline; a cell's bearing is y down.
            bearing: [
                saturate(placement.left),
                saturate(placement.top.saturating_neg()),
            ],
            coverage: image.data,
        })
    }

    /// Copies `bitmap` into the page with its top left texel at `corner`.
    fn blit(&mut self, bitmap: &Bitmap, corner: [u16; 2]) {
        let width = usize::from(bitmap.width);
        if width == 0 {
            return;
        }
        let stride = usize::from(PAGE_SIZE);
        for (row, line) in bitmap.coverage.chunks_exact(width).enumerate() {
            let start = (usize::from(corner[1]) + row) * stride + usize::from(corner[0]);
            self.pixels[start..start + width].copy_from_slice(line);
        }
    }
}

/// One rasterized glyph or icon.
pub(crate) struct Bitmap {
    pub(crate) width: u16,
    pub(crate) height: u16,
    pub(crate) bearing: [i16; 2],
    /// `height` rows of `width` coverage bytes.
    pub(crate) coverage: Vec<u8>,
}

fn blank_page() -> Vec<u8> {
    vec![0; usize::from(PAGE_SIZE) * usize::from(PAGE_SIZE)]
}

fn saturate(value: i32) -> i16 {
    value.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16
}
