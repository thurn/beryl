//! Input and clipping of plain scrollable elements, without a game screen or a wall clock.

use glam::Vec2;
use kit_base::geometry::{Insets, Size, Window};
use kit_frame::frame::ElementId;
use kit_ui::element::{Anchor, Element, Overflow, column, row};
use kit_ui::input::{
    Button, Key, KeyCode, KeyPhase, Modifiers, Pointer, PointerKind, PointerPhase,
};
use kit_ui::layout::Layer;
use kit_ui::scroll::{ScrollGesture, ScrollInput};

const INSIDE: Vec2 = Vec2::new(10.0, 10.0);
const MS: u64 = 1_000_000;

fn screen(overflow: Overflow, content_height: f32) -> Element {
    let mut viewport = column()
        .name("viewport")
        .overflow(overflow)
        .width(100.0)
        .height(100.0)
        .child(
            row()
                .name("first row")
                .width(300.0)
                .shrink(0.0)
                .height(30.0)
                .on_press(1_u8),
        )
        .child(
            row()
                .name("remaining rows")
                .width(300.0)
                .shrink(0.0)
                .height(content_height - 30.0),
        );
    viewport.anchor = Some(Anchor::Point(Vec2::ZERO, Vec2::ZERO));
    viewport
}

struct Page {
    layer: Layer,
    gesture: ScrollGesture<ElementId>,
    viewport: ElementId,
}

impl Page {
    fn new(overflow: Overflow) -> Self {
        let mut layer = Layer::new();
        layer.resize(Window {
            size: Size {
                width: 800.0,
                height: 600.0,
            },
            scale: 1.0,
            safe_area: Insets::default(),
        });
        layer.show(Some(screen(overflow, 600.0)));
        let viewport = layer
            .scroller_at(INSIDE, true)
            .expect("a viewport at this point");
        Self {
            layer,
            gesture: ScrollGesture::default(),
            viewport,
        }
    }

    fn pointer(
        &mut self,
        kind: PointerKind,
        phase: PointerPhase,
        at: Vec2,
        wheel: Vec2,
        now: u64,
    ) -> bool {
        let pointer = Pointer {
            id: u8::from(kind == PointerKind::Touch),
            kind,
            phase,
            position: at,
            button: if phase == PointerPhase::Wheel {
                Button::None
            } else {
                Button::Primary
            },
            wheel,
            modifiers: Modifiers::default(),
        };
        let at =
            self.layer
                .scroll_target_at(at, true, (phase == PointerPhase::Wheel).then_some(wheel));
        let input = self
            .gesture
            .pointer(&pointer, now, at, |id| self.layer.scroll_viewport(id));
        match input {
            ScrollInput::Pass => false,
            ScrollInput::Claimed(update) => {
                if let Some((id, offset)) = update {
                    self.layer.set_scroll_offset(id, offset);
                }
                true
            }
        }
    }

    fn wheel(&mut self, delta: Vec2) -> bool {
        self.pointer(PointerKind::Mouse, PointerPhase::Wheel, INSIDE, delta, 0)
    }

    fn drag(&mut self, kind: PointerKind, phase: PointerPhase, y: f32, time: u64) -> bool {
        self.pointer(kind, phase, Vec2::new(INSIDE.x, y), Vec2::ZERO, time * MS)
    }

    fn tick(&mut self, time: u64) {
        if let Some((id, offset)) = self
            .gesture
            .advance(time * MS, |id| self.layer.scroll_viewport(id))
        {
            self.layer.set_scroll_offset(id, offset);
        }
    }

    fn key(&mut self, code: KeyCode) {
        let key = Key {
            code,
            phase: KeyPhase::Down,
            modifiers: Modifiers::default(),
        };
        let (id, offset) = self
            .layer
            .scroll_key(key, true)
            .expect("focus is inside the viewport");
        self.layer.set_scroll_offset(id, offset);
    }

    fn offset(&self) -> Vec2 {
        self.layer
            .scroll_offset(self.viewport)
            .expect("the viewport still exists")
    }
}

#[test]
fn wheel_and_mouse_drag_share_the_clamped_offset_and_clip_presses() {
    let mut page = Page::new(Overflow::ScrollY);
    assert!(page.layer.press_at(INSIDE, true).is_some());
    assert!(!page.wheel(Vec2::new(-60.0, 0.0)));
    assert!(page.wheel(Vec2::new(-50.0, -60.0)));
    assert_eq!(page.offset(), Vec2::new(0.0, 60.0));
    assert!(page.layer.press_at(INSIDE, true).is_none());

    assert!(!page.drag(PointerKind::Mouse, PointerPhase::Down, 10.0, 0));
    assert!(!page.drag(PointerKind::Mouse, PointerPhase::Move, 9.0, 16));
    assert!(page.drag(PointerKind::Mouse, PointerPhase::Move, -30.0, 32));
    assert!(page.drag(PointerKind::Mouse, PointerPhase::Up, -30.0, 48));
    assert_eq!(page.offset(), Vec2::new(0.0, 100.0));
    assert!(!page.gesture.moving(500 * MS));

    page.wheel(Vec2::new(0.0, -900.0));
    assert_eq!(page.offset(), Vec2::new(0.0, 500.0));
    page.wheel(Vec2::new(0.0, 900.0));
    assert_eq!(page.offset(), Vec2::ZERO);
    assert!(page.layer.press_at(INSIDE, true).is_some());
}

#[test]
fn a_focused_child_or_viewport_scrolls_by_keys_and_keeps_its_offset_on_rebuild() {
    let mut page = Page::new(Overflow::ScrollX);
    let (child, ..) = page.layer.press_at(INSIDE, true).expect("the first row");
    page.layer.focus(Some(child));
    page.key(KeyCode::ArrowRight);
    assert_eq!(page.offset(), Vec2::new(40.0, 0.0));
    page.key(KeyCode::PageDown);
    assert_eq!(page.offset(), Vec2::new(140.0, 0.0));

    page.layer.show(Some(screen(Overflow::ScrollX, 400.0)));
    assert_eq!(page.offset(), Vec2::new(140.0, 0.0));
    page.layer.focus(Some(page.viewport));
    page.key(KeyCode::End);
    assert_eq!(page.offset(), Vec2::new(200.0, 0.0));
    page.key(KeyCode::Home);
    assert_eq!(page.offset(), Vec2::ZERO);
}

#[test]
fn touch_momentum_uses_the_frame_clock_and_decelerates_to_rest() {
    let mut page = Page::new(Overflow::ScrollY);
    let touch = PointerKind::Touch;
    assert!(!page.drag(touch, PointerPhase::Down, 90.0, 0));
    assert!(page.drag(touch, PointerPhase::Move, 60.0, 30));
    assert!(page.drag(touch, PointerPhase::Up, 60.0, 30));
    let released = page.offset().y;
    assert_eq!(released.to_bits(), 30.0_f32.to_bits());
    page.tick(80);
    let early = page.offset().y;
    page.tick(130);
    let later = page.offset().y;
    assert!(early > released && later > early);
    assert!(
        early - released > later - early,
        "successive samples slow down"
    );
    page.tick(330);
    let rest = page.offset();
    assert!(rest.y > later);
    assert!(!page.gesture.moving(500 * MS));
    page.tick(390);
    assert_eq!(page.offset(), rest);
}

#[test]
fn momentum_stops_at_either_bound_and_a_held_finger_does_not_fling() {
    let mut page = Page::new(Overflow::ScrollY);
    let touch = PointerKind::Touch;
    for (offset, start, end, bound) in [(450.0, 90.0, 55.0, 500.0), (50.0, 25.0, 60.0, 0.0)] {
        page.layer
            .set_scroll_offset(page.viewport, Vec2::Y * offset);
        page.drag(touch, PointerPhase::Down, start, 0);
        page.drag(touch, PointerPhase::Move, end, 25);
        page.drag(touch, PointerPhase::Up, end, 25);
        page.tick(80);
        assert_eq!(page.offset(), Vec2::Y * bound);
        assert!(
            !page.gesture.moving(500 * MS),
            "the track ended when it hit the bound"
        );
    }

    page.drag(touch, PointerPhase::Down, 90.0, 100);
    page.drag(touch, PointerPhase::Move, 30.0, 150);
    page.drag(touch, PointerPhase::Up, 30.0, 300);
    assert!(
        !page.gesture.moving(500 * MS),
        "no momentum after pausing before release"
    );
    page.tick(450);
    assert_eq!(page.offset(), Vec2::new(0.0, 60.0));
}
