//! Typed storage for every mounted feature and prompt slot.
//!
//! Each registered feature owns one named field on [`Slots`], exposed through [`SlotOwner`] or
//! [`PromptSlotOwner`]. This replaces runtime map lookups and `Box<dyn Any>` downcasts so a
//! missing or mismatched slot fails to compile instead of panicking.

use std::cell::RefCell;

use app_screens::performance::FpsMonitor;
use app_screens::screens::bug_report::BugReport;
use app_screens::screens::gallery::GalleryScreen;
use app_screens::screens::settings::SettingsSheet;
use app_screens::screens::start::StartScreen;
use app_screens::screens::testing::TestingScreen;
use game_hud::hud::full_control::FullControl;
use game_hud::hud::log::GameLog;
use game_hud::hud::steps::StepRow;
use game_hud::hud::viewer::Viewer;
use game_hud::hud::{
    banner::Banner, counts::CountFloats, inspector::Inspector, notices::NoticeFloats,
    piles::PileCounts, rows::RowCounts,
};
use game_hud::prompts::{self, bar::PromptBar};
use game_hud::screens::game_over::GameOver;
use kit_scene::{Feature, FeatureSlot};

use super::manual::{
    AskPrompt, CommandList, ContextMenu, LobbyScreen, OrderTray, ProposalPanel, TableLog,
    TableScreen, TokenSearch,
};
use super::prompt::Prompt;

/// A feature that owns a typed [`FeatureSlot`] inside [`Slots`].
pub(super) trait SlotOwner: Feature + Sized + 'static {
    fn slot(slots: &Slots) -> &FeatureSlot<Self>;
    fn slot_mut(slots: &mut Slots) -> &mut FeatureSlot<Self>;
}

/// A prompt feature that owns a typed [`prompts::Mounted`] slot inside [`Slots`].
pub(super) trait PromptSlotOwner: Prompt + Sized {
    fn prompt_slot(slots: &Slots) -> &RefCell<prompts::Mounted<Self>>;
}

macro_rules! feature_slots {
    (
        features { $($f_field:ident: $f_ty:ty,)* }
        prompts { $($p_field:ident: $p_ty:ty,)* }
    ) => {
        /// Owns the runtime slot of every registered feature and prompt.
        #[derive(Default)]
        pub(super) struct Slots {
            $($f_field: FeatureSlot<$f_ty>,)*
            $($p_field: RefCell<prompts::Mounted<$p_ty>>,)*
        }

        $(
            impl SlotOwner for $f_ty {
                fn slot(slots: &Slots) -> &FeatureSlot<Self> {
                    &slots.$f_field
                }

                fn slot_mut(slots: &mut Slots) -> &mut FeatureSlot<Self> {
                    &mut slots.$f_field
                }
            }
        )*

        $(
            impl PromptSlotOwner for $p_ty {
                fn prompt_slot(slots: &Slots) -> &RefCell<prompts::Mounted<Self>> {
                    &slots.$p_field
                }
            }
        )*
    };
}

feature_slots! {
    features {
        settings: SettingsSheet,
        game_log: GameLog,
        start: StartScreen,
        testing: TestingScreen,
        gallery: GalleryScreen,
        bug_report: BugReport,
        game_over: GameOver,
        life_totals: CountFloats,
        pile_counts: PileCounts,
        row_counts: RowCounts,
        notices: NoticeFloats,
        step_banner: Banner,
        inspector: Inspector,
        fps_monitor: FpsMonitor,
        steps: StepRow,
        full_control: FullControl,
        viewer: Viewer,
        table: TableScreen,
        lobby: LobbyScreen,
        menu: ContextMenu,
        palette: CommandList,
        ask: AskPrompt,
        tokens: TokenSearch,
        order: OrderTray,
        table_log: TableLog,
        proposals: ProposalPanel,
    }
    prompts {
        prompt_bar: PromptBar,
        prompt_pick: prompts::Pick,
        prompt_order: prompts::Order,
        prompt_split: prompts::Split,
        prompt_yes_no: prompts::YesNo,
        prompt_number: prompts::Number,
        prompt_mode: prompts::Mode,
        prompt_catalog: prompts::Catalog,
        prompt_divide: prompts::Divide,
        prompt_pay: prompts::Pay,
        prompt_declare: prompts::Declare,
        prompt_candidates: prompts::Candidates,
    }
}
