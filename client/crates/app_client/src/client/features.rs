//! Registers feature placement, stores typed feature slots and routes typed messages.

use std::collections::BTreeSet;
use std::fmt::Debug;

use crate::command::Request;
use app_screens::performance::FpsMonitor;
use app_screens::screens::bug_report::{self, BugReport};
use app_screens::screens::gallery::{self, GalleryScreen};
use app_screens::screens::settings::{self, SettingsSheet};
use app_screens::screens::start::{self, StartScreen};
use app_screens::screens::testing::{self, TestingScreen};
use app_screens::screens::{Music, Screen, Sheet};
use app_screens::settings::ScreenSettings;
use beryl_protocol::prompt::Shape;
use beryl_protocol::{PlayerView, SessionId};
use game_hud::hud::HudIn;
use game_hud::hud::full_control::{self, FullControl};
use game_hud::hud::log::{self, GameLog, Logged};
use game_hud::hud::steps::{self, StepRow};
use game_hud::hud::viewer::{self, Viewer};
use game_hud::prompts::{self, bar::PromptBar};

mod prompt;
mod slots;
use game_hud::screens::game_over::{self, GameOver};
use game_manual::manual::bindings::Bindings;
use game_manual::screens::table_entries::TableEntries;
use game_table::faces::FaceList;
use game_table::profiles::Profile;
use game_table::table::Window;
use kit_scene::{Effect, FeatureSlot, StateUpdate};
use kit_ui::element::{Action, Element};
use kit_ui::layout::{FieldChange, Layer};
use kit_ui::widgets::{FieldState, Fields};

use super::{Client, SessionCommand};

mod hud;
mod inspect;
mod keys;
use game_hud::hud::{
    banner::Banner, counts::CountFloats, inspector::Inspector, notices::NoticeFloats,
    piles::PileCounts, rows::RowCounts,
};
pub use inspect::{FeatureInfo, composed_features, registered_features};
mod manual;
mod table_input;
use manual::{
    AskPrompt, CommandList, ContextMenu, LobbyScreen, OrderTray, ProposalPanel, TableLog,
    TableScreen, TokenSearch,
};
use slots::{SlotOwner, Slots};

pub(super) const BUG_REPORT: &str = "bug-report";
const REGISTRY: &[Registration] = &[
    Registration::of::<SettingsSheet>("settings", Placement::Sheet(Sheet::Settings)),
    Registration::of::<GameLog>("game-log", Placement::GameLog),
    Registration::of::<StartScreen>("start", Placement::Screen(Screen::Start))
        .keys(&start::SHORTCUTS, keys::start),
    Registration::of::<TestingScreen>("testing", Placement::Screen(Screen::Testing)),
    Registration::of::<GalleryScreen>("gallery", Placement::Screen(Screen::Gallery)),
    Registration::of::<BugReport>(BUG_REPORT, Placement::Sheet(Sheet::BugReport))
        .keys(&keys::BUG_REPORT_KEYS, keys::bug_report),
    Registration::of::<GameOver>("game-over", Placement::GameOver),
    Registration::floating::<CountFloats>("life-totals"),
    Registration::floating::<PileCounts>("pile-counts"),
    Registration::floating::<RowCounts>("row-counts"),
    Registration::floating::<NoticeFloats>("notices"),
    Registration::floating::<Banner>("step-banner"),
    Registration::floating::<Inspector>("inspector"),
    Registration::floating::<FpsMonitor>("fps-monitor"),
    Registration::routed::<StepRow>("steps"),
    Registration::routed::<FullControl>("full-control"),
    Registration::routed::<Viewer>("pile-viewer"),
    Registration::prompt::<PromptBar>("prompt-bar", Placement::PromptBar)
        .keys(&keys::PROMPT_BAR, keys::prompt_bar),
    Registration::prompt::<prompts::Pick>("prompt-pick", Placement::Shape(Shape::Pick)),
    Registration::prompt::<prompts::Order>("prompt-order", Placement::Shape(Shape::Order)),
    Registration::prompt::<prompts::Split>("prompt-split", Placement::Shape(Shape::Split)),
    Registration::prompt::<prompts::YesNo>("prompt-yes-no", Placement::Shape(Shape::YesNo)),
    Registration::prompt::<prompts::Number>("prompt-number", Placement::Shape(Shape::Number)),
    Registration::prompt::<prompts::Mode>("prompt-mode", Placement::Shape(Shape::Mode)),
    Registration::prompt::<prompts::Catalog>("prompt-catalog", Placement::Shape(Shape::Catalog)),
    Registration::prompt::<prompts::Divide>("prompt-divide", Placement::Shape(Shape::Divide)),
    Registration::prompt::<prompts::Pay>("prompt-pay", Placement::Shape(Shape::Pay)),
    Registration::prompt::<prompts::Declare>("prompt-declare", Placement::Shape(Shape::Declare)),
    Registration::prompt::<prompts::Candidates>("prompt-candidates", Placement::Candidates),
    Registration::of::<TableScreen>("manual-table", Placement::Screen(Screen::Table))
        .keys(&keys::TABLE, keys::table),
    Registration::of::<LobbyScreen>("table-lobby", Placement::Screen(Screen::Lobby))
        .keys(&game_manual::features::lobby::SHORTCUTS, keys::lobby),
    Registration::of::<ContextMenu>("table-context-menu", Placement::TableMenu),
    Registration::of::<CommandList>("table-command-list", Placement::TableOverlay),
    Registration::of::<AskPrompt>("table-ask", Placement::TableOverlay),
    Registration::of::<TokenSearch>("table-token-search", Placement::TableOverlay),
    Registration::of::<OrderTray>("table-library-order", Placement::TableOverlay),
    Registration::of::<TableLog>("table-log", Placement::TableLog),
    Registration::of::<ProposalPanel>("table-proposals", Placement::TableProposals),
];

/// Locations in the flat layer's composition that take a registered feature.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Placement {
    Screen(Screen),
    Sheet(Sheet),
    GameLog,
    Floating,
    GameOver,
    PromptBar,
    Shape(Shape),
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
    pub layer: &'a Layer,
    pub hud: &'a HudIn<'a>,
    pub bindings: &'a Bindings,
    pub faces: &'a FaceList,
    pub game_over: bool,
    pub settings: ScreenSettings,
    pub start: Option<(bool, &'a TableEntries)>,
    pub link_error: Option<&'a str>,
    pub explaining: bool,
}

struct Registration {
    owner: fn() -> &'static str,
    owns_float: fn(kit_ui::element::FloatId) -> bool,
    id: &'static str,
    source: &'static str,
    placement: Option<Placement>,
    state: fn(&Slots) -> String,
    route: fn(&mut Client, &Action) -> Option<bool>,
    view: fn(&mut Features, ViewInputs<'_>) -> Option<Element>,
    prompt_view: Option<prompt::Draw>,
    /// The keyboard shortcuts the feature answers, and what a press of one sends (`keys`).
    shortcuts: &'static [kit_scene::Shortcut],
    on_key: keys::OnKey,
}

impl Registration {
    const fn of<F: Mounted>(id: &'static str, placement: Placement) -> Self {
        Self::mounted::<F>(id, Some(placement))
    }

    const fn mounted<F: Mounted>(id: &'static str, placement: Option<Placement>) -> Self {
        Self {
            owner: std::any::type_name::<F>,
            owns_float: |_| false,
            id,
            source: F::SOURCE,
            placement,
            state: |slots| print_state(F::slot(slots).state()),
            route: route::<F>,
            view: |features, inputs| F::mounted_view(features, inputs).cloned(),
            prompt_view: None,
            shortcuts: &[],
            on_key: keys::none,
        }
    }

    const fn routed<F: Mounted>(id: &'static str) -> Self {
        Self::mounted::<F>(id, None)
    }

    const fn keys(self, shortcuts: &'static [kit_scene::Shortcut], on_key: keys::OnKey) -> Self {
        Self {
            shortcuts,
            on_key,
            ..self
        }
    }
}

pub(super) struct Features {
    slots: Slots,
    pub opening: super::viewing::Opening,
    pub screen_cards: super::viewing::ScreenCards,
    pub screen: Screen,
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
            slots: Slots::default(),
            opening: super::viewing::Opening::default(),
            screen_cards: super::viewing::ScreenCards::default(),
            screen: Screen::Game,
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
    pub(super) fn float_owner(id: kit_ui::element::FloatId) -> Option<&'static str> {
        REGISTRY
            .iter()
            .find(|entry| (entry.owns_float)(id))
            .map(|entry| (entry.owner)())
    }

    fn slot<F: SlotOwner>(&mut self) -> &mut FeatureSlot<F> {
        F::slot_mut(&mut self.slots)
    }

    fn state_of<F: SlotOwner>(&self) -> &F::State {
        F::slot(&self.slots).state()
    }

    pub fn steps(&self) -> &steps::State {
        self.state_of::<StepRow>()
    }

    pub fn update_steps<U: StateUpdate<StepRow>>(&mut self, update: U) -> U::Output {
        self.slot::<StepRow>().apply(update)
    }

    pub fn step_view(&mut self, input: steps::Inputs) -> Option<Element> {
        input
            .visible()
            .then(|| self.slot::<StepRow>().view(input).clone())
    }

    pub fn full_control_view(&mut self, input: full_control::Inputs) -> Element {
        self.slot::<FullControl>().view(input).clone()
    }

    pub fn viewer(&self) -> &viewer::State {
        self.state_of::<Viewer>()
    }

    pub fn viewer_playback(&mut self, update: viewer::Playback<'_>) {
        self.slot::<Viewer>().apply(update);
    }

    pub fn show_viewer(
        &mut self,
        open: Option<game_hud::inspect::Viewed>,
        revealed: Vec<viewer::RevealedFace>,
    ) {
        self.slot::<Viewer>()
            .update(&viewer::Message::Show(open, revealed));
    }

    pub fn viewer_view(&mut self, input: viewer::Inputs<'_>) -> Option<Element> {
        let slot = self.slot::<Viewer>();
        let open = slot.state().open()?;
        if matches!(open, game_hud::inspect::Viewed::Pile(..)) {
            input.view?;
        }
        Some(slot.view(input).clone())
    }

    pub fn bug(&self) -> &bug_report::State {
        self.state_of::<BugReport>()
    }

    pub fn reset_result(&mut self) {
        self.slot::<GameOver>().update(&game_over::Message::Reset);
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
        self.slot::<SettingsSheet>()
            .update(&settings::Message::Reset);
    }

    pub fn view(&mut self, placement: Placement, inputs: ViewInputs<'_>) -> Option<Element> {
        REGISTRY
            .iter()
            .filter(|entry| entry.placement == Some(placement))
            .find_map(|entry| self.mount(entry, inputs))
    }

    pub fn view_named(&mut self, id: &str, inputs: ViewInputs<'_>) -> Option<Element> {
        let entry = REGISTRY.iter().find(|entry| entry.id == id)?;
        if entry.prompt_view.is_some() {
            return self.named_prompt_view(entry.placement?, *inputs.hud);
        }
        self.mount(entry, inputs)
    }

    fn mount(&mut self, entry: &Registration, inputs: ViewInputs<'_>) -> Option<Element> {
        let entries;
        let link_error;
        let inputs = match entry.placement {
            Some(Placement::Screen(Screen::Start)) => {
                entries = self.lobby().entries.clone();
                let has_recordings = !self.state_of::<TestingScreen>().recordings.is_empty();
                ViewInputs {
                    start: Some((has_recordings, &entries)),
                    ..inputs
                }
            }
            Some(Placement::Screen(Screen::Table)) => {
                link_error = self.lobby().lobby.link_error.clone();
                ViewInputs {
                    link_error: link_error.as_deref(),
                    ..inputs
                }
            }
            _ => inputs,
        };
        let mut element = (entry.view)(self, inputs)?;
        if inputs.explaining {
            element.probe_owner = Some((entry.owner)());
        }
        Some(element)
    }

    #[cfg(feature = "stories")]
    pub fn settings_state(&self) -> &settings::State {
        self.state_of::<SettingsSheet>()
    }

    pub fn state(&self, id: &str) -> Option<String> {
        let entry = REGISTRY.iter().find(|entry| entry.id == id)?;
        Some((entry.state)(&self.slots))
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
        update::<StartScreen>(self, message);
    }

    pub(super) fn update_gallery(&mut self, message: &gallery::Message) {
        update::<GalleryScreen>(self, message);
    }

    pub(super) fn update_testing(&mut self, message: &testing::Message) {
        update::<TestingScreen>(self, message);
    }

    pub(super) fn update_bug(&mut self, message: &bug_report::Message) -> bool {
        update::<BugReport>(self, message)
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
        REGISTRY
            .iter()
            .find_map(|entry| (entry.route)(self, action))
    }
}

/// Adapts each feature's inputs and effects to the application.
trait Mounted: SlotOwner {
    fn mounted_view<'a>(_: &'a mut Features, _: ViewInputs<'_>) -> Option<&'a Element> {
        None
    }
    fn command(client: &mut Client, command: Self::Command);
    fn request(client: &mut Client, request: Self::Request) -> bool;
}

fn route<F: Mounted>(client: &mut Client, action: &Action) -> Option<bool> {
    let message = action.downcast_ref::<F::Message>()?;
    Some(update::<F>(client, message))
}

fn update<F: Mounted>(client: &mut Client, message: &F::Message) -> bool {
    let (mut changed, effects) = client.features.slot::<F>().update(message);
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
    fn mounted_view<'a>(features: &'a mut Features, inputs: ViewInputs<'_>) -> Option<&'a Element> {
        Some(features.slot::<Self>().view(settings::Inputs {
            screen: inputs.screen,
            window: inputs.window,
            settings: inputs.settings,
            game_over: inputs.game_over,
            layout: settings::Layout::new(inputs.profile),
        }))
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
    fn mounted_view<'a>(features: &'a mut Features, inputs: ViewInputs<'_>) -> Option<&'a Element> {
        let view = inputs.view?;
        let slot = Self::slot_mut(&mut features.slots);
        let layout = log::Layout::for_table(
            inputs.profile,
            inputs.window,
            slot.state().open,
            view.shared.is_some(),
        )?;
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
    fn mounted_view<'a>(features: &'a mut Features, inputs: ViewInputs<'_>) -> Option<&'a Element> {
        let (has_recordings, entries) = inputs.start?;
        let in_progress = start::in_progress(inputs.view);
        Some(features.slot::<Self>().view(start::Inputs {
            tooltip: inputs.hud.data.flat.tooltip,
            window: inputs.window,
            column: inputs.profile.screen_column(),
            in_progress,
            session: inputs.session.filter(|_| in_progress),
            has_recordings,
            fields: inputs.fields,
            entries,
        }))
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

impl Mounted for GalleryScreen {
    fn mounted_view<'a>(features: &'a mut Features, inputs: ViewInputs<'_>) -> Option<&'a Element> {
        Some(features.slot::<Self>().view(gallery::Inputs {
            window: inputs.window,
            layout: inputs.hud.data.flat.gallery,
            arrangement: inputs.profile.data().gallery_arrangement,
        }))
    }

    fn command(_: &mut Client, command: Self::Command) {
        match command {}
    }

    fn request(client: &mut Client, request: Self::Request) -> bool {
        match request {
            gallery::Request::Open(name) => client.select_story(&name),
            gallery::Request::Restart => client.restart_story(),
        }
        true
    }
}

impl Mounted for TestingScreen {
    fn mounted_view<'a>(features: &'a mut Features, inputs: ViewInputs<'_>) -> Option<&'a Element> {
        Some(features.slot::<Self>().view(testing::Inputs {
            window: inputs.window,
            column: inputs.profile.screen_column(),
            filter: inputs.fields.get(testing::FILTER),
        }))
    }

    fn command(_: &mut Client, command: Self::Command) {
        match command {}
    }

    fn request(client: &mut Client, request: Self::Request) -> bool {
        match request {
            testing::Request::Replay { name, seat } => {
                client.requests.push_back(Request::Replay { name, seat });
                client.features.screen = Screen::Game;
                client.change_sheet(None)
            }
            testing::Request::Story(name) => {
                client.update_gallery(&gallery::Message::Open(name));
                update::<StartScreen>(client, &start::Message::Open(Screen::Gallery))
            }
        }
    }
}

impl Mounted for BugReport {
    fn mounted_view<'a>(features: &'a mut Features, inputs: ViewInputs<'_>) -> Option<&'a Element> {
        let slot = features.slot::<Self>();
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
    fn mounted_view<'a>(features: &'a mut Features, inputs: ViewInputs<'_>) -> Option<&'a Element> {
        let inputs = game_over::Inputs::new(inputs.profile, inputs.view, inputs.faces)?;
        Some(features.slot::<Self>().view(inputs))
    }

    fn command(_: &mut Client, command: Self::Command) {
        match command {}
    }

    fn request(client: &mut Client, _: Self::Request) -> bool {
        update::<StartScreen>(client, &start::Message::Open(Screen::Start))
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
            viewer::Request::Open(pile) => client.view(Some(pile)),
            viewer::Request::Close => client.dismiss_viewer(),
            viewer::Request::DismissRevealed => client.dismiss_revealed(),
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use beryl_protocol::prompt::Shape;

    use super::{Features, Placement, REGISTRY};

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

    #[test]
    fn every_prompt_shape_has_a_unique_registry_entry() {
        let all = [
            Shape::Pick,
            Shape::Order,
            Shape::Split,
            Shape::YesNo,
            Shape::Number,
            Shape::Mode,
            Shape::Catalog,
            Shape::Divide,
            Shape::Pay,
            Shape::Declare,
        ];
        for shape in all {
            // Exhaustive match so adding a new Shape variant updates this test.
            match shape {
                Shape::Pick
                | Shape::Order
                | Shape::Split
                | Shape::YesNo
                | Shape::Number
                | Shape::Mode
                | Shape::Catalog
                | Shape::Divide
                | Shape::Pay
                | Shape::Declare => {}
            }
            let matches = REGISTRY
                .iter()
                .filter(|entry| entry.placement == Some(Placement::Shape(shape)))
                .count();
            assert_eq!(matches, 1, "expected one registry entry for {shape:?}");
        }
    }
}
