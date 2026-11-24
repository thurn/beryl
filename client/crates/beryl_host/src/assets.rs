//! One asset service for every rendering host. It hands the client's glyph, icon and face
//! requests to the workers, then hands back the cells and layers the renderer uploaded.
//! Live hosts advance the service around each frame. Pictures wait for the same deliveries.

use kit_ui::input::Input as DeviceInput;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};

use app_client::Client;
use app_client::Input;
use beryl_art::{ArtCache, Origins};
use beryl_assets::glyphs::{GlyphWorker, IconSet, prepare_latin};
use beryl_assets::{
    Config, FULL_LAYERS, FaceText, Format, Full, MatchFace, MatchFaces, Plate, Prepared, Preparer,
    default_workers,
};
use beryl_render::Renderer;
use game_hud::art::{self, orb};
use game_table::faces::FaceList;
use kit_base::atlas::{AssetReady, FaceId, FaceSize, GlyphKey};
use kit_base::data::{
    Data,
    light::{Light, Own},
    profiles::Profiles,
};
use kit_frame::frame::Wake;
use kit_ui::art::icons as icon_artwork;
use kit_ui::widgets::icon_shadows;

use crate::frames::When;

/// Starts the glyph worker with the widget icons, their shadows, and the game's artwork.
#[must_use]
pub fn spawn_glyph_worker(
    out: Sender<Prepared>,
    wake: impl Fn() + Send + 'static,
    profiles: Option<&Profiles>,
) -> GlyphWorker {
    GlyphWorker::spawn(
        out,
        wake,
        crate::threads::asset_thread,
        IconSet {
            icons: icon_artwork::ALL.to_vec(),
            shadows: &icon_shadows::CHAINS,
            layers: profiles.map_or_else(Vec::new, art::atlas_artwork),
        },
    )
}

/// Converts a table's faces into worker inputs in the same [`FaceId`] order.
#[must_use]
pub fn match_faces(list: &FaceList) -> MatchFaces {
    let faces = list
        .faces()
        .iter()
        .map(|face| {
            // Plates have no lookup names of their own. Their fallback frame prints the
            // source card's name.
            let named = face
                .plate
                .as_ref()
                .and_then(|plate| list.get(plate.source))
                .unwrap_or(face);
            let text = &face.text;
            MatchFace::from_image(
                &face.image,
                named.names.first().cloned().unwrap_or_default(),
                face.colors.clone(),
                FaceText {
                    mana_cost: text.mana_cost.clone(),
                    type_line: text.type_line.clone(),
                    rules: text.rules.clone(),
                    power: text.power.clone(),
                    toughness: text.toughness.clone(),
                    loyalty: text.loyalty.clone(),
                    token: text.token,
                },
                face.plate.as_ref().map(|plate| Plate {
                    source: plate.source,
                    triggered: plate.triggered,
                    choice: plate.choice,
                    rules: plate.rules.clone(),
                }),
            )
        })
        .collect();
    MatchFaces { faces }
}

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
    /// elsewhere. `BERYL_CLIENT_SCANS` names another directory to use in their place, such as an
    /// empty one, to see every face drawn as Beryl's frame.
    #[must_use]
    pub fn app() -> Self {
        let var = |name: &str| std::env::var_os(name).map(PathBuf::from);
        if let Some(dir) = var("BERYL_CLIENT_SCANS") {
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

/// The workers and their deliveries to one renderer and client.
pub struct Assets {
    /// The tuning the atlas's shapes of light were last drawn from, so that an edit of it
    /// draws and uploads the page again.
    light: Option<(Light, Own)>,
    glyphs: GlyphWorker,
    pages: Receiver<Prepared>,
    scale: Option<f32>,
    tuning: Option<Arc<Data>>,
    faces: Preparer,
    results: Option<Receiver<Prepared>>,
    /// Whether the current list belongs to a story, and its generation.
    active: Option<(bool, u64)>,
    /// Owners of uploaded full-size layers. An assignment alone doesn't mean it was uploaded.
    full: [Option<FaceId>; FULL_LAYERS],
    /// The wake before a nudge or another input sets the client's wake to `Now`.
    after_frame: Wake,
    wake: Arc<dyn Fn() + Send + Sync>,
}

impl Assets {
    /// Starts the asset workers. The host's `wake` asks its event loop for a frame after a
    /// delivery or while face uploads are queued.
    pub fn start(scans: &Scans, origins: Origins, wake: impl Fn() + Send + Sync + 'static) -> Self {
        let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(wake);
        let (out, pages) = mpsc::channel();
        let glyphs = spawn_glyph_worker(
            out,
            {
                let wake = Arc::clone(&wake);
                move || wake()
            },
            None,
        );
        let faces = Preparer::new(Config {
            art: Arc::new(ArtCache::new(scans.art.clone(), origins)),
            faces_dir: scans.faces.clone(),
            workers: default_workers(),
            wake: Arc::clone(&wake),
            start: crate::threads::asset_thread,
        });
        Self {
            light: None,
            glyphs,
            pages,
            scale: None,
            tuning: None,
            faces,
            results: None,
            active: None,
            full: [None; FULL_LAYERS],
            after_frame: Wake::Never,
            wake,
        }
    }

    pub(crate) fn live(wake: impl Fn() + Send + Sync + 'static) -> Self {
        Self::start(&Scans::app(), beryl_art::SCRYFALL, wake)
    }

    /// Draws the shapes of light from `data`'s tuning and uploads them as atlas page 0, unless
    /// the page already holds that tuning. A host calls it before each frame, so the page is
    /// there for the first frame and follows an edit of `light.json` or `own.json`.
    pub fn sync_light(&mut self, renderer: &mut Renderer, data: &Data) {
        let tuning = (data.light, data.own);
        if self.light != Some(tuning) {
            renderer.upload_page(&beryl_assets::light::prepared(&data.light, &data.own));
            self.light = Some(tuning);
        }
    }

    /// Advances the service without waiting on a worker. In a live frame, card faces are
    /// uploaded only before the frame, at the rate the renderer allows.
    pub fn deliver(&mut self, client: &mut Client, renderer: &mut Renderer, when: When) {
        if when == When::AfterFrame {
            self.after_frame = client.wake();
        }
        self.sync_light(renderer, client.data());
        self.request_text(client);
        self.prepare_faces(client, renderer);
        if self.active == Some(client.face_generation()) {
            let wanted = client.wanted_faces().collect::<Vec<_>>();
            for face in wanted {
                self.request_full(client, face);
            }
        }
        self.receive_faces(renderer);
        if when == When::AfterFrame {
            if renderer.waiting() > 0 {
                (self.wake)();
            }
        } else {
            self.upload_text(client, renderer, |_| {});
            self.upload_faces(client, renderer, self.after_frame);
        }
    }

    pub(crate) fn low_memory(&mut self, renderer: &mut Renderer) {
        self.faces.forget_full();
        self.full.fill(None);
        // Keep board deliveries that were already sent and discard the old full-size ones.
        self.receive_faces(renderer);
        renderer.low_memory();
    }

    /// Waits for the picture's current requests before drawing, with the same workers and
    /// uploads as a live frame. The replay driver retains atlas deliveries across seeks.
    #[cfg(feature = "headless")]
    pub(crate) fn settle(
        &mut self,
        client: &mut Client,
        renderer: &mut Renderer,
        mut remember_atlas: impl FnMut(AssetReady),
    ) {
        use std::time::Duration;

        // Detect a hung worker or an encoder that never produced a result.
        const FACE_WAIT: Duration = Duration::from_mins(2);

        self.sync_light(renderer, client.data());
        self.request_text(client);
        self.glyphs.flush();
        self.upload_text(client, renderer, |ready| remember_atlas(ready.clone()));
        self.prepare_faces(client, renderer);
        if self.active != Some(client.face_generation()) {
            return;
        }
        // A backward seek creates a new client. Board layers keep their contents in the
        // renderer and can be announced again without decoding any pictures.
        let count = client.faces().faces().len();
        for index in 0..count {
            let layer = index as u16;
            let face = FaceId(layer);
            if client.face_layers().board_layer_of(face).is_none()
                && renderer.layer_uploaded(FaceSize::Board, layer)
            {
                client.handle(Input::Device(DeviceInput::Assets(AssetReady::Face {
                    face,
                    size: FaceSize::Board,
                    layer,
                })));
            }
        }
        // A picture can wait for only as many full-size faces as fit at once.
        let faces = client.wanted_faces().take(FULL_LAYERS).collect::<Vec<_>>();
        let wanted = faces
            .into_iter()
            .filter(|&face| self.request_full(client, face))
            .collect::<Vec<_>>();
        self.receive_faces(renderer);
        while renderer.uploaded(FaceSize::Board) < count
            || wanted
                .iter()
                .any(|&face| client.face_layers().full_layer_of(face).is_none())
            || renderer.waiting() > 0
        {
            if renderer.waiting() == 0 {
                let Some(prepared) = self
                    .results
                    .as_ref()
                    .and_then(|faces| faces.recv_timeout(FACE_WAIT).ok())
                else {
                    break;
                };
                renderer.hold(prepared);
                self.receive_faces(renderer);
            }
            self.upload_faces(client, renderer, Wake::Never);
        }
    }

    /// Waits for a toolkit scene's glyphs and icons, then delivers the assets uploaded
    /// to the renderer. The scene supplies its scale and glyph requests directly.
    #[cfg(feature = "headless")]
    pub fn settle_glyphs(
        &mut self,
        renderer: &mut Renderer,
        data: &Arc<Data>,
        scale: f32,
        wanted: impl IntoIterator<Item = GlyphKey>,
        deliver: impl FnMut(AssetReady),
    ) {
        self.sync_light(renderer, data);
        self.request_artwork(data, scale);
        self.request_glyphs(scale, wanted);
        self.glyphs.flush();
        self.upload_glyphs(renderer, deliver);
    }

    fn request_text(&mut self, client: &mut Client) {
        let scale = client.surface_scale();
        self.request_artwork(client.data(), scale);
        self.request_glyphs(scale, client.wanted_glyphs());
    }

    fn request_artwork(&mut self, data: &Arc<Data>, scale: f32) {
        if self
            .tuning
            .as_ref()
            .is_none_or(|old| !Arc::ptr_eq(old, data))
        {
            if self.tuning.as_ref().is_none_or(|old| {
                !orb::atlas_scales(&old.profiles)
                    .map(f32::to_bits)
                    .eq(orb::atlas_scales(&data.profiles).map(f32::to_bits))
            }) {
                self.glyphs
                    .layers(scale, art::atlas_artwork(&data.profiles));
            }
            self.tuning = Some(Arc::clone(data));
        }
    }

    fn request_glyphs(&mut self, scale: f32, wanted: impl IntoIterator<Item = GlyphKey>) {
        if self
            .scale
            .is_none_or(|previous| (scale - previous).abs() > f32::EPSILON)
        {
            self.scale = Some(scale);
            self.glyphs.icons(scale);
            self.glyphs.prepare(prepare_latin(scale));
        }
        self.glyphs.request(wanted);
    }

    fn upload_text(
        &self,
        client: &mut Client,
        renderer: &mut Renderer,
        mut delivered: impl FnMut(&AssetReady),
    ) {
        self.upload_glyphs(renderer, |ready| {
            delivered(&ready);
            client.handle(Input::Device(DeviceInput::Assets(ready)));
        });
    }

    fn upload_glyphs(&self, renderer: &mut Renderer, mut deliver: impl FnMut(AssetReady)) {
        for prepared in self.pages.try_iter() {
            if renderer.upload_page(&prepared) {
                deliver(prepared.ready);
            }
        }
    }

    fn prepare_faces(&mut self, client: &mut Client, renderer: &mut Renderer) {
        let active = client.face_generation();
        let list = client.faces();
        // A match's list is empty until its card records arrive.
        if self.active == Some(active) || list.faces().is_empty() {
            return;
        }
        let format = Format::choose(&renderer.formats());
        let names: Vec<&str> = list
            .faces()
            .iter()
            .map(|face| face.names.first().map_or("", String::as_str))
            .collect();
        renderer.set_faces(&names, format);
        self.results = Some(self.faces.prepare_match(match_faces(list), &[format]));
        self.full.fill(None);
        client.face_arrays_made();
        self.active = Some(active);
    }

    fn request_full(&mut self, client: &mut Client, face: FaceId) -> bool {
        let Some(request) = self.faces.full(face) else {
            return false;
        };
        if let Full::Kept { layer } = request
            && self.full[usize::from(layer)] == Some(face)
        {
            client.handle(Input::Device(DeviceInput::Assets(AssetReady::Face {
                face,
                size: FaceSize::Full,
                layer,
            })));
        }
        true
    }

    fn receive_faces(&self, renderer: &mut Renderer) {
        if let Some(results) = &self.results {
            for prepared in results.try_iter() {
                renderer.hold(prepared);
            }
        }
    }

    fn upload_faces(&mut self, client: &mut Client, renderer: &mut Renderer, wake: Wake) {
        if self.active != Some(client.face_generation()) {
            return;
        }
        for ready in renderer.upload_waiting(wake) {
            if let AssetReady::Face {
                face,
                size: FaceSize::Full,
                layer,
            } = ready
            {
                self.full[usize::from(layer)] = Some(face);
            }
            client.handle(Input::Device(DeviceInput::Assets(ready)));
        }
    }
}
