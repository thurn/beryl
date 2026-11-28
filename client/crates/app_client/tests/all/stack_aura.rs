//! The aura round a mythic rare spell on the stack, and the burst it leaves as it goes, as the
//! player sees them in the Enhanced tier: drawn for a mythic and for no other rarity, kept while
//! the spell waits on the stack even when the script that cast it also resolves it, and played
//! once as it leaves.

use app_client::driver::{Duration, Page, Recording};
use beryl_card_records::{TokenMakers, fixture_message};
use beryl_protocol::GameUpdate;
use beryl_protocol::ipc::FixtureMessage;
use beryl_protocol::protocol::{CardType, Side, StackItem, Why, ZoneName};
use beryl_protocol::script::{Act, Subject};
use beryl_protocol::testing::{ScriptBuilder, ViewBuilder, timing};
use kit_frame::frame::{Draw, Frame, Vfx};
use kit_ui::input::{Size, Tier};

/// The VFX records of `frame` that draw `kind`.
fn drawn(frame: &Frame, kind: Vfx) -> usize {
    (frame.draws.iter())
        .filter(|draw| draw.is_vfx() && draw.uv[0] == kind as u16)
        .count()
}

/// A page that plays `name`, a sorcery, cast from its owner's hand and resolved into the
/// graveyard in one script: the spell flies to the stack, waits there, and then fades away.
fn cast_and_resolved(name: &str) -> Page {
    let mut builder = ViewBuilder::new();
    for land in ["Island", "Plains"] {
        builder.card(ZoneName::Battlefield, Side::You, land).types = vec![CardType::Land];
    }
    let spell = builder.card(ZoneName::Hand, Side::You, name);
    spell.types = vec![CardType::Sorcery];
    let slot = spell.slot;
    builder.script(ScriptBuilder::reset().build());
    let before = builder.update();

    let mut view = before.view.clone();
    let record = |view: &mut beryl_protocol::protocol::PlayerView, zone| {
        let object = (view.objects.iter_mut())
            .find(|object| object.slot == slot)
            .expect("the spell is in the view");
        object.zone = zone;
        object.clone()
    };
    let item = StackItem::Spell {
        slot,
        targets: Vec::new(),
        x: 0,
        paid: Vec::new(),
    };
    let on_stack = record(&mut view, ZoneName::Stack);
    let gone = record(&mut view, ZoneName::Graveyard);
    view.players.you.graveyard.push(slot);
    let mut script = ScriptBuilder::default()
        .beat([Act::Fly {
            from: ZoneName::Hand,
            why: Why::Cast,
            object: on_stack,
            at: None,
            item: Some(item),
            origin: None,
        }])
        .beat([Act::Vanish {
            subject: Subject::Object { slot },
            from: ZoneName::Stack,
            why: Why::Resolve,
            object: Some(gone),
            at: None,
        }])
        .build();
    script.timing = vec![timing(400.0), timing(350.0)];
    view.script = Some(script);
    let after = GameUpdate {
        session: before.session,
        view,
    };
    let mut messages = vec![
        FixtureMessage::game_update(&before),
        FixtureMessage::game_update(&after),
    ];
    messages.insert(0, fixture_message(&messages, TokenMakers::NONE));
    Page::new(
        crate::test_data::data(),
        Recording(messages).into(),
        Side::You,
        Size {
            width: 1440.0,
            height: 900.0,
        },
    )
    .expect("the views can be read")
    .with_tier(Tier::Enhanced)
}

/// For each 10 ms frame of `page` until it ends, how many aura and burst records it draws.
fn played(page: &mut Page) -> Vec<(usize, usize)> {
    let mut frames = Vec::new();
    for _ in 0..400 {
        page.step(Duration::from_millis(10));
        let frame = page.frame();
        frames.push((
            drawn(frame, Vfx::MythicStack),
            drawn(frame, Vfx::MythicStackOutro),
        ));
    }
    frames
}

#[test]
fn a_mythic_spell_wears_its_aura_on_the_stack_and_bursts_as_it_leaves() {
    // Time Warp's printing in the card store is a mythic rare.
    let frames = played(&mut cast_and_resolved("Time Warp"));
    let aura = frames.iter().position(|&(aura, _)| aura > 0);
    let burst = frames.iter().position(|&(_, burst)| burst > 0);
    let (Some(aura), Some(burst)) = (aura, burst) else {
        panic!("the aura and the burst are drawn: {frames:?}");
    };
    assert!(aura < burst, "the aura comes before the burst");
    // The aura lasts from the cast until the spell leaves, through its wait on the stack, though
    // the record the spell is drawn with already says the graveyard.
    assert!(
        frames[aura..burst].iter().all(|&(aura, _)| aura == 1),
        "the aura stays on while the spell is on the stack: {frames:?}"
    );
    assert!(
        burst - aura > 20,
        "the spell waits on the stack: {aura}..{burst}"
    );
    assert!(
        frames[burst..].iter().all(|&(aura, _)| aura == 0),
        "the aura goes as the burst begins"
    );
    let bursting = frames[burst..].iter().filter(|&&(_, b)| b > 0).count();
    // The burst rides on the departing card, so it ends with the 350 ms step that takes the
    // card away if that is sooner than its own 550 ms.
    assert!(
        (25..=56).contains(&bursting),
        "the burst plays once, as the card leaves: {bursting} frames"
    );
}

#[test]
fn a_spell_of_another_rarity_wears_no_aura() {
    // Counterspell's printing in the card store is a rare.
    let frames = played(&mut cast_and_resolved("Counterspell"));
    assert!(frames.iter().all(|&(aura, burst)| aura == 0 && burst == 0));
}

#[test]
fn the_baseline_tier_draws_no_procedural_aura() {
    let mut page = cast_and_resolved("Time Warp").with_tier(Tier::Baseline);
    let frames = played(&mut page);
    assert!(frames.iter().all(|&(aura, burst)| aura == 0 && burst == 0));
    assert!(page.frame().draws.iter().all(|draw| !Draw::is_vfx(draw)));
}
