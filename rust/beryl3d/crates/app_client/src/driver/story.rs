//! Story inputs read from the driver, and authored settings rendered by the same feature.

use std::sync::Arc;

use kit_probe::agent::{TreeOptions, tree};

use super::Page;

impl Page {
    /// The stories its host makes available to the gallery and testing screens.
    #[must_use]
    pub fn with_stories(mut self, stories: Arc<[crate::stories::Source]>) -> Self {
        self.stories = stories;
        self.restart();
        self
    }

    /// Applies the driver actions stored at a captured story's moment.
    ///
    /// # Errors
    ///
    /// If an action is invalid or its locator cannot be reached.
    pub fn story_actions(&mut self, actions: &[String]) -> Result<(), String> {
        for action in actions {
            let parsed = action
                .parse::<super::Action>()
                .map_err(|why| why.to_string())?;
            if let super::Action::Type(locator, words) = &parsed
                && self
                    .try_find(locator)
                    .is_ok_and(|field| field.state.has(kit_frame::frame::StateBits::FOCUSED))
            {
                use kit_ui::input::{Input as DeviceInput, TextChange};

                self.give_input(crate::Input::Device(DeviceInput::Text(TextChange::Insert(
                    words.clone(),
                ))));
            } else {
                self.try_act(&parsed).map_err(|why| why.to_string())?;
            }
        }
        Ok(())
    }

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

pub use playback::settings_element;

mod playback {
    use app_screens::screens::{
        Screen,
        settings::{self, SettingsSheet},
    };
    #[cfg(feature = "stories")]
    use game_probe::story::Capture;
    use game_probe::story::SettingsStory;
    use game_table::profiles::Profile;
    use kit_base::{data::Data, geometry::Window};
    #[cfg(feature = "stories")]
    use kit_probe::agent::Judged;
    use kit_scene::Feature;
    use kit_ui::element::Element;

    use crate::driver::Page;
    #[cfg(feature = "stories")]
    use crate::driver::{Checked, Duration, Moment};
    use crate::settings::{Graphics, Settings};

    impl Page {
        /// A portable capture of the current instant, including the client inputs that led to it.
        #[must_use]
        #[cfg(feature = "stories")]
        pub fn capture(&self) -> Capture {
            Capture {
                source: self.name.clone(),
                at: self.now,
                seat: self.seat,
                scale: self.scale,
                catch_up: self.catch_up,
                messages: self.recording.0.clone(),
                story_catalog: Some(self.stories.clone()),
                actions: Vec::new(),
            }
        }

        /// Marks the instant a table story exposes as its start and end.
        #[must_use]
        #[cfg(feature = "stories")]
        pub fn with_story_moment(mut self, at: u64) -> Self {
            self.story_moment = Some(at);
            self
        }

        #[must_use]
        #[cfg(feature = "stories")]
        pub fn story_moment(&self) -> Option<u64> {
            self.story_moment
        }

        /// Checks only the instant a captured table story exposes.
        #[cfg(feature = "stories")]
        pub fn check_story(&mut self) -> Option<Checked> {
            let moment = Moment::At(Duration(self.story_moment?));
            self.seek(&moment).ok()?;
            let context = self.context();
            let frame = self.frame();
            let mut judged = Judged::default();
            judged.frame(frame, &context);
            game_probe::agent::count_frame(&mut judged, frame);
            Some(Checked {
                faults: game_probe::agent::check(frame, &context),
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
        #[cfg(feature = "stories")]
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
