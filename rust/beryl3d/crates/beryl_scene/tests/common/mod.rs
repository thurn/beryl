//! A table and the scripts played over it, built by hand, for the script clock's tests.

#![allow(dead_code)] // Each test binary uses part of it.

use beryl_scene::frame::{Frame, FrameBuilder};
use beryl_scene::input::{Insets, Size};
use beryl_scene::play::Show;
use beryl_scene::table::{DESKTOP, Table, Window};
use beryl_scene::testing::{ScriptBuilder, ViewBuilder, object, timing};
use beryl_session::GameUpdate;
use beryl_session::protocol::{CardType, ObjectId, ObjectView, PlayerView, Side, Why, ZoneName};
use beryl_session::script::{Act, Fact, StepTiming};

/// A millisecond, in nanoseconds.
pub const MS: u64 = 1_000_000;

pub fn window(width: f32, height: f32) -> Window {
    Window {
        size: Size { width, height },
        scale: 2.0,
        safe_area: Insets::default(),
    }
}

/// A show at 1440 by 900.
pub fn show() -> Show {
    Show::new(Table::new(DESKTOP, window(1440.0, 900.0)))
}

/// A frame with room for every record these tables need.
pub fn empty_frame() -> Frame {
    Frame::with_capacity(256, 256, 8192)
}

/// The frame `show` draws at `now`.
pub fn frame(show: &mut Show, frame: &mut Frame, now: u64) -> Frame {
    let mut builder = FrameBuilder::new(frame);
    show.frame(now, &mut builder);
    builder.finish().clone()
}

/// The slots of the cards the scripts move.
#[derive(Clone, Copy, Debug)]
pub struct Cards {
    /// A Plains in the viewer's hand, played in the first script.
    pub land: ObjectId,
    /// A Counterspell in the viewer's hand, discarded in the second script.
    pub discard: ObjectId,
    /// The top card of the viewer's library, drawn in `draw`.
    pub top: ObjectId,
    /// A Serra Angel whose summoning sickness wears off in the first script.
    pub angel: ObjectId,
    /// An Island that taps in the first script.
    pub island: ObjectId,
    /// The Soldier token the first script creates.
    pub token: ObjectId,
}

/// The table before: a library, a hand of three and two permanents, with a reset script.
pub fn before() -> (GameUpdate, Cards) {
    let mut builder = ViewBuilder::new();
    for _ in 0..5 {
        builder.card(ZoneName::Library, Side::You, "Island");
    }
    let land = builder.card(ZoneName::Hand, Side::You, "Plains");
    land.types = vec![CardType::Land];
    let land = land.slot;
    let discard = builder.card(ZoneName::Hand, Side::You, "Counterspell").slot;
    builder.card(ZoneName::Hand, Side::You, "Memory Lapse");
    let angel = builder.card(ZoneName::Battlefield, Side::You, "Serra Angel");
    angel.types = vec![CardType::Creature];
    angel.summoning_sick = true;
    let angel = angel.slot;
    let island = builder.card(ZoneName::Battlefield, Side::You, "Island");
    island.types = vec![CardType::Land];
    let island = island.slot;
    builder.script(ScriptBuilder::reset().build());
    let mut update = builder.update();
    let top = *update.view.players.you.library.last().expect("a library");
    let token = ObjectId(u16::try_from(update.view.objects.len()).expect("few objects"));
    update.view.objects.sort_by_key(|o| o.slot);
    let cards = Cards {
        land,
        discard,
        top,
        angel,
        island,
        token,
    };
    (update, cards)
}

fn record(view: &PlayerView, slot: ObjectId) -> ObjectView {
    view.objects
        .iter()
        .find(|o| o.slot == slot)
        .expect("the card is in the view")
        .clone()
}

fn replace(view: &mut PlayerView, record: &ObjectView) {
    if let Some(object) = view.objects.iter_mut().find(|o| o.slot == record.slot) {
        *object = record.clone();
    } else {
        view.objects.push(record.clone());
        view.objects.sort_by_key(|o| o.slot);
    }
}

/// The first script: three beats. The Plains flies from the hand onto the battlefield; a Soldier
/// token appears; the Angel's summoning sickness wears off (`set`) as the Island taps, which the
/// client draws as an instant change. Its timings: 420, 300, 0 and 150 ms.
pub fn three_beats(before: &GameUpdate, cards: Cards) -> (GameUpdate, Vec<StepTiming>) {
    let mut view = before.view.clone();
    let mut land = record(&view, cards.land);
    land.zone = ZoneName::Battlefield;
    replace(&mut view, &land);
    view.battlefield.push(cards.land);
    let mut token = object(
        cards.token,
        ZoneName::Battlefield,
        Side::You,
        Some("Soldier Token"),
    );
    token.token = true;
    token.types = vec![CardType::Creature];
    replace(&mut view, &token);
    view.battlefield.push(cards.token);
    let mut angel = record(&view, cards.angel);
    angel.summoning_sick = false;
    replace(&mut view, &angel);
    let mut island = record(&view, cards.island);
    island.tapped = true;
    replace(&mut view, &island);
    let script = ScriptBuilder::default()
        .beat([Act::Fly {
            from: ZoneName::Hand,
            why: Why::Play,
            object: land,
            at: None,
            item: None,
            origin: None,
        }])
        .beat([Act::Appear {
            from: ZoneName::Nowhere,
            why: Why::Created,
            object: token,
            at: None,
            item: None,
        }])
        .beat([
            Act::Set {
                fact: Fact::Object { object: angel },
            },
            Act::Tap { object: island },
        ])
        .build();
    view.script = Some(script);
    let timings = vec![timing(420.0), timing(300.0), timing(0.0), timing(150.0)];
    (
        GameUpdate {
            session: before.session,
            view,
        },
        timings,
    )
}

/// One beat in which a card of the viewer's hand flies to their graveyard, for 400 ms.
pub fn discard(before: &GameUpdate, cards: Cards) -> (GameUpdate, Vec<StepTiming>) {
    let mut view = before.view.clone();
    let mut card = record(&view, cards.discard);
    card.zone = ZoneName::Graveyard;
    replace(&mut view, &card);
    view.players.you.graveyard.push(cards.discard);
    view.script = Some(
        ScriptBuilder::default()
            .beat([Act::Fly {
                from: ZoneName::Hand,
                why: Why::Discard,
                object: card,
                at: None,
                item: None,
                origin: None,
            }])
            .build(),
    );
    (
        GameUpdate {
            session: before.session,
            view,
        },
        vec![timing(400.0)],
    )
}

/// One beat in which the viewer draws the top card of their library, for 400 ms.
pub fn draw(before: &GameUpdate, cards: Cards) -> (GameUpdate, Vec<StepTiming>) {
    let mut view = before.view.clone();
    let card = object(cards.top, ZoneName::Hand, Side::You, Some("Island"));
    replace(&mut view, &card);
    view.players.you.library.retain(|&slot| slot != cards.top);
    view.script = Some(
        ScriptBuilder::default()
            .beat([Act::Fly {
                from: ZoneName::Library,
                why: Why::Draw,
                object: card,
                at: None,
                item: None,
                origin: None,
            }])
            .build(),
    );
    (
        GameUpdate {
            session: before.session,
            view,
        },
        vec![timing(400.0)],
    )
}
