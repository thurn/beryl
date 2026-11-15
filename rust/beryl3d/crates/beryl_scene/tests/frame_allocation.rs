//! `Client::frame` allocates nothing once a prompt bar has been laid out, nor while a life total
//! falls and the amount it fell by sinks away, and neither does moving a float of the flat
//! layer and writing it (docs/beryl-3d.md, "The frame": no allocation). This binary counts
//! every heap allocation, so it holds one test.

mod common;

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

use beryl_protocol::GameUpdate;
use beryl_protocol::prompt::Purpose;
use beryl_protocol::prompt::{ButtonId, PartName, PromptButton, PromptView, ShapeBody, Words};
use beryl_protocol::protocol::{DecisionId, DecisionView, Kind, SessionId, Side, Status};
use beryl_protocol::script::{Act, Tally};
use beryl_scene::element::{floats, text};
use beryl_scene::frame::{Frame, FrameBuilder};
use beryl_scene::input::{AssetReady, GlyphCell, Input, Insets, Size};
use beryl_scene::layout::{FloatLook, Layer};
use beryl_scene::table::Table;
use beryl_scene::testing::ViewBuilder;
use beryl_scene::{Client, Config};
use glam::Vec2;

/// The system allocator, counting the allocations it makes.
struct Counting;

static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);

#[expect(unsafe_code, reason = "a global allocator is an unsafe trait")]
// SAFETY: every call is passed straight to the system allocator.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        // SAFETY: the caller upholds `alloc`'s contract, which is the system allocator's.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        // SAFETY: `pointer` came from `alloc` above, so from the system allocator.
        unsafe { System.dealloc(pointer, layout) }
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        // SAFETY: as for `dealloc`, and the caller upholds `realloc`'s contract.
        unsafe { System.realloc(pointer, layout, size) }
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

fn deciding() -> Input {
    let button = |id, label: &str| PromptButton {
        id,
        label: label.to_owned(),
        caption: String::new(),
        enabled: true,
        primary: false,
    };
    let prompt = PromptView {
        revision: 0,
        purpose: Purpose::Mana,
        part: PartName::Pick,
        title: "Pay {1}{U}".to_owned(),
        words: Words::default(),
        count: None,
        about: Vec::new(),
        from: None,
        marks: Vec::new(),
        previews: Vec::new(),
        buttons: vec![
            button(ButtonId::Auto, "Auto Pay"),
            button(ButtonId::Decline, "Cancel"),
        ],
        body: ShapeBody::Candidates,
    };
    let mut view = ViewBuilder::sample().build();
    view.status = Status::Deciding {
        decision: DecisionView {
            id: DecisionId {
                session: SessionId(1),
                seq: 1,
            },
            kind: Kind::ManaPayment,
            candidates: Vec::new(),
            resolving: None,
            prompt: Box::new(prompt),
            pass_to: None,
            pass_on: None,
            source: None,
            halt: false,
            cancellable: true,
            undo: 0,
            undo_depth: 0,
            refused: Vec::new(),
            acting_for: None,
        },
    };
    Input::Update(Box::new(GameUpdate {
        session: SessionId(1),
        view,
    }))
}

#[test]
fn a_frame_with_a_prompt_bar_allocates_nothing() {
    let mut client = Client::new(Config {
        elements: 256,
        draws: 1024,
        name_bytes: 4096,
    });
    client.handle(Input::Surface {
        size: Size {
            width: 1440.0,
            height: 900.0,
        },
        scale: 2.0,
        safe_area: Insets::default(),
    });
    client.handle(deciding());
    let glyphs: Vec<GlyphCell> = client
        .wanted_glyphs()
        .map(|key| GlyphCell {
            key,
            rect: [0, 0, 20, 24],
            bearing: [0, -20],
        })
        .collect();
    client.handle(Input::Assets(AssetReady::Glyphs { page: 0, glyphs }));
    let before = ALLOCATIONS.load(Ordering::Relaxed);
    let records = client.frame(0).draws.len();
    let after = ALLOCATIONS.load(Ordering::Relaxed);
    assert!(records > 3, "the frame holds the bar's records");
    assert_eq!(after - before, 0, "allocations during Client::frame");

    // The frames of a step that takes the viewer's life from 20 to 17: the one that begins it,
    // which shows the new total and the loss, and those that sink and fade the loss.
    let (mut client, steps) = common::counting(vec![Act::Count {
        tally: Tally::Life {
            player: Side::You,
            life: 17,
        },
    }]);
    let (from, to) = steps[0];
    for now in [from, from + (to - from) / 2, to - 1, to] {
        let before = ALLOCATIONS.load(Ordering::Relaxed);
        client.frame(now);
        let after = ALLOCATIONS.load(Ordering::Relaxed);
        assert_eq!(
            after - before,
            0,
            "allocations in the frame at {now} during a loss"
        );
    }

    // A float moved and faded by a new look, then written into a frame.
    let mut layer = Layer::new();
    layer.float(floats::LIFE[0], Some(text("20")));
    let mut frame = Frame::with_capacity(64, 256, 1024);
    let before = ALLOCATIONS.load(Ordering::Relaxed);
    layer.set_float_look(
        floats::LIFE[0],
        FloatLook {
            point: Vec2::new(100.0, 50.0),
            anchor: Vec2::new(0.5, 0.5),
            opacity: 0.5,
            tint: Some([255, 0, 0, 255]),
        },
    );
    let mut builder = FrameBuilder::new(&mut frame);
    layer.write(&mut builder, Table::default().camera(), false);
    let elements = builder.finish().resolved.len();
    let after = ALLOCATIONS.load(Ordering::Relaxed);
    assert_eq!(elements, 1, "the frame holds the float");
    assert_eq!(
        after - before,
        0,
        "allocations setting a float's look and writing it"
    );
}
