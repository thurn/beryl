//! Toolkit story playback through the same flat layer, frame builder and effects as the client.

use std::collections::{BTreeMap, BTreeSet};
use std::iter::Peekable;
use std::sync::Arc;

use glam::camera::rh::proj::directx;
use glam::{Affine3A, Vec2};
use kit_base::atlas::{GlyphKey, Tier};
use kit_base::data::{Data, effect::Effects};
use kit_base::geometry::{Rect, Window, plane::ScreenPlane};
use kit_frame::frame::{ElementId, Frame, FrameBuilder, NameRef, Place, Resolved, Role, StateBits};
use kit_fx::effects::{Effect, preview};
use kit_ui::element::{Element, Position, holder, text};
use kit_ui::input::AssetReady;
use kit_ui::layout::Layer;

use crate::agent::locate;
use crate::driver::atlas::{STUB_PAGE, stub_glyph, stub_icons};

use super::style::Property;
use super::{Body, Node, NodeState, Story};

/// A toolkit scene drawn on its own clock. It does no I/O and asks its host for text and icons.
pub struct Player {
    data: Arc<Data>,
    window: Window,
    layer: Layer,
    nodes: BTreeMap<ElementId, PreviewNode>,
    effect: Option<(Effect, Rect)>,
    frame: Frame,
    now: u64,
    tier: Tier,
    dirty: bool,
}

#[derive(Clone, Copy)]
struct PreviewNode {
    id: ElementId,
    states: StateBits,
    edits: bool,
}

impl Player {
    /// Lays out the story in the given window. The caller supplies the atlas, either through
    /// the host or with [`Self::stub_atlas`] for checks that only need draw records.
    #[must_use]
    pub fn new(story: &Story, window: Window, data: Arc<Data>) -> Self {
        let mut layer = Layer::new();
        layer.resize(window);
        let mut authored = vec![None];
        let mut canvas = kit_ui::widgets::screen::whole_window(window.size, holder().name("Story"));
        let mut effect = None;
        match &story.body {
            Body::Elements(nodes) => {
                canvas.children =
                    children(&mut nodes.iter().peekable(), 0, Vec2::ZERO, &mut authored);
            }
            Body::Container(container) => {
                // The arrangement solver has yet to land. Keep the input typed and show its
                // region, so the same stories can exercise the actual solver once available.
                let label = format!(
                    "{} container ({} items, solver pending)",
                    container.policy.word(),
                    container.items.len()
                );
                canvas.children.push(placed(
                    text(&label).padding(8.0).background([39, 49, 66, 255]),
                    container.region,
                    Vec2::ZERO,
                ));
                authored.push(None);
            }
            Body::Effect(kind, rect) => {
                effect = Some((*kind, *rect));
                preview::prepare_text(&data.effects, *kind, &mut layer);
            }
        }
        if effect.is_none() {
            layer.show(Some(canvas));
        }
        let mut used: BTreeSet<_> = authored.iter().flatten().map(|node| node.id.0).collect();
        let mut spare = (0..=u32::MAX).rev();
        let nodes: BTreeMap<_, _> = layer
            .element_ids()
            .into_iter()
            .zip(authored)
            .map(|(id, node)| {
                let preview = node.unwrap_or_else(|| PreviewNode {
                    id: ElementId(
                        spare
                            .find(|id| used.insert(*id))
                            .expect("a story cannot use every id"),
                    ),
                    states: StateBits::default(),
                    edits: false,
                });
                (id, preview)
            })
            .collect();
        if let Some((&id, _)) = nodes.iter().find(|(_, node)| node.edits) {
            layer.focus(Some(id));
        }
        Self {
            data,
            window,
            layer,
            nodes,
            effect,
            frame: Frame::with_capacity(32, 128, 256),
            now: 0,
            tier: Tier::Baseline,
            dirty: true,
        }
    }

    /// The tuning supplied by the host.
    #[must_use]
    pub fn data(&self) -> &Arc<Data> {
        &self.data
    }

    /// Delivers every requested glyph and the toolkit icons as placeholder cells for checks.
    pub fn stub_atlas(&mut self) {
        self.give(stub_icons(self.window.scale, []));
        let glyphs = self.wanted_glyphs().into_iter().map(stub_glyph).collect();
        self.give(AssetReady::Glyphs {
            page: STUB_PAGE,
            glyphs,
        });
    }

    /// The glyphs the toolkit needs from the host.
    pub fn wanted_glyphs(&mut self) -> Vec<GlyphKey> {
        self.layer.glyphs_mut().drain_wanted().collect()
    }

    /// Updates the atlas for subsequent frames.
    pub fn give(&mut self, ready: AssetReady) {
        match ready {
            AssetReady::Glyphs { page, glyphs } => self.layer.glyphs_mut().add(page, &glyphs),
            AssetReady::Icons { page, cells } => self.layer.icons_mut().add(page, &cells),
            _ => return,
        }
        self.dirty = true;
    }

    /// Chooses the renderer's visual tier for subsequent frames.
    pub fn set_tier(&mut self, tier: Tier) {
        self.tier = tier;
        self.dirty = true;
    }

    /// Whether the frame records each element's style and its draw records.
    pub fn look(&mut self, looking: bool) {
        self.frame.looking = looking;
        self.dirty = true;
    }

    /// A still story has a single moment. An effect plays from start to finish.
    #[must_use]
    pub fn end(&self) -> u64 {
        self.effect.map_or(0, |_| preview::DURATION)
    }

    /// Where the player is on its clock, in nanoseconds.
    #[must_use]
    pub fn now(&self) -> u64 {
        self.now
    }

    /// Sets the time, clamped to the end of the story.
    pub fn seek(&mut self, time: u64) {
        let now = time.min(self.end());
        self.dirty |= now != self.now;
        self.now = now;
    }

    /// Points at an element by its tree locator.
    ///
    /// # Errors
    ///
    /// When the locator does not identify exactly one element.
    pub fn hover(&mut self, locator: &str) -> Result<(), String> {
        if let Some(point) = crate::driver::point(locator) {
            self.point(point);
            return Ok(());
        }
        let frame = self.frame();
        let index = locate(frame, locator).map_err(|why| why.to_string())?;
        if kit_scene::hit::uncovered(frame, index).is_none() {
            return Err(format!(
                "no part of '{locator}' can be hovered: it is covered"
            ));
        }
        let id = frame.resolved[index as usize].id;
        self.aim(Some(id));
        Ok(())
    }

    /// Points at the element under a position in logical window points.
    pub fn point(&mut self, point: Vec2) {
        let frame = self.frame();
        let id = kit_scene::hit::hit(frame, point).map(|index| frame.resolved[index as usize].id);
        self.aim(id);
    }

    fn aim(&mut self, id: Option<ElementId>) {
        let id = id.and_then(|id| {
            self.nodes
                .iter()
                .find_map(|(kept, shown)| (shown.id == id).then_some(*kept))
        });
        self.layer.point_at(id, None);
        self.dirty = true;
    }

    /// Draws the current moment using the supplied glyphs and icons.
    pub fn frame(&mut self) -> &Frame {
        if self.dirty {
            let Window { size, .. } = self.window;
            let half = Vec2::new(size.width, size.height) / 2.0;
            let plane = ScreenPlane::new(Affine3A::IDENTITY, self.window, half.y, 16.0);
            let mut builder = FrameBuilder::new(&mut self.frame, self.tier);
            builder.camera(directx::orthographic(
                -half.x, half.x, -half.y, half.y, 0.1, 1024.0,
            ));
            if let Some((effect, rect)) = self.effect {
                write_effect(
                    &self.data.effects,
                    effect,
                    rect,
                    self.now,
                    &plane,
                    &mut builder,
                );
                preview::position_text(
                    &self.data.effects,
                    effect,
                    rect,
                    self.now,
                    self.tier,
                    &mut self.layer,
                );
            }
            self.layer.write_preview(&mut builder, &plane, &|id| {
                self.nodes
                    .get(&id)
                    .map_or_else(StateBits::default, |node| node.states)
            });
            builder.finish();
            for element in &mut self.frame.resolved {
                if let Some(node) = self.nodes.get(&element.id) {
                    element.id = node.id;
                }
            }
            self.dirty = false;
        }
        &self.frame
    }
}

fn children<'a>(
    nodes: &mut Peekable<impl Iterator<Item = &'a Node>>,
    depth: usize,
    origin: Vec2,
    authored: &mut Vec<Option<PreviewNode>>,
) -> Vec<Element> {
    let mut children = Vec::new();
    while nodes.peek().is_some_and(|node| node.depth == depth) {
        let Some(node) = nodes.next() else { break };
        let (mut element, states) = element(node, origin);
        authored.push(Some(PreviewNode {
            id: node.id,
            states,
            edits: element.field.is_some() && states.has(StateBits::FOCUSED),
        }));
        for child in &element.children {
            generated(child, authored);
        }
        // An absolute child's inset is measured from inside the parent's border.
        let border = element.style.border.map_or([0.0; 4], |border| border.width);
        let corner = Vec2::new(node.rect.x + border[3], node.rect.y + border[0]);
        element
            .children
            .extend(self::children(nodes, depth + 1, corner, authored));
        children.push(element);
    }
    children
}

fn generated(element: &Element, authored: &mut Vec<Option<PreviewNode>>) {
    authored.push(None);
    for child in &element.children {
        generated(child, authored);
    }
}

fn element(node: &Node, origin: Vec2) -> (Element, StateBits) {
    let mut bits = StateBits::default();
    let mut interactive = false;
    let mut opacity = 1.0;
    for state in &node.states {
        match *state {
            NodeState::Interactive => interactive = true,
            NodeState::Bit(bit) => bits.0 |= bit,
            NodeState::Faded(percent) => opacity = 1.0 - percent / 100.0,
        }
    }
    let name = node.name.as_deref().unwrap_or_default();
    let text_property = node.properties.iter().find_map(|property| match property {
        Property::Text(words) => Some(words.as_str()),
        _ => None,
    });
    let widget = node.properties.iter().find_map(|property| match property {
        Property::Widget(widget) => Some(*widget),
        _ => None,
    });
    let mut element = widget.map_or_else(
        || {
            let mut element = Element::new(node.role);
            element.text = text_property
                .or((node.role == Role::Text).then_some(name))
                .map(str::to_owned);
            element
        },
        |widget| widget.build(node, text_property.unwrap_or(name), bits),
    );
    element = element
        .name(name)
        .enabled(!bits.has(StateBits::DISABLED))
        .opacity(opacity);
    if interactive && element.press.is_none() && !element.focus {
        element = element.on_press(());
    }
    for property in &node.properties {
        property.apply(&mut element);
    }
    (placed(element, node.rect, origin), bits)
}

fn placed(mut element: Element, rect: Rect, origin: Vec2) -> Element {
    // The authored box is the border box; explicit child coordinates are in window points.
    element.style.min_width = None;
    element.style.min_height = None;
    element.style.max_width = None;
    element.style.max_height = None;
    element
        .width(rect.width)
        .height(rect.height)
        .position(Position::Absolute)
        .inset([Some(rect.y - origin.y), None, None, Some(rect.x - origin.x)])
}

fn write_effect(
    data: &Effects,
    effect: Effect,
    rect: Rect,
    now: u64,
    plane: &ScreenPlane,
    builder: &mut FrameBuilder<'_>,
) {
    let Rect {
        x,
        y,
        width,
        height,
    } = preview::position(data, effect, rect, now);
    builder.resolved(
        effect.name(),
        Resolved {
            id: ElementId(u32::MAX - 1),
            parent: u32::MAX,
            role: Role::Region,
            name: NameRef::default(),
            place: Place::Layer,
            pose: None,
            corners: [
                Vec2::new(x, y + height),
                Vec2::new(x + width, y + height),
                Vec2::new(x + width, y),
                Vec2::new(x, y),
            ],
            depth: 0.0,
            opacity: 1.0,
            clip: None,
            interactive: false,
            state: StateBits(if now < preview::DURATION && preview::moves(data, effect) {
                StateBits::MOVING
            } else {
                0
            }),
            face: None,
            copies: 1,
            really: NameRef::default(),
            note: NameRef::default(),
        },
    );
    preview::write(data, effect, rect, now, plane, builder);
}
