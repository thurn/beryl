//! Glyphs and card faces for the hosts that draw frames as they come: the desktop, windowless
//! and phone hosts (docs/beryl-3d.md, "Text", "Card faces" and "Threads").
//!
//! Text: the client lays text out without waiting for its glyphs and lists the glyphs no atlas
//! page holds (`Client::wanted_glyphs`). `Glyphs` hands them to `beryl_assets`' glyph worker,
//! which rasterizes them on its own thread, and uploads each page the worker finishes before
//! the next frame, telling the client where its glyphs landed (`Input::Assets`). The worker
//! wakes the host when a page is ready, so a still table draws the new text at once.
//!
//! Card faces (`Faces`): whenever the client's active face list changes (a match's
//! `Input::Cards`, or the component gallery opening or closing), the renderer's face arrays are
//! made for it and `beryl_assets`' workers prepare every face of it at board size. Each face
//! they finish is held by the renderer (`Renderer::hold`) and uploaded as a frame allows
//! (`Renderer::upload_waiting`), and the client is told where it landed. Faces a card being
//! read needs at full size (`Client::wanted_faces`) go to the same workers, and so do the scans
//! downloaded late, which replace a drawn frame only while the table is still.
//!
//! The headless host does the same, but waits for the workers before each picture
//! (`Headless`), so that a picture never lacks a glyph or a face.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver};

use beryl_art::{ArtCache, Origins};
use beryl_assets::glyphs::{GlyphWorker, prepare_latin};
use beryl_assets::{Config, Format, Prepared, Preparer, default_workers};
use beryl_render::Renderer;
use beryl_scene::Client;
use beryl_scene::frame::Wake;
use beryl_scene::input::{AssetReady, FaceSize, Input};

use crate::frames::When;

/// The app's identifier, which names its cache directory.
const APP: &str = "ca.thurn.beryl";

/// Where card pictures are read from: the scan cache that `beryl_art` keeps, and the cache of
/// compressed faces beside it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Scans {
    pub art: PathBuf,
    pub faces: PathBuf,
}

impl Scans {
    /// The app's own caches, in the directory Tauri gives the app: `~/Library/Caches/<app>` on
    /// macOS, `%LOCALAPPDATA%\<app>` on Windows and `$XDG_CACHE_HOME/<app>` (or `~/.cache`)
    /// elsewhere. `BERYL3D_SCANS` names another directory to use in their place, such as an
    /// empty one, to see every face drawn as Beryl's frame.
    #[must_use]
    pub fn app() -> Self {
        let var = |name: &str| std::env::var_os(name).map(PathBuf::from);
        if let Some(dir) = var("BERYL3D_SCANS") {
            return Self::in_dir(&dir);
        }
        let home = || var("HOME").unwrap_or_default();
        let base = if cfg!(windows) {
            var("LOCALAPPDATA").unwrap_or_default()
        } else if cfg!(target_os = "macos") {
            home().join("Library/Caches")
        } else {
            var("XDG_CACHE_HOME").unwrap_or_else(|| home().join(".cache"))
        };
        Self::in_dir(&base.join(APP))
    }

    /// The caches `art` and `faces` in `dir`.
    #[must_use]
    pub fn in_dir(dir: &std::path::Path) -> Self {
        Self {
            art: dir.join("art"),
            faces: dir.join("faces"),
        }
    }
}

/// The glyph worker of a host, and the pages it has finished.
pub(crate) struct Glyphs {
    worker: GlyphWorker,
    pages: Receiver<Prepared>,
    /// The physical pixels to a point that the Latin glyphs and the icons were asked for at.
    scale: f32,
}

impl Glyphs {
    /// Starts the worker and asks it for the Latin glyphs and every icon at `scale` physical
    /// pixels to a point, before the first frame needs them. The worker calls `wake` after it
    /// sends each page.
    pub(crate) fn start(scale: f32, wake: impl Fn() + Send + 'static) -> Self {
        let (out, pages) = mpsc::channel();
        let worker = GlyphWorker::spawn(out, wake, crate::threads::asset_thread);
        worker.prepare(prepare_latin(scale));
        worker.icons(scale);
        Self {
            worker,
            pages,
            scale,
        }
    }

    /// Asks again for the Latin glyphs and the icons when the display's scale has changed, as
    /// when the window moves to a display of another density.
    pub(crate) fn rescale(&mut self, scale: f32) {
        if (scale - self.scale).abs() > f32::EPSILON {
            self.scale = scale;
            self.worker.prepare(prepare_latin(scale));
            self.worker.icons(scale);
        }
    }

    /// Asks the worker for the glyphs the client's last frame lacked, and uploads every page it
    /// has finished, giving the client each page's glyphs or icons. It never waits.
    pub(crate) fn deliver(&mut self, client: &mut Client, renderer: &mut Renderer) {
        self.worker.request(client.wanted_glyphs());
        while let Ok(prepared) = self.pages.try_recv() {
            if renderer.upload_page(&prepared) {
                client.handle(Input::Assets(prepared.ready));
            }
        }
    }
}

/// The asset workers that prepare the client's card faces, and the face list the renderer's
/// arrays were last made for.
pub struct Faces {
    preparer: Preparer,
    /// Calls the host's wake, so that held faces get frames to be uploaded in.
    wake: Arc<dyn Fn() + Send + Sync>,
    /// Whether the active list the arrays were made for is the gallery's, and its number
    /// (`beryl_scene::faces::Faces::number`).
    active: Option<(bool, u64)>,
    /// Where the workers send the faces of the active list.
    results: Option<Receiver<Prepared>>,
    /// The client's wake right after its last frame: whether that frame was animating. Read
    /// then, because every input the client takes afterwards, the nudge that asks for the next
    /// frame among them, sets its wake to `Now` until that frame is made.
    after_frame: Wake,
}

impl Faces {
    /// Starts the workers, which read scans from `scans` and download the ones it lacks from
    /// `origins` (`beryl_art::SCRYFALL` in the app). A worker calls `wake` after it sends a
    /// face, and the host calls it again after each frame while faces wait to be uploaded.
    pub fn start(scans: &Scans, origins: Origins, wake: impl Fn() + Send + Sync + 'static) -> Self {
        let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(wake);
        let preparer = Preparer::new(Config {
            art: Arc::new(ArtCache::new(scans.art.clone(), origins)),
            faces_dir: scans.faces.clone(),
            workers: default_workers(),
            wake: Arc::clone(&wake),
            start: crate::threads::asset_thread,
        });
        Self {
            preparer,
            wake,
            active: None,
            results: None,
            after_frame: Wake::Never,
        }
    }

    /// Before a frame: makes the renderer's face arrays when the client's active face list has
    /// changed, hands the workers the faces wanted at full size, holds every face they have
    /// finished and uploads what the frame allows, telling the client where each landed. After
    /// a frame it only asks: uploads are limited per frame, so they are made once. While faces
    /// are held it wakes the host, so that a still table keeps getting frames to upload them in.
    /// It never waits.
    pub fn deliver(&mut self, client: &mut Client, renderer: &mut Renderer, when: When) {
        let gallery = client.showing_gallery();
        let active = (gallery, client.face_layers().number());
        // A match's list is empty until its card records arrive; the arrays wait for them.
        if self.active != Some(active) && !client.faces().faces().is_empty() {
            self.switch(client, renderer, active);
        }
        if self.active.is_none() {
            return;
        }
        // The full-size faces a match asks for are its own, so the gallery leaves them asked
        // for until the match's list is active again.
        if !gallery {
            let wanted: Vec<_> = client.wanted_faces().collect();
            for face in wanted {
                self.preparer.full(face);
            }
        }
        if let Some(results) = &self.results {
            for prepared in results.try_iter() {
                renderer.hold(prepared);
            }
        }
        if when == When::AfterFrame {
            self.after_frame = client.wake();
        } else {
            for ready in renderer.upload_waiting(self.after_frame) {
                client.handle(Input::Assets(for_list(ready, gallery)));
            }
        }
        if when == When::AfterFrame && renderer.waiting() > 0 {
            (self.wake)();
        }
    }

    /// Makes the renderer's arrays for the client's active list and queues its every face.
    fn switch(&mut self, client: &mut Client, renderer: &mut Renderer, active: (bool, u64)) {
        let list = client.faces();
        let format = Format::choose(&renderer.formats());
        let names: Vec<&str> = (list.faces().iter())
            .map(|face| face.names.first().map_or("", String::as_str))
            .collect();
        renderer.set_faces(&names, format);
        self.results = Some(self.preparer.prepare_match(list, &[format]));
        if !active.0 {
            client.face_arrays_made();
        }
        self.active = Some(active);
    }
}

/// A face's `AssetReady` as the client's active list knows it: the gallery's board-size faces
/// are its own, so that they never overwrite where the match's faces are.
fn for_list(ready: AssetReady, gallery: bool) -> AssetReady {
    match ready {
        AssetReady::Face {
            face,
            size: FaceSize::Board,
            layer,
        } if gallery => AssetReady::GalleryFace { face, layer },
        ready => ready,
    }
}
