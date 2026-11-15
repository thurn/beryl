//! `Client::frame` allocates nothing once a prompt bar has been laid out, nor while a life total
//! falls and the amount it fell by sinks away, nor while 255 marks are drawn, nor with 255
//! attackers wearing their wedges, nor on a table of 255 permanents that each wear badges, and
//! neither does moving a float of the flat layer and writing it (docs/beryl-3d.md, "The frame":
//! no allocation). This binary counts every heap allocation, so it holds one test.

mod common;

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

use beryl_protocol::GameUpdate;
use beryl_protocol::prompt::Purpose;
use beryl_protocol::prompt::{ButtonId, PartName, PromptButton, PromptView, ShapeBody, Words};
use beryl_protocol::protocol::{
    CardType, Combat, Counter, CounterView, DecisionId, DecisionView, Kind, ObjectId,
    PowerToughness, SessionId, Side, Status, ZoneName,
};
use beryl_protocol::script::{Act, Mark, Subject, Tally};
use beryl_scene::element::{floats, text};
use beryl_scene::frame::{Frame, FrameBuilder, Material, Role};
use beryl_scene::input::{AssetReady, GlyphCell, Input, Insets, Size};
use beryl_scene::layout::{FloatLook, Layer};
use beryl_scene::light::{MarkSpan, Span};
use beryl_scene::table::Table;
use beryl_scene::testing::{ScriptBuilder, ViewBuilder};
use beryl_scene::verbs::Interval;
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

    // A frame with 255 marks: outlines on cards and players, and tap symbols, both coming up
    // and standing whole.
    let marks = [
        Mark::Eligible,
        Mark::Selected,
        Mark::Paying,
        Mark::HostileTarget,
    ];
    for index in 0..255_u16 {
        let on = if index % 8 == 7 {
            Subject::Player { player: Side::You }
        } else {
            Subject::Object {
                slot: ObjectId(index % 8),
            }
        };
        let mark = marks[usize::from(index) % marks.len()];
        client.lit_mut().begin(
            Span::Mark(MarkSpan { mark, on }),
            Interval {
                from: to,
                to: to + 100_000_000,
            },
        );
    }
    for now in [to + 50_000_000, to + 200_000_000] {
        let before = ALLOCATIONS.load(Ordering::Relaxed);
        client.frame(now);
        let after = ALLOCATIONS.load(Ordering::Relaxed);
        assert_eq!(after - before, 0, "allocations in a frame with 255 marks");
    }

    a_frame_of_255_attackers_allocates_nothing();

    a_badged_table_allocates_nothing();

    // A float moved and faded by a new look, then written into a frame.
    let mut layer = Layer::new();
    layer.float(floats::LIFE[0], Some(text("20")));
    let mut frame = Frame::with_capacity(64, 256, 1024);
    let table = Table::default();
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
    layer.write(&mut builder, table.camera(), false);
    let elements = builder.finish().resolved.len();
    let after = ALLOCATIONS.load(Ordering::Relaxed);
    assert_eq!(elements, 1, "the frame holds the float");
    assert_eq!(
        after - before,
        0,
        "allocations setting a float's look and writing it"
    );
}

/// A battlefield of 255 attackers, each wearing its wedge as it fades in.
fn a_frame_of_255_attackers_allocates_nothing() {
    let mut client = Client::new(Config {
        elements: 512,
        draws: 1024,
        name_bytes: 16_384,
    });
    client.handle(Input::Surface {
        size: Size {
            width: 1440.0,
            height: 900.0,
        },
        scale: 2.0,
        safe_area: Insets::default(),
    });
    let mut view = ViewBuilder::new();
    for _ in 0..255 {
        let card = view.card(ZoneName::Battlefield, Side::You, "Serra Angel");
        card.types = vec![CardType::Creature];
        card.combat = Some(Combat::Attacking {
            planeswalker: None,
            target_departed: None,
            blocked: false,
        });
    }
    view.script(ScriptBuilder::reset().build());
    client.handle(Input::Update(Box::new(view.update())));
    client.frame(0);
    let before = ALLOCATIONS.load(Ordering::Relaxed);
    let frame = client.frame(20_000_000);
    let after = ALLOCATIONS.load(Ordering::Relaxed);
    let count = |material: Material| {
        frame
            .draws
            .iter()
            .filter(|draw| draw.material == material as u8)
            .count()
    };
    // A row shows as many as fit; the rest wait undrawn at its end.
    assert!(count(Material::Card) > 10, "many attackers are drawn");
    assert_eq!(
        count(Material::Glow),
        count(Material::Card),
        "each attacker drawn wears a wedge"
    );
    assert_eq!(after - before, 0, "allocations in a frame of 255 attackers");
}

/// A frame of a table of 255 creatures, each with its power and toughness and two kinds of
/// counters, allocates nothing.
fn a_badged_table_allocates_nothing() {
    let mut client = Client::new(Config::LARGEST);
    client.handle(Input::Surface {
        size: Size {
            width: 1440.0,
            height: 900.0,
        },
        scale: 2.0,
        safe_area: Insets::default(),
    });
    let mut builder = ViewBuilder::new();
    for index in 0..255_i32 {
        let side = if index % 2 == 0 {
            Side::You
        } else {
            Side::Opponent
        };
        let creature = builder.card(ZoneName::Battlefield, side, &format!("Creature {index}"));
        creature.types = vec![CardType::Creature];
        creature.pt = Some(PowerToughness {
            power: index % 10,
            toughness: 3,
        });
        creature.damage = index % 2;
        creature.counters = ["time", "ice"]
            .into_iter()
            .map(|name| CounterView {
                kind: Counter::named(name).expect("a kind of counter"),
                count: 2,
            })
            .collect();
    }
    client.handle(Input::Update(Box::new(builder.update())));
    let glyphs: Vec<GlyphCell> = client
        .wanted_glyphs()
        .map(|key| GlyphCell {
            key,
            rect: [0, 0, 20, 24],
            bearing: [0, -20],
        })
        .collect();
    client.handle(Input::Assets(AssetReady::Glyphs { page: 1, glyphs }));
    client.frame(0);
    let before = ALLOCATIONS.load(Ordering::Relaxed);
    let frame = client.frame(1_000_000);
    let after = ALLOCATIONS.load(Ordering::Relaxed);
    // Each creature in view wears a plaque and two counter badges; the rows scroll the rest.
    let cards = frame
        .resolved
        .iter()
        .filter(|element| element.role == Role::Card)
        .count();
    let panels = frame.draws[..frame.layer as usize]
        .iter()
        .filter(|draw| draw.material == Material::Panel as u8)
        .count();
    assert!(cards > 10, "the frame draws the rows' cards: {cards}");
    assert_eq!(panels, 3 * cards, "three badges on each card drawn");
    assert_eq!(
        after - before,
        0,
        "allocations during a badged table's frame"
    );
}
