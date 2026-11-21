//! Registers feature placement, stores state by id and routes typed messages.

use std::any::Any;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Debug;

use app_screens::screens::settings::{self, SettingsSheet};
use app_screens::screens::{Screen, Sheet};
use app_screens::settings::ScreenSettings;
use game_hud::hud::log::{self, GameLog, Logged};
use game_table::table::Window;
use kit_scene::{Effect, Feature, FeatureSlot};
use kit_ui::element::{Action, Element};

use super::{Client, SessionCommand};

const SETTINGS: &str = "settings";
const REGISTRY: &[Registration] = &[
    Registration::of::<SettingsSheet>(SETTINGS, Placement::Sheet(Sheet::Settings)),
    Registration::of::<GameLog>("game-log", Placement::GameLog),
];

/// Locations in the flat layer's composition that take a registered feature.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Placement {
    Sheet(Sheet),
    GameLog,
}

/// App inputs available to mounts. Each feature receives only the inputs it reads.
#[derive(Clone, Copy)]
pub(super) struct ViewInputs {
    pub screen: Screen,
    pub window: Window,
    pub has_view: bool,
    pub game_over: bool,
    pub settings: ScreenSettings,
}

type Slots = BTreeMap<&'static str, Box<dyn Any>>;

struct Registration {
    id: &'static str,
    placement: Placement,
    create: fn() -> Box<dyn Any>,
    state: for<'a> fn(&'a Slots, &'static str) -> &'a dyn Debug,
    route: fn(&mut Client, &'static str, &Action) -> Option<bool>,
    view: for<'a> fn(&'a mut Features, &'static str, ViewInputs) -> Option<&'a Element>,
}

impl Registration {
    const fn of<F: Mounted>(id: &'static str, placement: Placement) -> Self {
        Self {
            id,
            placement,
            create: || Box::new(FeatureSlot::<F>::default()),
            state: |slots, id| slots[id].downcast_ref::<FeatureSlot<F>>().unwrap().state(),
            route: route::<F>,
            view: F::mounted_view,
        }
    }
}

pub(super) struct Features {
    slots: Slots,
    /// The route of the open sheet, shared by the settings and bug report.
    pub sheet: Option<Sheet>,
    /// Narration derived from the ordered session views, including views no
    /// longer available from playback. It is an input to the game log.
    pub log_history: Logged,
    frame_times: BTreeSet<u64>,
}

impl Default for Features {
    fn default() -> Self {
        Self {
            slots: REGISTRY
                .iter()
                .map(|entry| (entry.id, (entry.create)()))
                .collect(),
            sheet: None,
            log_history: Logged::default(),
            frame_times: BTreeSet::new(),
        }
    }
}

impl Features {
    fn slot<'a, F: Feature + 'static>(
        slots: &'a mut Slots,
        id: &'static str,
    ) -> &'a mut FeatureSlot<F> {
        slots
            .get_mut(id)
            .and_then(|slot| slot.downcast_mut())
            .expect("the registry owns this feature's slot")
    }

    fn request_frame(&mut self, at: u64) {
        self.frame_times.insert(at);
    }

    pub fn begin_frame(&mut self, now: u64) {
        while self.next_frame().is_some_and(|at| at <= now) {
            self.frame_times.pop_first();
        }
    }

    pub fn next_frame(&self) -> Option<u64> {
        self.frame_times.first().copied()
    }

    pub fn reset_sheet(&mut self) {
        Self::slot::<SettingsSheet>(&mut self.slots, SETTINGS).update(&settings::Message::Reset);
    }

    pub fn view(&mut self, placement: Placement, inputs: ViewInputs) -> Option<Element> {
        let entry = REGISTRY.iter().find(|entry| entry.placement == placement)?;
        (entry.view)(self, entry.id, inputs).cloned()
    }

    pub fn state(&self, id: &str) -> Option<&dyn Debug> {
        let entry = REGISTRY.iter().find(|entry| entry.id == id)?;
        Some((entry.state)(&self.slots, entry.id))
    }
}

impl Client {
    /// Prints the current state of a registered feature, by id.
    #[must_use]
    pub fn feature_state(&self, id: &str) -> Option<String> {
        self.features.state(id).map(|state| format!("{state:#?}"))
    }

    /// Returns whether a registered feature handled the action and changed the view.
    pub(super) fn route_feature(&mut self, action: &Action) -> Option<bool> {
        REGISTRY
            .iter()
            .find_map(|entry| (entry.route)(self, entry.id, action))
    }
}

/// Adapts each feature's inputs and effects to the application.
trait Mounted: Feature + Sized + 'static {
    fn mounted_view<'a>(
        features: &'a mut Features,
        id: &'static str,
        inputs: ViewInputs,
    ) -> Option<&'a Element>;
    fn command(client: &mut Client, command: Self::Command);
    fn request(client: &mut Client, request: Self::Request) -> bool;
}

fn route<F: Mounted>(client: &mut Client, id: &'static str, action: &Action) -> Option<bool> {
    let message = action.downcast_ref::<F::Message>()?;
    let (mut changed, effects) =
        Features::slot::<F>(&mut client.features.slots, id).update(message);
    for effect in effects {
        match effect {
            Effect::Command(command) => F::command(client, command),
            Effect::Request(request) => changed |= F::request(client, request),
            Effect::Message(action) => changed |= client.route_feature(&action).unwrap_or(false),
            Effect::FrameAt(at) => client.features.request_frame(at),
        }
    }
    Some(changed)
}

impl Mounted for SettingsSheet {
    fn mounted_view<'a>(
        features: &'a mut Features,
        id: &'static str,
        inputs: ViewInputs,
    ) -> Option<&'a Element> {
        Some(
            Features::slot::<Self>(&mut features.slots, id).view(settings::Inputs {
                screen: inputs.screen,
                window: inputs.window,
                settings: inputs.settings,
                game_over: inputs.game_over,
            }),
        )
    }

    fn command(client: &mut Client, command: Self::Command) {
        match command {
            settings::Command::Concede => {
                if let Some(session) = client.session {
                    client.commands.push_back(SessionCommand::Concede(session));
                }
            }
        }
    }

    fn request(client: &mut Client, request: Self::Request) -> bool {
        match request {
            settings::Request::Open => client.change_sheet(Some(Sheet::Settings)),
            settings::Request::Close => client.change_sheet(None),
            settings::Request::ReportBug => client.change_sheet(Some(Sheet::BugReport)),
            settings::Request::Change(setting) => {
                client.set(setting);
                true
            }
        }
    }
}

impl Mounted for GameLog {
    fn mounted_view<'a>(
        features: &'a mut Features,
        id: &'static str,
        inputs: ViewInputs,
    ) -> Option<&'a Element> {
        if !inputs.has_view {
            return None;
        }
        let slot = Features::slot::<Self>(&mut features.slots, id);
        let layout = if inputs.window.phone() {
            if !slot.state().open {
                return None;
            }
            log::Layout::Phone(inputs.window)
        } else {
            log::Layout::Desktop
        };
        Some(slot.view(log::Inputs {
            layout,
            history: &features.log_history,
        }))
    }

    fn command(_: &mut Client, command: Self::Command) {
        match command {}
    }

    fn request(client: &mut Client, request: Self::Request) -> bool {
        match request {
            log::Request::DismissSheet => {
                client.features.sheet.is_some() && client.change_sheet(None)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Features;

    #[test]
    fn an_earlier_frame_keeps_the_later_requests_pending() {
        let mut features = Features::default();
        features.request_frame(20);
        features.request_frame(10);
        features.request_frame(20);
        assert_eq!(features.next_frame(), Some(10));

        features.begin_frame(15);
        assert_eq!(features.next_frame(), Some(20));

        features.begin_frame(20);
        assert_eq!(features.next_frame(), None);
    }
}
