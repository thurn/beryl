//! Style tracks are created with the new tree and sampled with the client's frame time.
//! The first frame after reconciliation stamps their start. Sampling leaves the layout alone.

use glam::{Vec2, Vec4};

use super::{Layer, Node};
use crate::element::{Action, Presence, Property, Style, Timing};
use crate::input::Rect;

/// One independently timed property. Opacity and layout slides are kept inline because
/// screens use them frequently. Other properties reserve storage only when first animated.
#[derive(Debug, Default)]
pub(super) struct Tracks {
    opacity: Option<Track>,
    placement: Option<Track>,
    other: Vec<(Property, Track)>,
}

#[derive(Clone, Copy, Debug)]
struct Track {
    from: Vec4,
    to: Vec4,
    timing: Timing,
    start: Option<u64>,
    progress: f32,
}

impl Track {
    fn new(from: Vec4, to: Vec4, timing: Timing) -> Option<Self> {
        (from != to && timing.nanoseconds() != (0, 0)).then_some(Self {
            from,
            to,
            timing,
            start: None,
            progress: 0.0,
        })
    }

    fn current(self) -> Vec4 {
        self.from + (self.to - self.from) * self.progress
    }

    /// Returns true until the track has reached its destination.
    fn advance(&mut self, now: u64, ticked: &mut Ticked) -> bool {
        let (delay, duration) = self.timing.nanoseconds();
        if self.start.is_none() {
            ticked.began = true;
            ticked.length = ticked.length.max(delay.saturating_add(duration));
        }
        let start = *self.start.get_or_insert(now);
        let elapsed = now.saturating_sub(start);
        if elapsed < delay {
            return true;
        }
        let elapsed = elapsed - delay;
        if elapsed >= duration {
            return false;
        }
        #[expect(
            clippy::cast_precision_loss,
            reason = "the fraction of a transition elapsed"
        )]
        let fraction = elapsed as f32 / duration as f32;
        self.progress = self.timing.curve.ease(fraction);
        true
    }
}

impl Tracks {
    pub(super) fn active_properties(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.opacity
            .iter()
            .map(|_| "opacity")
            .chain(self.placement.iter().map(|_| "layout position"))
            .chain(self.other.iter().map(|(property, _)| match property {
                Property::Opacity => "opacity",
                Property::Translation => "translation",
                Property::Scale => "scale",
                Property::Color => "color",
                Property::Background => "background",
                Property::Width => "width",
                Property::Height => "height",
            }))
    }

    pub(super) fn reserve_presence(&mut self, presence: &Presence) {
        self.other.reserve(
            usize::from(presence.translation.is_some()) + usize::from(presence.scale.is_some()),
        );
    }

    fn get(&self, property: Property) -> Option<Track> {
        if property == Property::Opacity {
            self.opacity
        } else {
            self.other
                .iter()
                .find_map(|&(key, track)| (key == property).then_some(track))
        }
    }

    fn set(&mut self, property: Property, next: Option<Track>) {
        if property == Property::Opacity {
            self.opacity = next;
        } else if let Some(index) = self.other.iter().position(|&(key, _)| key == property) {
            if let Some(track) = next {
                self.other[index].1 = track;
            } else {
                self.other.swap_remove(index);
            }
        } else if let Some(track) = next {
            self.other.push((property, track));
        }
    }

    fn advance(&mut self, now: u64, ticked: &mut Ticked) {
        for slot in [&mut self.opacity, &mut self.placement] {
            if slot
                .as_mut()
                .is_some_and(|track| !track.advance(now, ticked))
            {
                *slot = None;
            }
        }
        self.other
            .retain_mut(|(_, track)| track.advance(now, ticked));
        ticked.moving |=
            self.opacity.is_some() || self.placement.is_some() || !self.other.is_empty();
    }

    /// A departing node is done when the tracks declared by its leave have ended.
    fn leaving(&self, leave: &Presence) -> bool {
        (leave.opacity.is_some() && self.opacity.is_some())
            || (leave.translation.is_some() && self.get(Property::Translation).is_some())
            || (leave.scale.is_some() && self.get(Property::Scale).is_some())
    }
}

const LOOK_PROPERTIES: [Property; 5] = [
    Property::Opacity,
    Property::Translation,
    Property::Scale,
    Property::Color,
    Property::Background,
];

fn vector(value: Vec2) -> Vec4 {
    value.extend(0.0).extend(0.0)
}

fn color(value: [u8; 4]) -> Vec4 {
    Vec4::from_array(value.map(f32::from))
}

fn style_value(style: &Style, property: Property) -> Vec4 {
    match property {
        Property::Opacity => Vec4::splat(style.opacity),
        Property::Translation => vector(style.translation),
        Property::Scale => Vec4::splat(style.scale),
        Property::Color => color(style.color.unwrap_or(kit_base::tokens::color::TYPE)),
        Property::Background => color(style.background.unwrap_or([0; 4])),
        Property::Width => Vec4::splat(style.width.unwrap_or_default()),
        Property::Height => Vec4::splat(style.height.unwrap_or_default()),
    }
}

impl Node {
    fn property(&self, property: Property) -> Option<Vec4> {
        self.tracks
            .get(property)
            .filter(|track| track.start.is_some())
            .map(Track::current)
    }

    pub(super) fn offset(&self) -> Vec2 {
        self.tracks
            .placement
            .filter(|track| track.start.is_some())
            .map_or(Vec2::ZERO, |track| track.current().truncate().truncate())
    }

    pub(super) fn opacity(&self) -> f32 {
        self.property(Property::Opacity)
            .map_or_else(|| self.opacity_target(&self.element.style), |value| value.x)
    }

    fn opacity_target(&self, style: &Style) -> f32 {
        if self.hovering && style.opaque_on_hover.is_some() {
            1.0
        } else {
            style.opacity
        }
    }

    pub(super) fn set_hovering(&mut self, hovering: bool) {
        let style = &self.element.style;
        let old = Vec4::splat(self.opacity_target(style));
        self.hovering = hovering;
        let new = Vec4::splat(self.opacity_target(style));
        if old != new {
            self.change_track(Property::Opacity, old, new, style.opaque_on_hover);
        }
    }

    pub(super) fn translation(&self) -> Vec2 {
        self.property(Property::Translation)
            .map_or(self.element.style.translation, |value| {
                value.truncate().truncate()
            })
    }

    pub(super) fn scale(&self) -> f32 {
        self.property(Property::Scale)
            .map_or(self.element.style.scale, |value| value.x)
    }

    pub(super) fn paint_color(&self, property: Property) -> Option<[u8; 4]> {
        self.property(property).map(|value| {
            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "rounded color channels"
            )]
            let channels = value
                .to_array()
                .map(|channel| channel.round().clamp(0.0, f32::from(u8::MAX)) as u8);
            channels
        })
    }

    /// The border box as painted, with its interpolated dimensions.
    pub(super) fn paint_rect(&self) -> Rect {
        Rect {
            width: self
                .property(Property::Width)
                .map_or(self.rect.width, |value| value.x),
            height: self
                .property(Property::Height)
                .map_or(self.rect.height, |value| value.x),
            ..self.rect
        }
    }

    pub(super) fn unseen(&self) -> bool {
        self.tracks
            .opacity
            .is_some_and(|track| track.start.is_none() && track.from.x <= 0.0)
    }

    pub(super) fn slide_layout(&mut self, moved: Vec2, timing: Timing) {
        let from = self.tracks.placement.map_or(Vec4::ZERO, Track::current) + vector(moved);
        self.tracks.placement = Track::new(from, Vec4::ZERO, timing);
    }

    pub(super) fn stop_entrance_fade(&mut self) {
        if self.unseen() {
            self.tracks.opacity = None;
        }
    }

    pub(super) fn update_tracks(&mut self, next: &Style, anchored: bool, animate: bool) {
        let timing = |property| animate.then(|| timing(next, property, anchored)).flatten();
        for property in LOOK_PROPERTIES {
            let (old, new) = if property == Property::Opacity {
                (
                    Vec4::splat(self.opacity_target(&self.element.style)),
                    Vec4::splat(self.opacity_target(next)),
                )
            } else {
                (
                    style_value(&self.element.style, property),
                    style_value(next, property),
                )
            };
            if old != new {
                self.change_track(property, old, new, timing(property));
            }
        }
        for property in [Property::Width, Property::Height] {
            let (old, new) = match property {
                Property::Width => (self.element.style.width, next.width),
                _ => (self.element.style.height, next.height),
            };
            if old != new {
                // Layout supplies the resolved destination, including min/max constraints.
                if let Some(timing) = timing(property) {
                    let size = if property == Property::Width {
                        self.rect.width
                    } else {
                        self.rect.height
                    };
                    let from = self
                        .tracks
                        .get(property)
                        .map_or(Vec4::splat(size), Track::current);
                    self.tracks.set(
                        property,
                        Some(Track {
                            from,
                            to: from,
                            timing,
                            start: None,
                            progress: 0.0,
                        }),
                    );
                } else {
                    self.tracks.set(property, None);
                }
            }
        }
    }

    pub(super) fn finish_size_tracks(&mut self, size: Vec2) {
        for (property, value) in [(Property::Width, size.x), (Property::Height, size.y)] {
            if let Some(track) = self.tracks.get(property)
                && track.start.is_none()
            {
                let next = Vec4::splat(value);
                self.tracks
                    .set(property, Track::new(track.from, next, track.timing));
            }
        }
    }

    fn change_track(&mut self, property: Property, old: Vec4, new: Vec4, timing: Option<Timing>) {
        let from = self.tracks.get(property).map_or(old, Track::current);
        self.tracks.set(
            property,
            timing.and_then(|timing| Track::new(from, new, timing)),
        );
    }

    pub(super) fn leaving(&self) -> bool {
        self.element
            .style
            .leave
            .is_some_and(|leave| self.tracks.leaving(&leave))
    }

    /// Starts an enter or leave. A leave starts at the currently displayed values.
    pub(super) fn presence(&mut self, presence: Presence, entering: bool) {
        let values = [
            (Property::Opacity, presence.opacity.map(Vec4::splat)),
            (Property::Translation, presence.translation.map(vector)),
            (Property::Scale, presence.scale.map(Vec4::splat)),
        ];
        for (property, outside) in values {
            let Some(outside) = outside else { continue };
            let inside = style_value(&self.element.style, property);
            let (from, to) = if entering {
                (outside, inside)
            } else {
                (
                    self.tracks.get(property).map_or(inside, Track::current),
                    outside,
                )
            };
            self.tracks
                .set(property, Track::new(from, to, presence.timing));
        }
    }
}

fn timing(style: &Style, property: Property, anchored: bool) -> Option<Timing> {
    if matches!(property, Property::Width | Property::Height)
        && !anchored
        && style.position != crate::element::Position::Absolute
    {
        return None;
    }
    style
        .transition
        .iter()
        .find(|spec| spec.property == property)
        .map(|spec| spec.timing)
        .or_else(|| {
            style
                .opacity_transition
                .filter(|_| property == Property::Opacity)
        })
}

impl Layer {
    /// Changes before or during the first frame show at their final values.
    pub(super) fn started(&self) -> bool {
        self.ticks.is_some_and(|(first, latest)| latest != first)
    }

    /// Shakes every element that takes `press` across the screen by `across` logical points,
    /// or stops shaking with `None`. Lays out and allocates nothing.
    pub fn shake(&mut self, shaken: Option<(&Action, f32)>) {
        self.shaken = shaken.map(|(press, across)| (press.clone(), across));
    }

    /// Stamps new tracks and samples the current look. Lays out and allocates nothing.
    pub fn tick(&mut self, now: u64) -> Ticked {
        self.ticks = Some((self.ticks.map_or(now, |(first, _)| first), now));
        self.tick_tip(now);
        let mut ticked = Ticked::default();
        if let Some(node) = &mut self.screen {
            advance_node(node, now, &mut ticked);
        }
        advance_departures(&mut self.departing, now, &mut ticked);
        ticked
    }
}

fn advance_node(node: &mut Node, now: u64, ticked: &mut Ticked) {
    node.tracks.advance(now, ticked);
    for child in &mut node.children {
        advance_node(child, now, ticked);
    }
    advance_departures(&mut node.departing, now, ticked);
}

fn advance_departures(nodes: &mut Vec<Node>, now: u64, ticked: &mut Ticked) {
    nodes.retain_mut(|node| {
        let mut tracks = Ticked::default();
        node.tracks.advance(now, &mut tracks);
        if !node.leaving() {
            return false;
        }
        ticked.moving |= tracks.moving;
        ticked.began |= tracks.began;
        ticked.length = ticked.length.max(tracks.length);
        for child in &mut node.children {
            advance_node(child, now, ticked);
        }
        advance_departures(&mut node.departing, now, ticked);
        true
    });
}

/// What `Layer::tick` found.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Ticked {
    /// Whether a transition is still under way.
    pub moving: bool,
    /// Whether a transition began at this frame.
    pub began: bool,
    /// The longest time until a track started in this frame ends, in nanoseconds.
    pub length: u64,
}
