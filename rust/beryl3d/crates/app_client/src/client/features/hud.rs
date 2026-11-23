//! Mounts the HUD floats and routes their updates.

use app_screens::performance::{FpsMonitor, FrameRates};
use beryl_protocol::prompt::Click;
use game_hud::hud::FloatFeature;
use game_hud::hud::banner::Banner;
use game_hud::hud::counts::CountFloats;
use game_hud::hud::inspector::{self, Inspector};
use game_hud::hud::notices::NoticeFloats;
use game_hud::hud::piles::PileCounts;
use game_hud::hud::rows::{self, RowCounts};
use kit_scene::StateUpdate;
use std::convert::Infallible;

use super::{Client, Features, Mounted, Placement, Registration};

pub(super) const LIFE_TOTALS: &str = "life-totals";
pub(super) const PILE_COUNTS: &str = "pile-counts";
pub(super) const ROW_COUNTS: &str = "row-counts";
pub(super) const NOTICES: &str = "notices";
pub(super) const STEP_BANNER: &str = "step-banner";
pub(super) const INSPECTOR: &str = "inspector";
pub(super) const FPS_MONITOR: &str = "fps-monitor";

impl Registration {
    pub(super) const fn floating<F: Mounted + FloatFeature>(id: &'static str) -> Self {
        Self {
            owns_float: |id| F::floats().any(|float| float == id),
            view: |_, _, inputs| {
                let mut floats = F::floats().filter_map(|id| inputs.layer.float_element(id));
                let first = floats.next()?;
                if let Some(second) = floats.next() {
                    Some(
                        kit_ui::element::column()
                            .child(first)
                            .child(second)
                            .children(floats),
                    )
                } else {
                    Some(first)
                }
            },
            ..Self::of::<F>(id, Placement::Floating)
        }
    }
}

impl Features {
    pub fn update_fps_monitor(&mut self, rates: FrameRates) -> bool {
        Self::slot::<FpsMonitor>(&mut self.slots, FPS_MONITOR)
            .update(&rates)
            .0
    }

    pub fn fps_monitor_view(&mut self, compact: bool) -> kit_ui::element::Element {
        Self::slot::<FpsMonitor>(&mut self.slots, FPS_MONITOR)
            .view(compact)
            .clone()
    }

    pub fn totals<U: StateUpdate<CountFloats>>(&mut self, update: U) -> U::Output {
        Self::slot::<CountFloats>(&mut self.slots, LIFE_TOTALS).apply(update)
    }

    pub fn piles(&self) -> &PileCounts {
        self.state_of::<PileCounts>(PILE_COUNTS)
    }

    pub fn update_piles<U: StateUpdate<PileCounts>>(&mut self, update: U) -> U::Output {
        Self::slot::<PileCounts>(&mut self.slots, PILE_COUNTS).apply(update)
    }

    pub fn notices<U: StateUpdate<NoticeFloats>>(&mut self, update: U) -> U::Output {
        Self::slot::<NoticeFloats>(&mut self.slots, NOTICES).apply(update)
    }

    pub fn rows(&self) -> &RowCounts {
        self.state_of::<RowCounts>(ROW_COUNTS)
    }

    pub fn update_rows<U: StateUpdate<RowCounts>>(&mut self, update: U) -> U::Output {
        Self::slot::<RowCounts>(&mut self.slots, ROW_COUNTS).apply(update)
    }

    pub fn focus_row(&mut self, message: rows::Message) {
        Self::slot::<RowCounts>(&mut self.slots, ROW_COUNTS).update(&message);
    }

    pub fn banner<U: StateUpdate<Banner>>(&mut self, update: U) -> U::Output {
        Self::slot::<Banner>(&mut self.slots, STEP_BANNER).apply(update)
    }

    pub fn inspector(&self) -> &Inspector {
        self.state_of::<Inspector>(INSPECTOR)
    }

    pub fn inspect(&mut self, message: &inspector::Message) {
        Self::slot::<Inspector>(&mut self.slots, INSPECTOR).update(message);
    }

    pub fn update_inspector<U: StateUpdate<Inspector>>(&mut self, update: U) -> U::Output {
        Self::slot::<Inspector>(&mut self.slots, INSPECTOR).apply(update)
    }

    pub fn draw_inspector(&mut self, input: inspector::Draw<'_>) -> Option<kit_ui::input::FaceId> {
        Self::slot::<Inspector>(&mut self.slots, INSPECTOR).apply(inspector::Draw {
            prompt: self.prompts.current(),
            ..input
        })
    }
}

trait PassiveFloat: FloatFeature<Command = Infallible, Request = Infallible> {}

impl PassiveFloat for PileCounts {}
impl PassiveFloat for NoticeFloats {}
impl PassiveFloat for RowCounts {}
impl PassiveFloat for Banner {}
impl PassiveFloat for Inspector {}
impl PassiveFloat for FpsMonitor {}

impl<F: PassiveFloat + 'static> Mounted for F {
    fn command(_: &mut Client, command: Infallible) {
        match command {}
    }

    fn request(_: &mut Client, request: Infallible) -> bool {
        match request {}
    }
}

impl Mounted for CountFloats {
    fn command(client: &mut Client, command: Click) {
        client.click(command);
    }

    fn request(_: &mut Client, request: Infallible) -> bool {
        match request {}
    }
}
