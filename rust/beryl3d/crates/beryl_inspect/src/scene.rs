//! The common scene operations used by the text and picture tools.

use std::sync::Arc;

use app_client::driver::{
    Action, Checked, DriverError, Duration, Element, Mode, Moment, Name, Page, Recording, TICK,
    settings_element,
};
use beryl_protocol::Seat;
use beryl_protocol::protocol::Side;
use game_probe::story::Body;
use kit_frame::frame::Frame;
use kit_probe::agent::{Context, Judged, Layout, checks, locate};
use kit_probe::story::Player;
use kit_scene::hit::bounds;
use kit_ui::element::Element as Widget;
use kit_ui::input::{AssetReady, GlyphKey, Size, Tier, Window};

use crate::recordings::Listed;
use crate::{pages, recordings, stories};

/// One scene opened in a particular window.
pub enum Scene {
    Recording(Box<Page>),
    Story(Box<StoryScene>),
}

enum Preview {
    Toolkit(Arc<stories::Story>),
    Feature(Box<Widget>),
}

impl Preview {
    fn player(&self, window: Window, data: Arc<kit_base::data::Data>) -> Result<Player, String> {
        match self {
            Self::Toolkit(source) => {
                let stories::Story::Toolkit(story) = source.as_ref() else {
                    unreachable!("this preview only holds toolkit stories");
                };
                Player::new(story, window, data)
            }
            Self::Feature(element) => {
                Ok(Player::from_element(Widget::clone(element), window, data))
            }
        }
    }
}

/// A toolkit or feature preview and its context for the interface tools.
pub struct StoryScene {
    source: Preview,
    player: Player,
    context: Context,
    scale: f32,
    tier: Tier,
}

impl Scene {
    /// Opens a scene from its name. The window and layout are selected by the caller.
    ///
    /// # Errors
    ///
    /// When the scene cannot be opened in the given seat.
    pub fn open(
        name: &str,
        seat: Side,
        layout: Layout,
        size: Size,
        scale: f32,
    ) -> Result<Self, String> {
        if !stories::is_story(name) {
            return pages::page(name, seat, layout, size)
                .map(|page| Self::Recording(Box::new(page.with_scale(scale))));
        }
        if seat != Side::You {
            return Err(format!("story {name:?} has one view; use --seat you"));
        }
        let source = stories::open(name)?;
        let profile = match source.as_ref() {
            stories::Story::Toolkit(_) => None,
            stories::Story::Game(story) => story
                .profile
                .as_deref()
                .map(pages::inspection_profile)
                .transpose()?,
        };
        let (data, safe_area) = pages::scene_data(layout, profile)?;
        let window = Window {
            size_class: None,
            size,
            scale,
            safe_area,
        };
        let source = match source.as_ref() {
            stories::Story::Toolkit(_) => Preview::Toolkit(source),
            stories::Story::Game(story) => match &story.body {
                Body::Table(table) => {
                    return pages::page_with_messages(
                        name,
                        Recording(table.messages()),
                        seat,
                        layout,
                        size,
                        profile,
                    )
                    .map(|page| Self::Recording(Box::new(page.with_scale(scale))));
                }
                Body::Settings(settings) => {
                    Preview::Feature(Box::new(settings_element(settings, window, &data)))
                }
                Body::Capture { feature, capture } => {
                    let mut page = pages::page_with_messages(
                        &capture.source,
                        Recording(capture.messages.clone()),
                        capture.seat,
                        layout,
                        size,
                        profile,
                    )?
                    .with_scale(scale)
                    .with_catch_up(capture.catch_up);
                    let moment = Moment::At(Duration(capture.at));
                    page.seek(&moment).map_err(|error| error.to_string())?;
                    if let Some(id) = feature {
                        let element = page.feature_element(id).ok_or_else(|| {
                            format!("feature {id:?} has no standalone view at this moment")
                        })?;
                        Preview::Feature(Box::new(element))
                    } else {
                        return Ok(Self::Recording(Box::new(
                            page.with_story_moment(capture.at),
                        )));
                    }
                }
            },
        };
        let mut player = source
            .player(window, data)
            .map_err(|error| format!("{name}: {error}"))?;
        player.stub_atlas();
        let audit = player.data().audit;
        Ok(Self::Story(Box::new(StoryScene {
            source,
            player,
            context: Context {
                recording: name.to_owned(),
                seat,
                layout,
                audit,
                checks: profile.map_or_else(
                    || pages::layout_checks(layout, size),
                    |profile| profile.definition().checks,
                ),
                size,
                safe_area,
                at: 0,
            },
            scale,
            tier: Tier::Baseline,
        })))
    }

    pub fn frame(&mut self) -> &Frame {
        match self {
            Self::Recording(page) => page.frame(),
            Self::Story(story) => story.player.frame(),
        }
    }

    pub fn size(&self) -> Size {
        match self {
            Self::Recording(page) => page.size(),
            Self::Story(story) => story.context.size,
        }
    }

    pub fn scale(&self) -> f32 {
        match self {
            Self::Recording(page) => page.scale(),
            Self::Story(story) => story.scale,
        }
    }

    /// The tuning the scene's client or story player was made with.
    #[must_use]
    pub fn data(&self) -> &std::sync::Arc<kit_base::data::Data> {
        match self {
            Self::Recording(page) => page.data(),
            Self::Story(story) => story.player.data(),
        }
    }

    pub fn now(&self) -> u64 {
        match self {
            Self::Recording(page) => page.now(),
            Self::Story(story) => story.player.now(),
        }
    }

    pub fn step(&mut self, elapsed: Duration) {
        match self {
            Self::Recording(page) => page.step(elapsed),
            Self::Story(story) => story
                .player
                .seek(story.player.now().saturating_add(elapsed.0)),
        }
    }

    pub fn time_of(&mut self, moment: &Moment) -> Result<u64, DriverError> {
        match self {
            Self::Recording(page) => match (page.story_moment(), moment) {
                (
                    Some(time),
                    Moment::Named {
                        name: Name::Start | Name::End,
                        offset,
                    },
                ) => {
                    if offset.length.0 != 0 {
                        return Err(DriverError(
                            "a captured story has a single moment".to_owned(),
                        ));
                    }
                    Ok(time)
                }
                (Some(time), Moment::At(at)) if time == at.0 => Ok(time),
                (Some(_), _) => Err(DriverError(
                    "a captured story has only its saved instant; use --at start or --at end"
                        .to_owned(),
                )),
                _ => page.time_of(moment),
            },
            Self::Story(story) => match *moment {
                Moment::At(at) => Ok(at.0),
                Moment::Named { name, offset } => {
                    let time = match name {
                        Name::Start => 0,
                        Name::End => story.player.end(),
                        _ => {
                            return Err(DriverError(format!(
                                "a preview story has start, end and times in between; write one of those instead of {name}"
                            )));
                        }
                    };
                    if offset.before {
                        time.checked_sub(offset.length.0).ok_or_else(|| {
                            DriverError(format!("{moment} is before the story starts"))
                        })
                    } else {
                        Ok(time.saturating_add(offset.length.0))
                    }
                }
            },
        }
    }

    pub fn seek(&mut self, moment: &Moment) -> Result<(), DriverError> {
        let time = self.time_of(moment)?;
        match self {
            Self::Recording(page) => page.seek(&Moment::At(Duration(time))),
            Self::Story(story) => {
                story.player.seek(time);
                Ok(())
            }
        }
    }

    pub fn moments(&mut self) -> Vec<(Name, Duration)> {
        match self {
            Self::Recording(page) => page.story_moment().map_or_else(
                || page.moments(),
                |time| vec![(Name::Start, Duration(time)), (Name::End, Duration(time))],
            ),
            Self::Story(story) => vec![
                (Name::Start, Duration(0)),
                (Name::End, Duration(story.player.end())),
            ],
        }
    }

    pub fn check_times(&mut self) -> Vec<u64> {
        match self {
            Self::Recording(page) => page
                .story_moment()
                .map_or_else(|| page.check_times(), |time| vec![time]),
            Self::Story(story) => times(story.player.end(), Mode::AtCheckTimes),
        }
    }

    pub fn check_all(&mut self, mode: Mode) -> Checked {
        match self {
            Self::Recording(page) => page.check_story().unwrap_or_else(|| page.check_all(mode)),
            Self::Story(story) => story.check(mode),
        }
    }

    pub fn set_looking(&mut self, looking: bool) {
        match self {
            Self::Recording(page) => page.set_looking(looking),
            Self::Story(story) => story.player.look(looking),
        }
    }

    pub fn try_find(&mut self, locator: &str) -> Result<Element, DriverError> {
        if let Self::Recording(page) = self {
            return page.try_find(locator);
        }
        let frame = self.frame();
        let index = locate(frame, locator).map_err(|why| DriverError(why.to_string()))?;
        let element = &frame.resolved[index as usize];
        Ok(Element {
            index,
            role: element.role,
            name: frame.name(element.name).to_owned(),
            place: element.place,
            rect: bounds(element),
            interactive: element.interactive,
            state: element.state,
        })
    }

    pub fn try_act(&mut self, action: &Action) -> Result<(), DriverError> {
        match self {
            Self::Recording(page) => page.try_act(action),
            Self::Story(story) => {
                let player = &mut story.player;
                match action {
                    Action::Hover(locator) => player.hover(locator).map_err(DriverError)?,
                    Action::Move(point) => player.point(*point),
                    Action::Step(elapsed) => player.seek(player.now().saturating_add(elapsed.0)),
                    Action::Still => player.seek(player.end()),
                    _ => {
                        return Err(DriverError(
                            "preview stories accept hover, move, step and still actions".to_owned(),
                        ));
                    }
                }
                Ok(())
            }
        }
    }

    /// A recording's released views and animation state. Toolkit scenes have no views.
    pub fn position(&mut self) -> (usize, usize, bool) {
        match self {
            Self::Recording(page) => page.position(),
            Self::Story(story) => (0, 0, story.player.now() < story.player.end()),
        }
    }

    /// When the recording's views were released. Toolkit scenes deliver no views.
    pub fn views_given_at(&self) -> Vec<u64> {
        match self {
            Self::Recording(page) => page.views_given_at(),
            Self::Story(_) => Vec::new(),
        }
    }

    /// When the recording's current view was released.
    pub fn view_given_at(&self) -> Option<u64> {
        match self {
            Self::Recording(page) => page.view_given_at(),
            Self::Story(_) => None,
        }
    }

    /// Reopens the atlas for real glyph and icon pages instead of the check driver's cells.
    #[must_use]
    pub fn with_host_atlas(self) -> Self {
        match self {
            Self::Recording(page) => Self::Recording(Box::new(page.with_host_atlas())),
            Self::Story(mut scene) => {
                // Recreate the layer so it requests even glyphs the stub atlas already supplied.
                scene.player = scene
                    .source
                    .player(
                        Window {
                            size_class: None,
                            size: scene.context.size,
                            scale: scene.scale,
                            safe_area: scene.context.safe_area,
                        },
                        scene.player.data().clone(),
                    )
                    .expect("this story has already opened with the same tuning");
                scene.player.set_tier(scene.tier);
                Self::Story(scene)
            }
        }
    }

    /// Supplies the visual tier to either kind of scene before drawing.
    #[must_use]
    pub fn with_tier(self, tier: Tier) -> Self {
        match self {
            Self::Recording(page) => Self::Recording(Box::new(page.with_tier(tier))),
            Self::Story(mut scene) => {
                scene.tier = tier;
                scene.player.set_tier(tier);
                Self::Story(scene)
            }
        }
    }

    pub fn wanted_glyphs(&mut self) -> Vec<GlyphKey> {
        match self {
            Self::Recording(page) => page.wanted_glyphs(),
            Self::Story(story) => story.player.wanted_glyphs(),
        }
    }

    /// The tuning a toolkit story is played with. A recording's tuning belongs to its client.
    #[must_use]
    pub fn story_data(&self) -> Option<&std::sync::Arc<kit_base::data::Data>> {
        match self {
            Self::Recording(_) => None,
            Self::Story(story) => Some(story.player.data()),
        }
    }

    pub fn give_glyphs(&mut self, ready: AssetReady) {
        match self {
            Self::Recording(page) => page.give_glyphs(ready),
            Self::Story(story) => story.player.give(ready),
        }
    }
}

impl StoryScene {
    fn check(&mut self, mode: Mode) -> Checked {
        let mut checked = Checked {
            faults: Vec::new(),
            frames: 0,
            line_pairs: 0,
            judged: Judged::default(),
        };
        let times = times(self.player.end(), mode);
        let mut kept = Vec::new();
        for time in times {
            self.player.seek(time);
            self.context.at = time;
            let frame = self.player.frame();
            checked.frames += 1;
            checked.judged.frame(frame, self.context.layout);
            checked.judged.lines(&kept, frame);
            for fault in checks::check(frame, &self.context)
                .into_iter()
                .chain(checks::lines_hold(&kept, frame, &self.context))
            {
                if !checked.faults.iter().any(|previous| {
                    previous.rule == fault.rule && previous.elements == fault.elements
                }) {
                    checked.faults.push(fault);
                }
            }
            checked.line_pairs += usize::from(checks::arrows_held(&kept, frame) != 0);
            checks::keep_arrows(frame, &mut kept);
        }
        checked
    }
}

fn times(end: u64, mode: Mode) -> Vec<u64> {
    if end == 0 {
        return vec![0];
    }
    match mode {
        Mode::AtCheckTimes => vec![0, end / 2, end],
        Mode::EveryTick => (0..end.div_ceil(TICK.0))
            .map(|tick| tick * TICK.0)
            .chain([end])
            .collect(),
    }
}

/// Whether only toolkit scene names can match a prefix.
fn story_prefix(prefix: &str) -> bool {
    prefix.starts_with(stories::PREFIX)
}

/// Candidate scene names for a prefix, without opening any story or playing a recording.
///
/// # Errors
///
/// When the catalog or the recordings pack cannot be read.
pub fn names(prefix: &str) -> Result<Vec<(String, Seat)>, String> {
    let mut names = if story_prefix(prefix) {
        Vec::new()
    } else {
        recordings::names()?
    };
    if story_prefix(prefix) || stories::PREFIX.starts_with(prefix) {
        names.extend(stories::names()?.map(|name| (name.to_owned(), Seat::P0)));
        names.sort_by(|a, b| (&a.0, a.1.get()).cmp(&(&b.0, b.1.get())));
    }
    Ok(names)
}

/// Scene listings matching a prefix, including the end of any effect story.
///
/// # Errors
///
/// When the catalog or the recordings pack cannot be read.
pub(crate) fn list(prefix: &str) -> Result<Vec<Listed>, String> {
    let mut listed = if story_prefix(prefix) {
        Vec::new()
    } else {
        recordings::list(prefix)?
    };
    if story_prefix(prefix) || stories::PREFIX.starts_with(prefix) {
        for name in stories::names()?.filter(|name| name.starts_with(prefix)) {
            let parsed = stories::open(name);
            listed.push(Listed {
                name: name.to_owned(),
                seat: Seat::P0,
                length_ms: parsed
                    .as_ref()
                    .ok()
                    .map(|story| story.duration() / 1_000_000),
                views: 0,
                failure: parsed.err(),
            });
        }
        listed.sort_by(|a, b| (&a.name, a.seat.get()).cmp(&(&b.name, b.seat.get())));
    }
    Ok(listed)
}
