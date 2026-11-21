//! Registers feature placement, stores state by id and routes typed messages.

use std::any::Any;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Debug;

use crate::command::Request;
use app_screens::screens::bug_report::{self, BugReport};
use app_screens::screens::settings::{self, SettingsSheet};
use app_screens::screens::start::{self, StartScreen};
use app_screens::screens::testing::{self, TestingScreen};
use app_screens::screens::{self, GalleryState, Music, Screen, Sheet};
use app_screens::settings::ScreenSettings;
use beryl_protocol::prompt::{Click, PromptView};
use beryl_protocol::{PlayerView, SessionId};
use game_hud::hud::HudIn;
use game_hud::hud::banner::Banner;
use game_hud::hud::counts::CountFloats;
use game_hud::hud::full_control::{self, FullControl};
use game_hud::hud::inspector::Inspector;
use game_hud::hud::log::{self, GameLog, Logged};
use game_hud::hud::notices::NoticeFloats;
use game_hud::hud::piles::PileCounts;
use game_hud::hud::rows::RowCounts;
use game_hud::hud::steps::{self, StepRow};
use game_hud::hud::viewer::{self, Viewer};
use game_hud::prompts::{self, bar::PromptBar};

mod prompt;
use game_hud::screens::game_over::{self, GameOver};
use game_manual::manual::bindings::Bindings;
use game_table::faces::{FaceList, Faces};
use game_table::profiles::Profile;
use game_table::table::Window;
use kit_scene::{Effect, Feature, FeatureSlot};
use kit_ui::element::{Action, Element};
use kit_ui::layout::FieldChange;
use kit_ui::widgets::{FieldState, Fields};

use super::{Client, SessionCommand};

mod manual;
mod table_input;
use manual::{
    ASK, AskPrompt, CommandList, ContextMenu, LOBBY, LobbyScreen, MENU, ORDER, OrderTray, PALETTE,
    PROPOSALS, ProposalPanel, TABLE, TABLE_LOG, TOKENS, TableLog, TableScreen, TokenSearch,
};

const SETTINGS: &str = "settings";
const START: &str = "start";
const TESTING: &str = "testing";
const BUG_REPORT: &str = "bug-report";
const GAME_OVER: &str = "game-over";
const TOTALS: &str = "life-totals";
const PILES: &str = "pile-counts";
const NOTICES: &str = "notices";
const ROWS: &str = "row-counts";
const BANNER: &str = "step-banner";
const INSPECTOR: &str = "inspector";
const STEPS: &str = "steps";
const FULL_CONTROL: &str = "full-control";
const VIEWER: &str = "pile-viewer";
const REGISTRY: &[Registration] = &[
    Registration::of::<SettingsSheet>(SETTINGS, Placement::Sheet(Sheet::Settings)),
    Registration::of::<GameLog>("game-log", Placement::GameLog),
    Registration::of::<StartScreen>(START, Placement::Screen(Screen::Start)),
    Registration::of::<TestingScreen>(TESTING, Placement::Screen(Screen::Testing)),
    Registration::of::<BugReport>(BUG_REPORT, Placement::Sheet(Sheet::BugReport)),
    Registration::of::<GameOver>(GAME_OVER, Placement::GameOver),
    Registration::prepared::<CountFloats>(TOTALS),
    Registration::prepared::<PileCounts>(PILES),
    Registration::prepared::<NoticeFloats>(NOTICES),
    Registration::prepared::<RowCounts>(ROWS),
    Registration::prepared::<Banner>(BANNER),
    Registration::prepared::<Inspector>(INSPECTOR),
    Registration::routed::<StepRow>(STEPS),
    Registration::routed::<FullControl>(FULL_CONTROL),
    Registration::routed::<Viewer>(VIEWER),
    Registration::prompt::<PromptBar>("prompt-bar", Placement::PromptBar),
    Registration::prompt::<prompts::Pick>("prompt-pick", Placement::Shape("pick")),
    Registration::prompt::<prompts::Order>("prompt-order", Placement::Shape("order")),
    Registration::prompt::<prompts::Split>("prompt-split", Placement::Shape("split")),
    Registration::prompt::<prompts::YesNo>("prompt-yes-no", Placement::Shape("yesNo")),
    Registration::prompt::<prompts::Number>("prompt-number", Placement::Shape("number")),
    Registration::prompt::<prompts::Mode>("prompt-mode", Placement::Shape("mode")),
    Registration::prompt::<prompts::Catalog>("prompt-catalog", Placement::Shape("catalog")),
    Registration::prompt::<prompts::Divide>("prompt-divide", Placement::Shape("divide")),
    Registration::prompt::<prompts::Pay>("prompt-pay", Placement::Shape("pay")),
    Registration::prompt::<prompts::Declare>("prompt-declare", Placement::Shape("declare")),
    Registration::prompt::<prompts::Candidates>("prompt-candidates", Placement::Candidates),
    Registration::of::<TableScreen>(TABLE, Placement::Screen(Screen::Table)),
    Registration::of::<LobbyScreen>(LOBBY, Placement::Screen(Screen::Lobby)),
    Registration::of::<ContextMenu>(MENU, Placement::TableMenu),
    Registration::of::<CommandList>(PALETTE, Placement::TableOverlay),
    Registration::of::<AskPrompt>(ASK, Placement::TableOverlay),
    Registration::of::<TokenSearch>(TOKENS, Placement::TableOverlay),
    Registration::of::<OrderTray>(ORDER, Placement::TableOverlay),
    Registration::of::<TableLog>(TABLE_LOG, Placement::TableLog),
    Registration::of::<ProposalPanel>(PROPOSALS, Placement::TableProposals),
];

/// Locations in the flat layer's composition that take a registered feature.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Placement {
    Screen(Screen),
    Sheet(Sheet),
    GameLog,
    GameOver,
    PromptBar,
    Shape(&'static str),
    Candidates,
    TableMenu,
    TableOverlay,
    TableLog,
    TableProposals,
}

/// App inputs available to mounts. Each feature receives only the inputs it reads.
#[derive(Clone, Copy)]
pub(super) struct ViewInputs<'a> {
    pub drag_threshold: f32,
    pub screen: Screen,
    pub window: Window,
    pub profile: Profile<'a>,
    pub view: Option<&'a PlayerView>,
    pub session: Option<SessionId>,
    pub fields: &'a Fields,
    pub hud: &'a HudIn<'a>,
    pub bindings: &'a Bindings,
    pub faces: &'a FaceList,
    pub game_over: bool,
    pub settings: ScreenSettings,
}

type Slots = BTreeMap<&'static str, Box<dyn Any>>;

struct Registration {
    id: &'static str,
    placement: Option<Placement>,
    create: fn() -> Box<dyn Any>,
    state: fn(&Slots, &'static str) -> String,
    route: fn(&mut Client, &'static str, &Action) -> Option<bool>,
    view: for<'a> fn(&'a mut Features, &'static str, ViewInputs<'_>) -> Option<&'a Element>,
    prompt_view: Option<prompt::Draw>,
}

impl Registration {
    const fn of<F: Mounted>(id: &'static str, placement: Placement) -> Self {
        Self::mounted::<F>(id, Some(placement))
    }

    const fn mounted<F: Mounted>(id: &'static str, placement: Option<Placement>) -> Self {
        Self {
            id,
            placement,
            create: || Box::new(FeatureSlot::<F>::default()),
            state: |slots, id| {
                print_state(slots[id].downcast_ref::<FeatureSlot<F>>().unwrap().state())
            },
            route: route::<F>,
            view: F::mounted_view,
            prompt_view: None,
        }
    }

    // Playback variants are prepared separately in Layer. The registry owns
    // their state even though there is no single screen mount to cache.
    const fn prepared<S: Debug + Default + 'static>(id: &'static str) -> Self {
        Self {
            id,
            placement: None,
            create: || Box::new(S::default()),
            state: |slots, id| print_state(slots[id].downcast_ref::<S>().unwrap()),
            route: |_, _, _| None,
            view: |_, _, _| None,
            prompt_view: None,
        }
    }

    const fn routed<F: Mounted>(id: &'static str) -> Self {
        Self::mounted::<F>(id, None)
    }
}

pub(super) struct Features {
    slots: Slots,
    pub opening: super::viewing::Opening,
    pub screen_cards: super::viewing::ScreenCards,
    pub screen: Screen,
    pub gallery: GalleryState,
    pub gallery_faces: Option<Faces>,
    pub music: Option<Music>,
    pub shown: (Screen, Option<Sheet>),
    pub prompts: prompts::router::Router,
    /// The layer's waiting elements draw and take input after playback settles.
    /// The bug report opens the layer even while it freezes the table.
    pub layer_open: bool,
    /// The route of the open sheet, shared by the settings and bug report.
    pub sheet: Option<Sheet>,
    pub lobby_view: Option<beryl_protocol::ipc::LobbyView>,
    /// Narration derived from the ordered session views, including views no
    /// longer available from playback. It is an input to the game log.
    pub log_history: Logged,
    frame_times: BTreeSet<u64>,
    /// A field to focus once an update's new layout exists.
    pub focus_after_show: Option<&'static str>,
}

impl Default for Features {
    fn default() -> Self {
        Self {
            slots: REGISTRY
                .iter()
                .map(|entry| (entry.id, (entry.create)()))
                .collect(),
            opening: super::viewing::Opening::default(),
            screen_cards: super::viewing::ScreenCards::default(),
            screen: Screen::Game,
            gallery: GalleryState::default(),
            gallery_faces: None,
            music: None,
            shown: (Screen::Game, None),
            prompts: prompts::router::Router::default(),
            layer_open: false,
            sheet: None,
            lobby_view: None,
            log_history: Logged::default(),
            frame_times: BTreeSet::new(),
            focus_after_show: None,
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

    fn state_of<F: Feature + 'static>(&self, id: &'static str) -> &F::State {
        self.slots[id]
            .downcast_ref::<FeatureSlot<F>>()
            .expect("the registry owns this feature's slot")
            .state()
    }

    fn prepared<S: 'static>(&self, id: &'static str) -> &S {
        self.slots[id].downcast_ref().expect("registered HUD state")
    }

    fn prepared_mut<S: 'static>(&mut self, id: &'static str) -> &mut S {
        self.slots
            .get_mut(id)
            .and_then(|slot| slot.downcast_mut())
            .expect("registered HUD state")
    }

    pub fn totals_mut(&mut self) -> &mut CountFloats {
        self.prepared_mut(TOTALS)
    }

    pub fn piles(&self) -> &PileCounts {
        self.prepared(PILES)
    }

    pub fn piles_mut(&mut self) -> &mut PileCounts {
        self.prepared_mut(PILES)
    }

    pub fn notices_mut(&mut self) -> &mut NoticeFloats {
        self.prepared_mut(NOTICES)
    }

    pub fn rows(&self) -> &RowCounts {
        self.prepared(ROWS)
    }

    pub fn rows_mut(&mut self) -> &mut RowCounts {
        self.prepared_mut(ROWS)
    }

    pub fn banner_mut(&mut self) -> &mut Banner {
        self.prepared_mut(BANNER)
    }

    pub fn inspector(&self) -> &Inspector {
        self.prepared(INSPECTOR)
    }

    pub fn inspector_mut(&mut self) -> &mut Inspector {
        self.prepared_mut(INSPECTOR)
    }

    pub fn inspector_and_prompt(&mut self) -> (&mut Inspector, Option<&PromptView>) {
        let inspector = self
            .slots
            .get_mut(INSPECTOR)
            .and_then(|slot| slot.downcast_mut())
            .expect("registered HUD state");
        (inspector, self.prompts.current())
    }

    pub fn steps(&self) -> &steps::State {
        self.state_of::<StepRow>(STEPS)
    }

    pub fn steps_mut(&mut self) -> &mut steps::State {
        Self::slot::<StepRow>(&mut self.slots, STEPS).state_mut()
    }

    pub fn step_view(&mut self, input: steps::Inputs) -> Option<Element> {
        input.visible().then(|| {
            Self::slot::<StepRow>(&mut self.slots, STEPS)
                .view(input)
                .clone()
        })
    }

    pub fn full_control_view(&mut self, input: full_control::Inputs) -> Element {
        Self::slot::<FullControl>(&mut self.slots, FULL_CONTROL)
            .view(input)
            .clone()
    }

    pub fn viewer(&self) -> &viewer::State {
        self.state_of::<Viewer>(VIEWER)
    }

    pub fn viewer_mut(&mut self) -> &mut viewer::State {
        Self::slot::<Viewer>(&mut self.slots, VIEWER).state_mut()
    }

    pub fn show_viewer(
        &mut self,
        open: Option<game_hud::inspect::Viewed>,
        revealed: Vec<viewer::RevealedFace>,
    ) {
        let slot = Self::slot::<Viewer>(&mut self.slots, VIEWER);
        slot.state_mut().show(open, revealed);
        slot.invalidate_view();
    }

    pub fn viewer_view(&mut self, input: viewer::Inputs<'_>) -> Option<Element> {
        let slot = Self::slot::<Viewer>(&mut self.slots, VIEWER);
        let open = slot.state().open()?;
        if matches!(open, game_hud::inspect::Viewed::Pile(..)) {
            input.view?;
        }
        Some(slot.view(input).clone())
    }

    pub fn bug(&self) -> &bug_report::State {
        self.state_of::<BugReport>(BUG_REPORT)
    }

    pub fn reset_result(&mut self) {
        Self::slot::<GameOver>(&mut self.slots, GAME_OVER).update(&game_over::Message::Reset);
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

    pub fn view(&mut self, placement: Placement, inputs: ViewInputs<'_>) -> Option<Element> {
        REGISTRY
            .iter()
            .filter(|entry| entry.placement == Some(placement))
            .find_map(|entry| (entry.view)(self, entry.id, inputs).cloned())
    }

    pub fn state(&self, id: &str) -> Option<String> {
        let entry = REGISTRY.iter().find(|entry| entry.id == id)?;
        Some((entry.state)(&self.slots, entry.id))
    }
}

fn print_state(state: &dyn Debug) -> String {
    format!("{state:#?}")
}

impl Client {
    /// Prints the current state of a registered feature, by id.
    #[must_use]
    pub fn feature_state(&self, id: &str) -> Option<String> {
        self.features.state(id)
    }

    pub(super) fn update_start(&mut self, message: &start::Message) {
        update::<StartScreen>(self, START, message);
    }

    pub(super) fn update_testing(&mut self, message: &testing::Message) {
        update::<TestingScreen>(self, TESTING, message);
    }

    pub(super) fn update_bug(&mut self, message: &bug_report::Message) -> bool {
        update::<BugReport>(self, BUG_REPORT, message)
    }

    pub(super) fn feature_field_changed(&mut self, change: FieldChange) {
        match change.name.as_str() {
            start::SEED => self.update_start(&start::Message::Seed(change.text)),
            testing::FILTER => self.update_testing(&testing::Message::Filter(change.text)),
            bug_report::DESCRIPTION => {
                self.update_bug(&bug_report::Message::Description(change.text));
            }
            _ => {}
        }
    }

    /// Returns whether a registered feature handled the action and changed the view.
    pub(super) fn route_feature(&mut self, action: &Action) -> Option<bool> {
        // Life totals come from the table's playback crate. A target on a
        // life total enters the same session router as a shape's flat controls.
        if let Some(&click @ Click::On { .. }) = action.downcast_ref::<Click>() {
            self.click(click);
            return Some(false);
        }
        REGISTRY
            .iter()
            .find_map(|entry| (entry.route)(self, entry.id, action))
    }
}

/// Adapts each feature's inputs and effects to the application.
trait Mounted: Feature + Sized + 'static {
    fn mounted_view<'a>(
        _: &'a mut Features,
        _: &'static str,
        _: ViewInputs<'_>,
    ) -> Option<&'a Element> {
        None
    }
    fn command(client: &mut Client, command: Self::Command);
    fn request(client: &mut Client, request: Self::Request) -> bool;
}

fn route<F: Mounted>(client: &mut Client, id: &'static str, action: &Action) -> Option<bool> {
    let message = action.downcast_ref::<F::Message>()?;
    Some(update::<F>(client, id, message))
}

fn update<F: Mounted>(client: &mut Client, id: &'static str, message: &F::Message) -> bool {
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
    changed
}

impl Mounted for SettingsSheet {
    fn mounted_view<'a>(
        features: &'a mut Features,
        id: &'static str,
        inputs: ViewInputs<'_>,
    ) -> Option<&'a Element> {
        Some(
            Features::slot::<Self>(&mut features.slots, id).view(settings::Inputs {
                screen: inputs.screen,
                window: inputs.window,
                settings: inputs.settings,
                game_over: inputs.game_over,
                layout: settings::Layout::new(inputs.profile),
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
        inputs: ViewInputs<'_>,
    ) -> Option<&'a Element> {
        inputs.view?;
        let slot = Features::slot::<Self>(&mut features.slots, id);
        let layout = log::Layout::for_profile(inputs.profile, slot.state().open)?;
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

impl Mounted for StartScreen {
    fn mounted_view<'a>(
        features: &'a mut Features,
        id: &'static str,
        inputs: ViewInputs<'_>,
    ) -> Option<&'a Element> {
        let has_recordings = !features
            .state_of::<TestingScreen>(TESTING)
            .recordings
            .is_empty();
        let in_progress = start::in_progress(inputs.view);
        let entries = features.lobby().entries.clone();
        Some(
            Features::slot::<Self>(&mut features.slots, id).view(start::Inputs {
                window: inputs.window,
                column: inputs.profile.screen_column(),
                in_progress,
                session: inputs.session.filter(|_| in_progress),
                has_recordings,
                fields: inputs.fields,
                entries: &entries,
            }),
        )
    }

    fn command(client: &mut Client, command: Self::Command) {
        client.commands.push_back(match command {
            start::Command::Concede(session) => SessionCommand::Concede(session),
            start::Command::Start(request) => SessionCommand::Start(request),
        });
    }

    fn request(client: &mut Client, request: Self::Request) -> bool {
        match request {
            start::Request::Open(screen) => {
                client.features.screen = screen;
                client.features.gallery = GalleryState::default();
                if screen == Screen::Gallery {
                    client
                        .features
                        .gallery_faces
                        .get_or_insert_with(|| Faces::new(screens::gallery_faces()));
                }
                client.change_sheet(None)
            }
            start::Request::AwaitGame => {
                client.features.screen = Screen::Start;
                true
            }
            start::Request::EnterGame => {
                client.features.screen = Screen::Game;
                client.change_sheet(None)
            }
            start::Request::ResumeGame => {
                client.features.screen = Screen::Game;
                true
            }
        }
    }
}

impl Mounted for TestingScreen {
    fn mounted_view<'a>(
        features: &'a mut Features,
        id: &'static str,
        inputs: ViewInputs<'_>,
    ) -> Option<&'a Element> {
        Some(
            Features::slot::<Self>(&mut features.slots, id).view(testing::Inputs {
                window: inputs.window,
                column: inputs.profile.screen_column(),
                filter: inputs.fields.get(testing::FILTER),
            }),
        )
    }

    fn command(_: &mut Client, command: Self::Command) {
        match command {}
    }

    fn request(client: &mut Client, replay: Self::Request) -> bool {
        client.requests.push_back(Request::Replay {
            name: replay.name,
            seat: replay.seat,
        });
        client.features.screen = Screen::Game;
        client.change_sheet(None)
    }
}

impl Mounted for BugReport {
    fn mounted_view<'a>(
        features: &'a mut Features,
        id: &'static str,
        inputs: ViewInputs<'_>,
    ) -> Option<&'a Element> {
        let slot = Features::slot::<Self>(&mut features.slots, id);
        if matches!(slot.state().stage, bug_report::BugStage::Capturing { .. }) {
            return None;
        }
        Some(slot.view(bug_report::Inputs {
            window: inputs.window,
            layout: bug_report::Layout::new(inputs.profile),
            description: inputs.fields.get(bug_report::DESCRIPTION),
        }))
    }

    fn command(_: &mut Client, command: Self::Command) {
        match command {}
    }

    fn request(client: &mut Client, request: Self::Request) -> bool {
        match request {
            bug_report::Request::Open => return client.change_sheet(Some(Sheet::BugReport)),
            bug_report::Request::Close => return client.change_sheet(None),
            bug_report::Request::FreezeFrame => {
                client
                    .layer
                    .set_field(bug_report::DESCRIPTION, FieldState::default());
                client.requests.push_back(Request::FreezeFrame);
            }
            bug_report::Request::FocusDescription => {
                client.features.focus_after_show = Some(bug_report::DESCRIPTION);
            }
            bug_report::Request::ClearFocus => client.layer.focus(None),
            bug_report::Request::Save {
                description,
                ai,
                region,
                window,
            } => {
                client.requests.push_back(Request::SaveBug {
                    description,
                    ai,
                    region,
                    window,
                });
            }
            bug_report::Request::Export => client.requests.push_back(Request::ExportBugs),
        }
        false
    }
}

impl Mounted for GameOver {
    fn mounted_view<'a>(
        features: &'a mut Features,
        id: &'static str,
        inputs: ViewInputs<'_>,
    ) -> Option<&'a Element> {
        let inputs = game_over::Inputs::new(inputs.profile, inputs.view, inputs.faces)?;
        Some(Features::slot::<Self>(&mut features.slots, id).view(inputs))
    }

    fn command(_: &mut Client, command: Self::Command) {
        match command {}
    }

    fn request(client: &mut Client, _: Self::Request) -> bool {
        update::<StartScreen>(client, START, &start::Message::Open(Screen::Start))
    }
}

impl Mounted for StepRow {
    fn command(_: &mut Client, command: Self::Command) {
        match command {}
    }

    fn request(client: &mut Client, request: Self::Request) -> bool {
        client.toggle_stop(request);
        true
    }
}

impl Mounted for FullControl {
    fn command(_: &mut Client, command: Self::Command) {
        match command {}
    }

    fn request(_: &mut Client, request: Self::Request) -> bool {
        match request {}
    }
}

impl Mounted for Viewer {
    fn command(_: &mut Client, command: Self::Command) {
        match command {}
    }

    fn request(client: &mut Client, request: Self::Request) -> bool {
        match request {
            viewer::Message::Open(pile) => client.view(Some(pile)),
            viewer::Message::Close => client.dismiss_viewer(),
            viewer::Message::DismissRevealed => client.dismiss_revealed(),
        }
        false
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
