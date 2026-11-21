//! `Client::frame` allocates nothing once a prompt bar has been laid out, nor with a 60-card
//! hand and its shadows, nor while a life total falls and the amount it fell by sinks away, nor
//! while the turn banner fades in, stands and fades out, nor while 255 marks are drawn, nor with
//! 255 attackers wearing their wedges, nor on a table of 255 permanents that each wear badges,
//! nor while a hand card rises under the mouse, nor while a scrolled battlefield row slides, and
//! neither does moving a float of the flat layer and writing it, nor an element anchored to a
//! card following it as it flies, nor elements of the screen fading in and moving
//! (docs/beryl-3d.md, "The frame": no allocation). This binary counts every heap allocation, so it holds one test.

mod common;

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

use app_client::Input;
use app_client::{Client, Config};
use beryl_protocol::GameUpdate;
use beryl_protocol::prompt::Purpose;
use beryl_protocol::prompt::{ButtonId, PartName, PromptButton, PromptView, ShapeBody, Words};
use beryl_protocol::protocol::{
    CardType, Combat, Counter, CounterView, DecisionId, DecisionView, Kind, ObjectId,
    PowerToughness, SessionId, Side, Status, ZoneName,
};
use beryl_protocol::script::{Act, Mark, Notice, Subject, Tally};
use beryl_protocol::testing::{ScriptBuilder, ViewBuilder};
use game_play::light::{MarkSpan, Span};
use game_play::verbs::Interval;
use game_table::table::Table;
use game_table::table::spot_of;
use glam::Vec2;
use kit_frame::frame::{Frame, FrameBuilder, Material, Place, Role, StateBits};
use kit_ui::element::{Anchor, floats, row, text};
use kit_ui::input::{
    AssetReady, Button, GlyphCell, Insets, Modifiers, Pointer, PointerKind, PointerPhase, Size,
    Tier,
};
use kit_ui::layout::{FloatLook, Layer};

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

/// A client with room for these frames, drawing in a 1440 by 900 window.
fn client() -> Client {
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
    client
}

/// A frame with a 60-card hand, each card with its contact shadow, over a battlefield and a
/// library.
fn a_frame_with_a_sixty_card_hand_allocates_nothing() {
    let mut crowded = ViewBuilder::new();
    for index in 0..60 {
        crowded.card(ZoneName::Hand, Side::You, &format!("Card {index}"));
        crowded.card(ZoneName::Library, Side::You, "Island");
    }
    for _ in 0..20 {
        crowded
            .card(ZoneName::Battlefield, Side::You, "Plains")
            .types = vec![CardType::Land];
    }
    crowded.script(ScriptBuilder::reset().build());
    let mut hand = client();
    hand.handle(Input::Update(Box::new(crowded.update())));
    hand.frame(0);
    hand.prepare(1);
    let before = ALLOCATIONS.load(Ordering::Relaxed);
    hand.frame(1);
    let after = ALLOCATIONS.load(Ordering::Relaxed);
    assert_eq!(
        after - before,
        0,
        "allocations in a frame with a 60-card hand"
    );
}

#[test]
fn a_frame_with_a_prompt_bar_allocates_nothing() {
    let mut client = client();
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
    client.prepare(0);
    let before = ALLOCATIONS.load(Ordering::Relaxed);
    let records = client.frame(0).draws.len();
    let after = ALLOCATIONS.load(Ordering::Relaxed);
    assert!(records > 3, "the frame holds the bar's records");
    assert_eq!(after - before, 0, "allocations during Client::frame");

    a_frame_with_a_sixty_card_hand_allocates_nothing();

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
        client.prepare(now);
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
        client.prepare(now);
        let before = ALLOCATIONS.load(Ordering::Relaxed);
        client.frame(now);
        let after = ALLOCATIONS.load(Ordering::Relaxed);
        assert_eq!(after - before, 0, "allocations in a frame with 255 marks");
    }

    the_turn_banner_allocates_nothing();
    a_frame_of_255_attackers_allocates_nothing();

    tables_allocate_nothing();

    a_moved_float_allocates_nothing();
    an_anchored_element_allocates_nothing();
    a_transition_allocates_nothing();
    changing_the_look_of_a_grid_allocates_nothing();
}

/// The frames of the turn banner: the one that shows it, those that fade it in and out, and
/// the one that hides it.
fn the_turn_banner_allocates_nothing() {
    let (mut client, steps) = common::counting(vec![Act::Announce {
        notice: Notice::Turn {
            active: Side::Opponent,
        },
    }]);
    let (from, to) = steps[0];
    for now in [
        from,
        from + (to - from) / 10,
        from + (to - from) / 2,
        to - 1,
        to,
    ] {
        client.prepare(now);
        let before = ALLOCATIONS.load(Ordering::Relaxed);
        client.frame(now);
        let after = ALLOCATIONS.load(Ordering::Relaxed);
        assert_eq!(
            after - before,
            0,
            "allocations in the frame at {now} during the turn banner"
        );
    }
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
    client.prepare(20_000_000);
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
    // A wedge is two glow records: its body in the tone's glow, its rim in the tone's core.
    assert_eq!(
        count(Material::Glow),
        2 * count(Material::Card),
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
    client.prepare(1_000_000);
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

/// A badged table, a hand card rising under the mouse and a row sliding as it scrolls.
fn tables_allocate_nothing() {
    a_badged_table_allocates_nothing();
    a_rising_hand_card_allocates_nothing();
    a_scrolling_row_allocates_nothing();
}

/// The frames while a hand card rises under the mouse, and while it falls back as the mouse moves
/// onto the next card, allocate nothing.
fn a_rising_hand_card_allocates_nothing() {
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
    let mut view = ViewBuilder::sample();
    view.script(ScriptBuilder::reset().build());
    client.handle(Input::Update(Box::new(view.update())));
    let hand: Vec<Vec2> = client
        .frame(0)
        .resolved
        .iter()
        .filter(|element| element.role == Role::Card && element.interactive)
        .filter(|element| {
            matches!(
                element.place,
                Place::Card {
                    zone: ZoneName::Hand,
                    ..
                }
            )
        })
        .map(|element| element.corners[3] + (element.corners[1] - element.corners[3]) * 0.1)
        .collect();
    assert_eq!(hand.len(), 3, "the viewer's hand");
    let mut now = 0;
    for point in hand {
        client.handle(Input::Pointer(Pointer {
            id: 0,
            kind: PointerKind::Mouse,
            phase: PointerPhase::Move,
            position: point,
            button: Button::None,
            wheel: Vec2::ZERO,
            modifiers: Modifiers::default(),
        }));
        for _ in 0..4 {
            now += 16_000_000;
            client.prepare(now);
            let before = ALLOCATIONS.load(Ordering::Relaxed);
            let hovered = client
                .frame(now)
                .resolved
                .iter()
                .any(|element| element.state.has(StateBits::HOVERED));
            let after = ALLOCATIONS.load(Ordering::Relaxed);
            assert!(hovered, "a card is hovered at {now}");
            assert_eq!(
                after - before,
                0,
                "allocations in a frame at {now} as a card rises"
            );
        }
    }
}

/// A float moved and faded by a new look, then written into a frame.
fn a_moved_float_allocates_nothing() {
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
            scale: 1.0,
        },
    );
    let mut builder = FrameBuilder::new(&mut frame, Tier::Baseline);
    layer.write(&mut builder, &table.camera().screen_plane(), false);
    let elements = builder.finish().resolved.len();
    let after = ALLOCATIONS.load(Ordering::Relaxed);
    assert_eq!(elements, 1, "the frame holds the float");
    assert_eq!(
        after - before,
        0,
        "allocations setting a float's look and writing it"
    );
}

/// The frames while a card flies with an element of the flat layer anchored to it: each one
/// draws the table, moves the element onto the card and writes the layer.
fn an_anchored_element_allocates_nothing() {
    let (start, cards) = common::before();
    let (flight, timings) = common::three_beats(&start, cards);
    let mut show = common::show();
    show.push(Box::new(start), Vec::new());
    show.push(Box::new(flight), timings);
    let mut layer = Layer::new();
    let mut badge = text("Plains");
    badge.anchor = Some(Anchor::Object(
        Subject::Object { slot: cards.land },
        Vec2::new(0.5, 0.5),
    ));
    layer.show(Some(row().name("screen").child(badge)));
    let mut frame = common::empty_frame();
    for now in (0..30).map(|index| index * 16 * common::MS) {
        show.prepare(now);
        let before = ALLOCATIONS.load(Ordering::Relaxed);
        let mut builder = FrameBuilder::new(&mut frame, Tier::Baseline);
        show.frame(now, &mut builder);
        layer.follow(|subject| spot_of(&builder, show.table(), subject));
        layer.write(&mut builder, &show.table().camera().screen_plane(), true);
        builder.finish();
        let after = ALLOCATIONS.load(Ordering::Relaxed);
        // The first frames take the scripts and lay out the table's cards.
        if now >= 2 * 16 * common::MS {
            assert_eq!(
                after - before,
                0,
                "allocations at {now} with an anchored element"
            );
        }
    }
}

/// The frames while an element enters, another leaves, and a kept one moves to a new place.
fn a_transition_allocates_nothing() {
    use kit_ui::element::{Presence, Property, Transition};
    use kit_ui::widgets::screen::{SCREEN_TIMING, with_transitions};

    const LEAVE: Presence = Presence::fade(SCREEN_TIMING);
    const SCALE: &[Transition] = &[Transition::new(Property::Scale, SCREEN_TIMING)];
    let mut layer = Layer::new();
    let screen = |gap: f32, scale: f32| {
        row()
            .name("screen")
            .gap(gap)
            .child(text("Keep"))
            .child(text("Pass").scale(scale).transition(SCALE))
    };
    layer.show(Some(with_transitions(
        screen(0.0, 1.0).child(text("Leave").leave(&LEAVE)),
    )));
    layer.tick(0);
    layer.tick(common::MS * 1_000);
    layer.show(Some(with_transitions(
        screen(40.0, 0.8).child(text("Mulligan")),
    )));
    let table = Table::default();
    let mut frame = Frame::with_capacity(64, 256, 1024);
    for now in (0..12).map(|index| common::MS * (1_000 + index * 16)) {
        let before = ALLOCATIONS.load(Ordering::Relaxed);
        let moving = layer.tick(now).moving;
        let mut builder = FrameBuilder::new(&mut frame, Tier::Baseline);
        layer.write(&mut builder, &table.camera().screen_plane(), true);
        builder.finish();
        let after = ALLOCATIONS.load(Ordering::Relaxed);
        assert!(moving, "the transitions last past {now}");
        assert_eq!(after - before, 0, "allocations at {now} in a transition");
    }
}

/// The step row can change its look from inside a frame, with the same grid geometry.
fn changing_the_look_of_a_grid_allocates_nothing() {
    use kit_ui::element::{Display, Track};

    let grid = |opacity| {
        row()
            .name("Steps")
            .display(Display::Grid)
            .grid_template_columns(vec![Track::Points(40.0)])
            .grid_template_rows(vec![Track::Points(40.0)])
            .opacity(opacity)
    };
    let mut layer = Layer::new();
    layer.show(Some(row().child(grid(1.0))));
    layer.take_work();
    let next = grid(0.5);

    let before = ALLOCATIONS.load(Ordering::Relaxed);
    let replaced = layer.replace(next);
    let after = ALLOCATIONS.load(Ordering::Relaxed);
    assert!(replaced);
    assert_eq!(after - before, 0);
    assert_eq!(layer.take_work(), (0, 0));
}

/// The frames while a battlefield row slides after a turn of the wheel scrolls it, and the
/// counts off its ends change, allocate nothing.
fn a_scrolling_row_allocates_nothing() {
    let mut client = Client::new(Config {
        elements: 512,
        draws: 2048,
        name_bytes: 8192,
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
    for index in 0..40 {
        view.card(ZoneName::Battlefield, Side::You, &format!("Land {index}"))
            .types = vec![CardType::Land];
    }
    view.script(ScriptBuilder::reset().build());
    client.handle(Input::Update(Box::new(view.update())));
    let point = client
        .frame(0)
        .resolved
        .iter()
        .find(|element| element.role == Role::Card)
        .map(|element| element.corners[3] + (element.corners[1] - element.corners[3]) * 0.1)
        .expect("a land");
    client.handle(Input::Pointer(Pointer {
        id: 0,
        kind: PointerKind::Mouse,
        phase: PointerPhase::Wheel,
        position: point,
        button: Button::None,
        wheel: Vec2::new(0.0, -48.0),
        modifiers: Modifiers::default(),
    }));
    let mut now = 0;
    let mut moving = 0;
    for _ in 0..12 {
        now += 16_000_000;
        client.prepare(now);
        let before = ALLOCATIONS.load(Ordering::Relaxed);
        let frame = client.frame(now);
        moving += usize::from(
            frame
                .resolved
                .iter()
                .any(|e| e.state.has(StateBits::MOVING)),
        );
        let after = ALLOCATIONS.load(Ordering::Relaxed);
        assert_eq!(
            after - before,
            0,
            "allocations in a frame at {now} as a row slides"
        );
    }
    assert!(moving > 0, "the row slides");
}
