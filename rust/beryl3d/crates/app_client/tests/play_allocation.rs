//! A frame that ends one script and starts the next allocates nothing (docs/beryl-3d.md, "The
//! frame": no allocation): scripts are compiled when their updates arrive. This binary counts
//! every heap allocation, so it holds one test, which also checks that the effects a starting
//! script begins, `Lit::advance`, a frame of twenty streaks, one of forty effects of light, one
//! with 50 arrows and one that lays a script out again as it starts allocate nothing.

#[path = "../../game_probe/tests/common/mod.rs"]
mod common;

use kit_ui::input::Tier;
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

use beryl_protocol::protocol::{Mana, ObjectId, Side, ZoneName};
use beryl_protocol::script::{
    self, Act, Arrow, Curve, Edge, Mark, StepEffect, StepTiming, Stream, Subject,
};
use beryl_protocol::testing::{ScriptBuilder, ViewBuilder, timing};
use common::{Cards, MS, before, emerge, empty_frame, resolve, show, three_beats};
use game_play::light::streaks::RECORDS;
use game_play::light::{ArrowSpan, Lit, MarkSpan, Span};
use game_play::play::Show;
use game_play::play::record::{Plant, Planted};
use game_play::verbs::Interval;
use kit_frame::frame::{Frame, FrameBuilder, Material, Role};

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

#[test]
fn a_frame_that_starts_a_script_allocates_nothing() {
    let (start, cards) = before();
    let (after, mut timings) = three_beats(&start, cards);
    // Twenty effects begin with the script's first step, so that the frame that starts it
    // also gives the show twenty effect runs.
    timings[0].effects = (0..20)
        .map(|at| StepEffect {
            name: if at % 2 == 0 { "glow" } else { "settle" }.to_owned(),
            when: Edge::Start,
            on: Some(Subject::Object {
                slot: ObjectId(u16::try_from(at).expect("few slots")),
            }),
            into: None,
            duration: Some(2000.0),
            delay: 0.0,
            curve: Curve::Linear,
        })
        .collect();
    let after_three = after.clone();
    let mut show = show();
    let mut frame = empty_frame();
    // A span that begins with the token's step, so that a frame also gives the table's `Lit` a
    // span.
    show.plant(Plant {
        kind: Planted::Mark,
        script: 1,
        t: 500 * MS,
    });
    show.push(Box::new(start), Vec::new());
    show.push(Box::new(after), timings);
    // The frame at 0 shows the first view and starts the three-beat script; the frame at 500 ms
    // draws it under way, and the frame at 900 ms ends it.
    for now in [0, 500 * MS, 900 * MS] {
        let (records, allocations) = counted_frame(&mut show, &mut frame, now);
        assert!(records > 1, "the frame draws the cards");
        assert_eq!(
            allocations,
            0,
            "allocations in the frame at {} ms",
            now / MS
        );
    }
    assert_eq!(
        show.lit().open().len(),
        1,
        "the planted span reached the table"
    );
    assert_eq!(show.effects().len(), 20, "the effects outlive the script");

    // An ability that goes on the stack and resolves: the frames that start, draw and end
    // each script allocate nothing either.
    let (emerged, timings) = emerge(&after_three, cards, 5);
    let (resolved, resolve_timings) = resolve(&emerged, 5);
    show.push(Box::new(emerged), timings);
    // The emerge plays from 1000 to 1200 ms, putting the ability on the stack at its start, and
    // the ability rests there after it. The resolution arrives then and plays from 1400 to
    // 1600 ms, the ability fading where it stands, and it is gone after.
    for now in [1_000 * MS, 1_100 * MS, 1_300 * MS, 1_400 * MS, 1_700 * MS] {
        if now == 1_400 * MS {
            show.push(Box::new(resolved.clone()), resolve_timings.clone());
        }
        show.prepare(now);
        let before = ALLOCATIONS.load(Ordering::Relaxed);
        let mut builder = FrameBuilder::new(&mut frame, Tier::Baseline);
        show.frame(now, &mut builder);
        let drawn = builder.finish();
        let ability = (drawn.resolved.iter()).any(|element| element.role == Role::Ability);
        let sounded = !drawn.sounds.is_empty();
        let after = ALLOCATIONS.load(Ordering::Relaxed);
        // The emerge's sound is listed in the frame at its start, which allocates nothing.
        assert_eq!(sounded, now == 1_000 * MS, "sounds at {} ms", now / MS);
        assert_eq!(
            after - before,
            0,
            "allocations in the frame at {} ms",
            now / MS
        );
        let shown = now <= 1_400 * MS;
        assert_eq!(ability, shown, "the ability is drawn at {} ms", now / MS);
    }

    // Forgetting the spans that have gone allocates nothing.
    let mut lit = Lit::with_capacity(4);
    for slot in 0..4 {
        let span = Span::Mark(MarkSpan {
            mark: Mark::Eligible,
            on: Subject::Object {
                slot: ObjectId(slot),
            },
        });
        lit.begin(span, Interval { from: 0, to: MS });
        if slot % 2 == 0 {
            lit.end(
                &span,
                Interval {
                    from: MS,
                    to: 2 * MS,
                },
            );
        }
    }
    let before = ALLOCATIONS.load(Ordering::Relaxed);
    lit.advance(3 * MS);
    assert_eq!(
        ALLOCATIONS.load(Ordering::Relaxed) - before,
        0,
        "allocations in `advance`"
    );
    assert_eq!(lit.open().len(), 2);

    fifty_arrows_allocate_nothing(&mut show, cards);
    twenty_streaks_allocate_nothing();
    forty_effects_allocate_nothing();
    laying_a_script_out_again_allocates_nothing();
}

/// The frame that starts a script whose mark's end waits for the mark to stand whole, and so
/// lays the script out again, allocates nothing.
fn laying_a_script_out_again_allocates_nothing() {
    let (start, cards) = before();
    let mark = |span| Act::Mark {
        span,
        mark: Mark::Selected,
        on: Subject::Object { slot: cards.angel },
    };
    let mut begun = start.clone();
    begun.view.script = Some(
        ScriptBuilder::default()
            .beat([mark(script::Span::Begin)])
            .build(),
    );
    let mut ended = begun.clone();
    ended.view.script = Some(
        ScriptBuilder::default()
            .beat([mark(script::Span::End)])
            .beat([mark(script::Span::Begin)])
            .build(),
    );
    let held = StepTiming {
        hold: Some(290.0),
        with: true,
        leads: true,
        ..timing(150.0)
    };
    let mut show = show();
    let mut frame = empty_frame();
    show.push(Box::new(start), Vec::new());
    show.push(Box::new(begun), vec![timing(200.0)]);
    show.push(Box::new(ended), vec![held, timing(150.0)]);
    // The mark's script plays from 0 to 200 ms, and the next starts at the frame at 200 ms.
    for now in [0, 100 * MS, 200 * MS, 300 * MS] {
        show.prepare(now);
        let before = ALLOCATIONS.load(Ordering::Relaxed);
        let mut builder = FrameBuilder::new(&mut frame, Tier::Baseline);
        show.frame(now, &mut builder);
        let after = ALLOCATIONS.load(Ordering::Relaxed);
        assert_eq!(
            after - before,
            0,
            "allocations in the frame at {} ms",
            now / MS
        );
    }
    assert_eq!(show.relays(), 1, "the script was laid out again");
}

/// A frame that draws twenty streaks at once allocates nothing.
fn twenty_streaks_allocate_nothing() {
    let mut builder = ViewBuilder::new();
    let island = builder
        .card(ZoneName::Battlefield, Side::You, "Island")
        .slot;
    let spell = builder.card(ZoneName::Stack, Side::You, "Shock").slot;
    builder.script(ScriptBuilder::reset().build());
    let start = builder.update();
    let mut streaks = start.clone();
    let acts = (0..20).map(|_| Act::Streak {
        from: Some(Subject::Object { slot: island }),
        to: Subject::Object { slot: spell },
        stream: Stream::Mana { color: Mana::Blue },
        amount: 1,
        object: None,
    });
    streaks.view.script = Some(ScriptBuilder::default().beat(acts).build());
    let timings = (0..20)
        .map(|at| StepTiming {
            with: at > 0,
            ..timing(300.0)
        })
        .collect();
    let mut show = common::show();
    let mut frame = Frame::with_capacity(256, 1024, 8192);
    show.push(Box::new(start), Vec::new());
    show.push(Box::new(streaks), timings);
    for now in [0, 150 * MS] {
        show.prepare(now);
        let before = ALLOCATIONS.load(Ordering::Relaxed);
        let mut builder = FrameBuilder::new(&mut frame, Tier::Baseline);
        show.frame(now, &mut builder);
        let drawn = builder
            .finish()
            .draws
            .iter()
            .filter(|draw| draw.material == Material::Streak as u8)
            .count();
        let after = ALLOCATIONS.load(Ordering::Relaxed);
        assert_eq!(
            after - before,
            0,
            "allocations in the frame at {} ms",
            now / MS
        );
        if now > 0 {
            assert_eq!(drawn, 20 * RECORDS, "twenty streaks are drawn");
        }
    }
}

/// A frame that holds forty effects of light at once allocates nothing. Each is on a slot of
/// its own, so that none cuts another short; those on the table's cards are drawn.
fn forty_effects_allocate_nothing() {
    let (start, cards) = before();
    let (after, mut timings) = three_beats(&start, cards);
    let names = ["glow", "impact", "flash", "beam", "pulse", "trail"];
    timings[0].effects = (0..40)
        .map(|at| StepEffect {
            name: names[at % names.len()].to_owned(),
            when: Edge::Start,
            on: Some(Subject::Object {
                slot: ObjectId(u16::try_from(at).expect("few slots")),
            }),
            into: None,
            duration: Some(2000.0),
            delay: 0.0,
            curve: Curve::Linear,
        })
        .collect();
    let mut show = show();
    let mut frame = Frame::with_capacity(256, 1024, 8192);
    show.push(Box::new(start), Vec::new());
    show.push(Box::new(after), timings);
    for now in [0, 150 * MS, 300 * MS] {
        show.prepare(now);
        let before = ALLOCATIONS.load(Ordering::Relaxed);
        let mut builder = FrameBuilder::new(&mut frame, Tier::Baseline);
        show.frame(now, &mut builder);
        let lights = builder
            .finish()
            .draws
            .iter()
            .filter(|draw| draw.material == Material::Glow as u8)
            .count();
        let after = ALLOCATIONS.load(Ordering::Relaxed);
        assert_eq!(
            after - before,
            0,
            "allocations in the frame at {} ms",
            now / MS
        );
        assert_eq!(show.effects().len(), 40);
        if now > 0 {
            assert!(lights > 0, "the effects are drawn");
        }
    }
}

/// A frame that draws 50 arrows between the Angel and the Island allocates nothing.
fn fifty_arrows_allocate_nothing(show: &mut Show, cards: Cards) {
    for number in 0..50 {
        let arrow = if number % 2 == 0 {
            Arrow::FriendlyTarget
        } else {
            Arrow::Block
        };
        let (from, to) = if number % 4 < 2 {
            (cards.angel, cards.island)
        } else {
            (cards.island, cards.angel)
        };
        show.lit_mut().begin(
            Span::Arrow(ArrowSpan {
                arrow,
                from: Subject::Object { slot: from },
                to: Subject::Object { slot: to },
            }),
            Interval {
                from: 1_800 * MS,
                to: 1_800 * MS,
            },
        );
    }
    // Each arrow writes two records for each piece of its ribbon and one for a target's head.
    let mut frame = Frame::with_capacity(512, 4096, 16384);
    show.prepare(1_900 * MS);
    let before = ALLOCATIONS.load(Ordering::Relaxed);
    let mut builder = FrameBuilder::new(&mut frame, Tier::Baseline);
    show.frame(1_900 * MS, &mut builder);
    let arrows = builder
        .finish()
        .resolved
        .iter()
        .filter(|element| element.role == Role::Arrow)
        .count();
    assert_eq!(
        ALLOCATIONS.load(Ordering::Relaxed) - before,
        0,
        "allocations in a frame with 50 arrows"
    );
    assert_eq!(arrows, 50);
}

/// Prepares the frame at `now` and draws it into `frame`, as a host does, and returns how many
/// records it drew and how many allocations drawing it made.
fn counted_frame(show: &mut Show, frame: &mut Frame, now: u64) -> (usize, usize) {
    show.prepare(now);
    let before = ALLOCATIONS.load(Ordering::Relaxed);
    let mut builder = FrameBuilder::new(frame, Tier::Baseline);
    show.frame(now, &mut builder);
    let records = builder.finish().draws.len();
    (records, ALLOCATIONS.load(Ordering::Relaxed) - before)
}
