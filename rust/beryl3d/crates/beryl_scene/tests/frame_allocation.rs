//! `Client::frame` allocates nothing once a prompt bar has been laid out (docs/beryl-3d.md,
//! "The frame": no allocation). This binary counts every heap allocation, so it holds one test.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

use beryl_protocol::GameUpdate;
use beryl_protocol::prompt::Purpose;
use beryl_protocol::prompt::{ButtonId, PartName, PromptButton, PromptView, ShapeBody, Words};
use beryl_protocol::protocol::{DecisionId, DecisionView, Kind, SessionId, Status};
use beryl_scene::input::{AssetReady, GlyphCell, Input, Insets, Size};
use beryl_scene::testing::ViewBuilder;
use beryl_scene::{Client, Config};

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
}
