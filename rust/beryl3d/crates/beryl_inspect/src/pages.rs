//! Recordings by name, played through the test driver (docs/beryl-3d.md, "The test driver"):
//! `Page::fixture` and `Page::spec`, and `check_recording`, which runs every interface check
//! on every tick of a recording. A name is a fixture's, a card spec moment's as
//! `<card>/<moment>`, or the name of a stress board or screen story.

use std::sync::{Arc, OnceLock};

use app_client::driver::{Duration, Page, RECORDINGS, Recording, TICK};
use beryl_protocol::Seat;
use beryl_protocol::ipc::FixtureMessage;
use beryl_protocol::protocol::Side;
use game_probe::story::Body;
use kit_probe::agent::Layout;
use kit_ui::input::{Button, Insets, Modifiers, Pointer, PointerKind, PointerPhase, Size};

use crate::faults::Listed;
use crate::runner::Test;
use crate::stress::{Board, pointer_moves};
use crate::{recordings, stories};

use kit_base::data::Data;
use kit_base::data::profiles::{Checks, ProfileData};
pub use kit_base::geometry::Window;

fn profiles() -> &'static kit_base::data::profiles::Profiles {
    &crate::data::checkout_ref()
        .expect("layout profiles")
        .profiles
}

fn by_layout(layout: Layout) -> &'static ProfileData {
    profiles()
        .iter()
        .find(|profile| profile.size_class == layout.size_class())
        .expect("the inspector's layouts have profiles")
}

/// The layout label of the profile that applies to this window.
#[must_use]
pub fn layout_for_size(size: Size) -> Layout {
    Layout::for_size(profiles(), size)
}

/// The interface thresholds associated with a recording's layout.
#[must_use]
pub fn layout_checks(layout: Layout, size: Size) -> Checks {
    layout.checks(profiles(), size)
}

/// The safe insets the recordings use for this layout.
#[must_use]
pub fn safe_area(layout: Layout) -> Insets {
    by_layout(layout).test_safe_area()
}

/// An explicitly selected profile and its isolated inspection data.
#[derive(Debug)]
pub struct InspectionProfile {
    source: (&'static str, &'static str),
    data: OnceLock<Result<Arc<Data>, String>>,
}

impl PartialEq for InspectionProfile {
    fn eq(&self, other: &Self) -> bool {
        self.source == other.source
    }
}

impl Eq for InspectionProfile {}

impl InspectionProfile {
    pub(crate) fn data(&self) -> Result<&Arc<Data>, String> {
        self.data
            .get_or_init(|| {
                let incoming = crate::data::checkout_ref()?;
                let mut data = incoming.as_ref().clone();
                let (name, file) = self.source;
                let path = crate::data::directory().join(file);
                let text = std::fs::read_to_string(&path)
                    .map_err(|error| format!("{}: {error}", path.display()))?;
                data.profiles = data.profiles.with_preview(file, &text, data.card)?;
                if data.profiles.for_size(Size::default()).name != name {
                    return Err(format!("{file}: expected profile name {name}"));
                }
                Ok(Arc::new(data))
            })
            .as_ref()
            .map_err(Clone::clone)
    }

    #[must_use]
    pub(crate) fn file(&self) -> &'static str {
        self.source.1
    }

    pub(crate) fn name(&self) -> &'static str {
        self.source.0
    }

    pub(crate) fn definition(&self) -> &ProfileData {
        self.data()
            .expect("validated inspection profile")
            .profiles
            .for_size(Size::default())
    }
}

pub(crate) static INSPECTION_PROFILES: [InspectionProfile; 1] = [InspectionProfile {
    source: game_table::profiles::wide_compact::SOURCE,
    data: OnceLock::new(),
}];

pub(crate) fn validate_inspection_profiles() -> Result<(), String> {
    INSPECTION_PROFILES
        .iter()
        .try_for_each(|profile| profile.data().map(|_| ()))
}

pub(crate) fn inspection_profile(name: &str) -> Result<&'static InspectionProfile, String> {
    let profile = INSPECTION_PROFILES
        .iter()
        .find(|profile| profile.name() == name)
        .ok_or_else(|| format!("unknown inspection profile {name:?}"))?;
    profile.data()?;
    Ok(profile)
}

/// Which windows of the test matrix a run of the interface tests plays.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Windows {
    Desktop,
    Phone,
    /// Run fixtures in the explicitly selected profile.
    Profile(&'static InspectionProfile),
    /// Both, in the usual order.
    All,
}

/// Each profile's first test window, in profile order.
pub(crate) fn first_windows() -> Vec<Window> {
    profiles()
        .iter()
        .filter_map(|profile| profile.test_windows().next())
        .collect()
}

impl Windows {
    /// The windows, in the order tests are named in.
    #[must_use]
    pub fn windows(self) -> Vec<Window> {
        let classes: &[Layout] = match self {
            Self::Desktop => &[Layout::Desktop],
            Self::Phone => &[Layout::Phone],
            Self::All => &[Layout::Desktop, Layout::Phone],
            Self::Profile(profile) => return profile.definition().test_windows().collect(),
        };
        classes
            .iter()
            .flat_map(|&layout| {
                profiles()
                    .iter()
                    .filter(move |profile| profile.size_class == layout.size_class())
            })
            .flat_map(ProfileData::test_windows)
            .collect()
    }
}

/// A page for a recording found by name.
pub trait Recorded: Sized {
    /// The recording `name` (a fixture or a stress board) at its start, as `seat` sees it in
    /// the layout's usual window: 1600 by 900 on the desktop, 393 by 759 on the phone.
    ///
    /// # Panics
    ///
    /// When there is no such recording for `seat`, or it cannot be read; each says why.
    fn fixture(name: &str, seat: Side, layout: Layout) -> Self;

    /// The spec moment `moment` of the card whose spec is `card` (its slug), as
    /// [`Recorded::fixture`] plays a fixture.
    ///
    /// # Panics
    ///
    /// As [`Recorded::fixture`].
    fn spec(card: &str, moment: &str, seat: Side, layout: Layout) -> Self;
}

impl Recorded for Page {
    fn fixture(name: &str, seat: Side, layout: Layout) -> Self {
        page(name, seat, layout, usual_window(layout)).unwrap_or_else(|why| panic!("{why}"))
    }

    fn spec(card: &str, moment: &str, seat: Side, layout: Layout) -> Self {
        Self::fixture(&format!("{card}/{moment}"), seat, layout)
    }
}

/// The recording `name` at its start, as `seat` sees it in a window of `size`, with the
/// layout's [`safe_area`].
///
/// # Errors
///
/// When there is no such recording for `seat`, or it cannot be read.
pub fn page(name: &str, seat: Side, layout: Layout, size: Size) -> Result<Page, String> {
    page_with_profile(name, seat, layout, size, None)
}

pub(crate) fn page_with_profile(
    name: &str,
    seat: Side,
    layout: Layout,
    size: Size,
    profile: Option<&InspectionProfile>,
) -> Result<Page, String> {
    page_with_messages(name, recording(name, seat)?, seat, layout, size, profile)
}

pub(crate) fn scene_data(
    layout: Layout,
    profile: Option<&InspectionProfile>,
) -> Result<(Arc<Data>, Insets), String> {
    let input = if let Some(profile) = profile {
        (
            Arc::clone(profile.data()?),
            profile.definition().test_safe_area(),
        )
    } else {
        let insets = if layout == Layout::Phone {
            safe_area(layout)
        } else {
            Insets::default()
        };
        (crate::data::checkout()?, insets)
    };
    Ok(input)
}

pub(crate) fn page_with_messages(
    name: &str,
    recording: Recording,
    seat: Side,
    layout: Layout,
    size: Size,
    profile: Option<&InspectionProfile>,
) -> Result<Page, String> {
    let (data, safe_area) = scene_data(layout, profile)?;
    let drive = Board::parse(name).map(Board::drive).unwrap_or_default();
    let delay = drive
        .rest_before_last
        .map(|frames| {
            let released = recording.views()?.len().saturating_sub(1);
            Ok::<_, String>((released, Duration(frames as u64 * TICK.0)))
        })
        .transpose()?;
    let mut page = Page::new(data, recording.into(), seat, layout, size)
        .map_err(|error| format!("{name}: {error}"))?
        .named(name);
    if safe_area != Insets::default() {
        // Giving a page its safe area sets its client up again.
        page = page.with_safe_area(safe_area);
    }
    if drive.pointer_over_hand {
        let moves = pointer_moves(page.frame());
        page.set_pointer_moves(moves);
    }
    // Desktop already shows the proposals. Replay the press on compact profiles so
    // the ordinary interface checks see the open panel alongside the cards and stack.
    if matches!(
        name,
        "screen/table-assists" | "story/game_manual/table-assists"
    ) && page.data().profiles.for_size(size).size_class == kit_scene::SizeClass::Compact
    {
        let toggle = page
            .try_find("button \"Show proposals\"")
            .map_err(|error| error.to_string())?;
        let rect = toggle.rect;
        let position = [rect.x + rect.width / 2.0, rect.y + rect.height / 2.0].into();
        let presses = [PointerPhase::Down, PointerPhase::Up].map(|phase| {
            (
                TICK.0,
                Pointer {
                    id: 0,
                    kind: PointerKind::Mouse,
                    phase,
                    position,
                    button: Button::Primary,
                    wheel: [0.0; 2].into(),
                    modifiers: Modifiers::default(),
                },
            )
        });
        page.set_pointer_moves(presses.to_vec());
    }
    if let Some((released, delay)) = delay {
        // The reply delay applies once every view but the last has been released.
        page.set_reply_delay(released, delay);
    }
    Ok(page)
}

/// The messages of the recording `name` as `seat` is sent them. A stress board is seen from
/// the first seat only.
///
/// # Errors
///
/// When there is no such recording for `seat`, or it could not be recorded.
pub fn recording(name: &str, seat: Side) -> Result<Recording, String> {
    let seat = match seat {
        Side::You => Seat::P0,
        Side::Opponent => Seat::P1,
    };
    if let Some(story_name) = stories::recording_story(name) {
        if seat != Seat::P0 {
            return Err(format!(
                "the recording {name:?} has one seat; use --seat you"
            ));
        }
        let story = stories::open(story_name)?;
        if let stories::Story::Game(story) = story.as_ref()
            && let Some(recording) = story_recording(&story.body)
        {
            return Ok(recording);
        }
        return Err(format!("{story_name} must hold a table or feature replay"));
    }
    recordings::find(name, seat).map(|messages| Recording(messages.to_vec()))
}

/// The driver inputs of a story that plays over time.
pub(crate) fn story_recording(body: &Body) -> Option<Recording> {
    let messages = match body {
        Body::Table(table) => table.messages(),
        Body::Replay(messages) => messages
            .iter()
            .map(|message| {
                // The testing screen lists the current pack. A capture can carry its own list.
                if let FixtureMessage::Event { event, payload } = message
                    && event == RECORDINGS
                    && payload.is_null()
                {
                    return FixtureMessage::Event {
                        event: event.clone(),
                        payload: recordings::listed().unwrap_or_default().into(),
                    };
                }
                message.clone()
            })
            .collect(),
        _ => return None,
    };
    Some(Recording(messages))
}

/// What [`check_recording`] played.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Checked {
    /// How many seat and window pairs it played.
    pub plays: usize,
    /// How many frames it checked, over all of them.
    pub frames: u64,
    /// How many pairs of consecutive frames "lines hold" compared, each sharing an arrow.
    pub line_pairs: u64,
}

/// Plays the recording `name` from each seat it is recorded for, in each of
/// the regular test windows, from its start to its end on its own clock, and runs every interface
/// check on the frame of every tick, and "lines hold" on each pair of consecutive frames. A
/// fault of a rule that `data/beryl3d/test-faults.json` lists for that seat and window is
/// allowed; `just b3d test` holds those to their counts.
///
/// # Panics
///
/// When a check finds a fault of a rule not listed, with the faults, the seat, the window and
/// the clock; or when the recording is recorded for neither seat, or the list cannot be read.
#[must_use]
pub fn check_recording(name: &str) -> Checked {
    let seats: Vec<Side> = [Side::You, Side::Opponent]
        .into_iter()
        .filter(|&seat| recording(name, seat).is_ok())
        .collect();
    if seats.is_empty() {
        let why = recording(name, Side::You).err().unwrap_or_default();
        panic!("{why}");
    }
    let listed = Listed::read().unwrap_or_else(|why| panic!("{why}"));
    let mut checked = Checked {
        plays: 0,
        frames: 0,
        line_pairs: 0,
    };
    for &seat in &seats {
        for window in by_layout(Layout::Desktop).test_windows() {
            let test = Test {
                recording: name.to_owned(),
                seat,
                size: window.size,
                scale: window.scale,
                profile: None,
            };
            let allowed = listed.tests.get(&test.name()).cloned().unwrap_or_default();
            let mut page = test.page().unwrap_or_else(|why| panic!("{why}"));
            // The arrows of the frame before, for "lines hold".
            let mut kept = Vec::new();
            loop {
                let held =
                    page.check_since(&mut kept, |fault| allowed.contains_key(fault.rule.word()));
                checked.frames += 1;
                checked.line_pairs += u64::from(held);
                if page.ended() {
                    break;
                }
                page.step(TICK);
            }
            checked.plays += 1;
        }
    }
    checked
}

/// The window a layout is usually shown in.
fn usual_window(layout: Layout) -> Size {
    by_layout(layout).usual_window().size
}
