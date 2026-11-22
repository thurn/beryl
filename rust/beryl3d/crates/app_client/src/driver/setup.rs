//! Starts the replay driver in a window using the same data as the client.

use std::sync::Arc;

use beryl_protocol::protocol::Side;
use kit_base::data::Data;
use kit_probe::agent::Layout;
use kit_ui::input::{Size, Tier, Window};

use super::{Capture, Client, Config, Cues, DriverError, Page, Recording, Responses, TICK};

impl Page {
    /// The recording at its start, as `seat` sees it in a window of `size` logical points.
    /// The client data selects the profile. The layout argument is retained for existing callers.
    ///
    /// # Errors
    ///
    /// When a view in the recording cannot be read.
    pub fn new(
        data: Arc<Data>,
        recording: Arc<Recording>,
        seat: Side,
        _layout: Layout,
        size: Size,
    ) -> Result<Self, DriverError> {
        Self::in_window(
            data,
            recording,
            seat,
            Window {
                size,
                ..Window::default()
            },
        )
    }

    /// The recording at its start in the given window, using the client's selected profile.
    ///
    /// # Errors
    ///
    /// When a view in the recording cannot be read.
    pub fn in_window(
        data: Arc<Data>,
        recording: Arc<Recording>,
        seat: Side,
        window: Window,
    ) -> Result<Self, DriverError> {
        let views = recording.views().map_err(DriverError)?.into();
        let cards = recording.card_records().map_err(DriverError)?.map(Arc::new);
        let mut page = Self {
            responses: Responses::shared(Arc::clone(&recording)),
            recording,
            views,
            cards,
            name: "recording".to_owned(),
            seat,
            size: window.size,
            scale: window.scale,
            safe_area: window.safe_area,
            tier: Tier::Baseline,
            tick: TICK,
            cues: Cues::default(),
            client: Client::new(data, Config::LARGEST),
            now: 0,
            #[cfg(feature = "stories")]
            story_moment: None,
            released: 0,
            batch: 0,
            rest_since: None,
            inputs: Vec::new(),
            given_at: Vec::new(),
            capture: Capture::None,
            plants: Vec::new(),
            catch_up: true,
            moments: None,
            steps: None,
            heard: Vec::new(),
            glyphs: Vec::new(),
            stories: Arc::default(),
            stub_atlas: true,
        };
        page.begin();
        Ok(page)
    }
}
