//! Plays a gallery scene on its own clock and in its authored window. The inset uses the same
//! frame format and atlas as the outer window.

use std::sync::Arc;

use app_screens::screens::{Screen, gallery};
use game_probe::story::{Body, Story};
use glam::Vec2;
use kit_base::atlas::{AssetReady, GlyphKey};
use kit_base::data::Data;
use kit_base::geometry::{Rect, Size, Window};
use kit_frame::frame::{Frame, Inset};
use kit_probe::story::{Header, Player, parse_header};

use crate::driver::{Duration, Moment, Page, Recording, settings_element};
use crate::stories::Source;

use super::Client;

#[derive(Default)]
pub(super) struct Stories {
    sources: Arc<[Source]>,
    atlas: Vec<AssetReady>,
    selected: Option<String>,
    player: Option<Playback>,
    started: Option<u64>,
    dirty: bool,
    pub(super) generation: u64,
}

enum Playback {
    Flat(Box<Player>),
    Table { page: Box<Page>, plays: bool },
}

impl Playback {
    fn new(source: &Source, original: &Arc<Data>, scale: f32) -> Result<(Self, String), String> {
        let (kind, header) = source
            .text
            .lines()
            .next()
            .and_then(parse_header)
            .ok_or("a story needs a header")?;
        if !matches!(kind, "table" | "feature") {
            let story = source
                .text
                .parse::<kit_probe::story::Story>()
                .map_err(|why| why.to_string())?;
            let (data, window, profile) = story_window(header, None, original, scale)?;
            return Ok((
                Self::Flat(Box::new(Player::new(&story, window, data)?)),
                profile,
            ));
        }
        let story = Story::parse(&source.text)?;
        let (data, window, profile) =
            story_window(story.header, story.profile.as_deref(), original, scale)?;
        let base = |messages, seat| {
            Page::new(
                Arc::clone(&data),
                Arc::new(Recording(messages)),
                seat,
                window.size,
            )
            .map(|page| {
                page.with_host_atlas()
                    .with_scale(scale)
                    .with_safe_area(window.safe_area)
            })
            .map_err(|why| why.to_string())
        };
        let player = match story.body {
            Body::Settings(settings) => {
                let element = settings_element(&settings, window, &data);
                Self::Flat(Box::new(Player::from_element(element, window, data)))
            }
            Body::Replay(messages) => {
                let page = base(messages, beryl_protocol::protocol::Side::You)?.named(&source.name);
                Self::recorded(page, None, window, data, true)?
            }
            Body::Capture { feature, capture } => {
                let mut page = base(capture.messages, capture.seat)?
                    .named(&capture.source)
                    .with_catch_up(capture.catch_up);
                if let Some(stories) = capture.story_catalog {
                    page = page.with_stories(stories);
                }
                page.seek(&Moment::At(Duration(capture.at)))
                    .map_err(|why| why.to_string())?;
                page.story_actions(&capture.actions)?;
                Self::recorded(page, feature.as_deref(), window, data, source.plays)?
            }
            #[cfg(feature = "stories")]
            Body::Table(table) => {
                let plays = table.views.len() > 1 && table.actions.is_empty();
                let mut page = base(table.messages(), beryl_protocol::protocol::Side::You)?;
                page.story_actions(&table.actions)?;
                Self::recorded(page, table.feature.as_deref(), window, data, plays)?
            }
        };
        Ok((player, profile))
    }

    fn recorded(
        mut page: Page,
        feature: Option<&str>,
        window: Window,
        data: Arc<Data>,
        plays: bool,
    ) -> Result<Self, String> {
        if let Some(id) = feature {
            let element = page
                .feature_element(id)
                .ok_or_else(|| format!("feature {id:?} has no standalone view"))?;
            Ok(Self::Flat(Box::new(Player::from_element(
                element, window, data,
            ))))
        } else {
            Ok(Self::Table {
                page: Box::new(page),
                plays,
            })
        }
    }

    fn give(&mut self, asset: AssetReady) {
        match self {
            Self::Flat(player) => player.give(asset),
            Self::Table { page, .. } => match asset {
                AssetReady::Glyphs { .. } | AssetReady::Icons { .. } => page.give_glyphs(asset),
                AssetReady::Face { .. } => page.give_face(asset),
            },
        }
    }

    fn step(&mut self, at: u64) {
        match self {
            Self::Flat(player) => player.seek(at),
            Self::Table { page, plays } => {
                if *plays && !page.ended() && at > page.now() {
                    page.step(Duration(at - page.now()));
                }
            }
        }
    }

    fn moving(&self) -> bool {
        match self {
            Self::Flat(player) => player.now() < player.end(),
            Self::Table { page, plays } => *plays && !page.ended(),
        }
    }

    fn timed(&self) -> bool {
        match self {
            Self::Flat(player) => player.end() > 0,
            Self::Table { plays, .. } => *plays,
        }
    }

    fn frame(&mut self) -> &Frame {
        match self {
            Self::Flat(player) => player.frame(),
            Self::Table { page, .. } => page.frame(),
        }
    }
}

fn story_window(
    header: Header,
    profile: Option<&str>,
    original: &Arc<Data>,
    scale: f32,
) -> Result<(Arc<Data>, Window, String), String> {
    let mut data = Arc::clone(original);
    if let Some(profile) = profile {
        if let Some(profiles) = original.profiles.selecting(profile) {
            Arc::make_mut(&mut data).profiles = profiles;
        } else {
            let (name, file) = game_table::profiles::wide_compact::SOURCE;
            let text = include_str!("../../../../../../data/beryl3d/profiles/wide-compact.json");
            if profile != name {
                return Err(format!("unknown story profile {profile:?}"));
            }
            let data = Arc::make_mut(&mut data);
            data.profiles = data.profiles.clone().with_preview(file, text, data.card)?;
        }
    } else {
        let current = original.profiles.for_size(header.window);
        let size_class = header.size_class;
        if current.size_class != size_class || original.profiles.preview().is_some() {
            let profile = original.profiles.for_size_class(size_class);
            Arc::make_mut(&mut data).profiles = original
                .profiles
                .selecting(&profile.name)
                .expect("the profile comes from the list");
        }
    }
    let layout = data.profiles.for_size(header.window);
    let window = Window {
        size: header.window,
        scale,
        safe_area: layout.test_safe_area(),
        size_class: Some(layout.size_class),
    };
    let name = layout.name.clone();
    Ok((data, window, name))
}

impl Stories {
    pub(super) fn wanted_glyphs(&mut self) -> Vec<GlyphKey> {
        match self.player.as_mut() {
            Some(Playback::Flat(player)) => player.wanted_glyphs(),
            Some(Playback::Table { page, .. }) => page.wanted_glyphs(),
            None => Vec::new(),
        }
    }

    pub(super) fn asset(&mut self, asset: &AssetReady) {
        if !self.sources.is_empty()
            && matches!(asset, AssetReady::Glyphs { .. } | AssetReady::Icons { .. })
        {
            self.atlas.push(asset.clone());
            self.give(asset.clone());
        }
    }

    pub(super) fn give(&mut self, asset: AssetReady) {
        if let Some(player) = &mut self.player {
            player.give(asset);
            self.dirty = true;
        }
    }

    pub(super) fn board(&self) -> Option<&Page> {
        match self.player.as_ref()? {
            Playback::Table { page, .. } => Some(page),
            Playback::Flat(_) => None,
        }
    }

    pub(super) fn board_mut(&mut self) -> Option<&mut Page> {
        match self.player.as_mut()? {
            Playback::Table { page, .. } => Some(page),
            Playback::Flat(_) => None,
        }
    }
}

impl Client {
    pub(super) fn set_stories(&mut self, sources: Arc<[Source]>) {
        let mut catalog: Vec<_> = sources
            .iter()
            .map(|source| gallery::Story {
                name: source.name.to_string(),
                feature: source.feature.as_deref().map(str::to_owned),
            })
            .collect();
        catalog.sort_unstable_by(|a, b| {
            let (a_package, _) = a.name.rsplit_once('/').unwrap_or_default();
            let (b_package, _) = b.name.rsplit_once('/').unwrap_or_default();
            (a_package != "story/kit_ui", a_package, &a.feature, &a.name).cmp(&(
                b_package != "story/kit_ui",
                b_package,
                &b.feature,
                &b.name,
            ))
        });
        catalog.dedup_by(|a, b| a.name == b.name);
        let names: Arc<[String]> = catalog.iter().map(|story| story.name.clone()).collect();
        self.stories.sources = sources;
        self.update_gallery(&gallery::Message::Stories(catalog.into()));
        self.update_testing(&app_screens::screens::testing::Message::Stories(names));
        if matches!(self.features.screen, Screen::Gallery | Screen::Testing) {
            self.show_screen();
        }
    }

    pub(super) fn select_story(&mut self, name: &str) {
        self.stories.selected = Some(name.to_owned());
        self.stories.started = None;
        self.stories.generation += 1;
        self.frame.inset = None;
        let Some(source) = self
            .stories
            .sources
            .iter()
            .find(|source| source.name == name)
        else {
            self.stories.player = None;
            return;
        };
        match Playback::new(source, self.data(), self.window.scale) {
            Ok((mut player, profile)) => {
                for asset in &self.stories.atlas {
                    player.give(asset.clone());
                }
                let timed = player.timed();
                let size = parse_header(source.text.lines().next().unwrap_or_default())
                    .map_or(Size::default(), |(_, header)| header.window);
                self.frame.inset = Some(Box::new(Inset {
                    frame: Frame::with_capacity(0, 0, 0),
                    size,
                    scale: self.window.scale,
                    viewport: Rect::default(),
                }));
                self.stories.player = Some(player);
                self.stories.dirty = true;
                self.update_gallery(&gallery::Message::Details {
                    text: format!("{profile} · {} × {}", size.width, size.height),
                    timed,
                });
            }
            Err(why) => {
                self.stories.player = None;
                self.update_gallery(&gallery::Message::Details {
                    text: why,
                    timed: false,
                });
            }
        }
    }

    pub(super) fn restart_story(&mut self) {
        if let Some(name) = self.stories.selected.clone() {
            self.select_story(&name);
        }
    }

    fn story_visible(&self) -> bool {
        self.features.screen == Screen::Gallery && self.features.sheet.is_none()
    }

    pub(super) fn story_moving(&self) -> bool {
        self.story_visible()
            && (self.stories.dirty || self.stories.player.as_ref().is_some_and(Playback::moving))
    }

    pub(crate) fn prepare_story(&mut self, now: u64) {
        if !self.story_visible() {
            return;
        }
        if let (Some(player), Some(inset)) = (&mut self.stories.player, &mut self.frame.inset) {
            let at = now.saturating_sub(*self.stories.started.get_or_insert(now));
            if player.moving() {
                player.step(at);
                self.stories.dirty = true;
            }
            if std::mem::take(&mut self.stories.dirty) {
                inset.frame.clone_from(player.frame());
            }
        }
    }

    pub(super) fn place_story(&mut self) {
        let pane = self
            .story_visible()
            .then(|| {
                self.frame
                    .resolved
                    .iter()
                    .rev()
                    .find(|element| self.frame.name(element.name) == gallery::PREVIEW)
                    .map(kit_scene::hit::bounds)
            })
            .flatten();
        if let Some(inset) = &mut self.frame.inset {
            inset.viewport = pane.map_or_else(Rect::default, |pane| {
                let logical = Vec2::new(inset.size.width, inset.size.height);
                let pane_size = Vec2::new(pane.width, pane.height);
                let fit = (pane_size / logical).min_element().min(1.0);
                let size = logical * fit;
                let position = Vec2::new(pane.x, pane.y) + (pane_size - size) / 2.0;
                let scale = self.window.scale;
                Rect {
                    x: position.x * scale,
                    y: position.y * scale,
                    width: size.x * scale,
                    height: size.y * scale,
                }
            });
        }
    }
}
