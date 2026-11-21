//! Story inputs read from the driver, and authored settings rendered by the same feature.

use kit_probe::agent::{TreeOptions, tree};

use super::Page;

impl Page {
    /// Prints the current state of a registered feature, by id.
    #[must_use]
    pub fn feature_state(&self, id: &str) -> Option<String> {
        self.client.feature_state(id)
    }

    /// The tree printout of the present frame.
    pub fn tree(&mut self) -> String {
        let frame = self.frame();
        tree(frame, &TreeOptions::default()).expect("the whole tree needs no locator")
    }
}

#[cfg(feature = "stories")]
pub use playback::settings_element;

#[cfg(feature = "stories")]
mod playback {
    use app_screens::screens::{
        Screen,
        settings::{self, SettingsSheet},
    };
    use game_probe::story::{Capture, SettingsStory};
    use game_table::profiles::Profile;
    use kit_base::{data::Data, geometry::Window};
    use kit_probe::agent::{Judged, checks};
    use kit_scene::Feature;
    use kit_ui::element::Element;

    use crate::driver::{Checked, Duration, Moment, Page};
    use crate::settings::{Graphics, Settings};

    impl Page {
        /// A portable capture of the current instant, including the client inputs that led to it.
        #[must_use]
        pub fn capture(&self) -> Capture {
            Capture {
                source: self.name.clone(),
                at: self.now,
                seat: self.seat,
                scale: self.scale,
                catch_up: self.catch_up,
                messages: self.recording.0.clone(),
            }
        }

        /// Marks the instant a table story exposes as its start and end.
        #[must_use]
        pub fn with_story_moment(mut self, at: u64) -> Self {
            self.story_moment = Some(at);
            self
        }

        #[must_use]
        pub fn story_moment(&self) -> Option<u64> {
            self.story_moment
        }

        /// Checks only the instant a captured table story exposes.
        pub fn check_story(&mut self) -> Option<Checked> {
            let moment = Moment::At(Duration(self.story_moment?));
            self.seek(&moment).ok()?;
            let context = self.context();
            let frame = self.frame();
            let mut judged = Judged::default();
            judged.frame(frame, context.layout);
            Some(Checked {
                faults: checks::check(frame, &context),
                frames: 1,
                line_pairs: 0,
                judged,
            })
        }

        /// The view of a registered feature that has a standalone mount at this moment.
        pub fn feature_element(&mut self, id: &str) -> Option<Element> {
            self.client.feature_element(id)
        }

        /// The settings sheet's transient state and the settings and screen its view reads.
        #[must_use]
        pub fn settings_story(&self) -> SettingsStory {
            self.client.settings_story()
        }
    }

    /// Draws an authored settings story using the registered settings feature's view function.
    #[must_use]
    pub fn settings_element(input: &SettingsStory, window: Window, data: &Data) -> Element {
        let mut settings = Settings::default().screen();
        settings.music = input.music.unwrap_or(settings.music);
        settings.effects = input.effects.unwrap_or(settings.effects);
        settings.muted = input.muted.unwrap_or(settings.muted);
        settings.full_control = input.full_control.unwrap_or(settings.full_control);
        settings.graphics = match input.graphics.as_deref() {
            Some("baseline") => Graphics::Baseline,
            Some("enhanced") => Graphics::Enhanced,
            _ => Graphics::Automatic,
        };
        let screen = Screen::ALL
            .into_iter()
            .find(|screen| Some(screen.name()) == input.screen.as_deref())
            .unwrap_or(Screen::Game);
        SettingsSheet::view(
            &settings::State {
                confirm_concede: input.confirm_concede,
            },
            settings::Inputs {
                window,
                screen,
                settings,
                game_over: input.game_over,
                layout: settings::Layout::new(Profile::for_window(&data.profiles, window)),
            },
        )
    }
}
