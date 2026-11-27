//! The Secret Lair printing each visible card of a printed deck shows
//! (`ObjectView::printing`), over whole seeded shared-deck games seen from both seats: no art
//! is visible on more cards at once than the deck has copies of it, a card keeps its art from
//! zone to zone while someone knows it, both seats see the same art, an undo and a replay give
//! the same art, and a deck with no printings shows none.

use std::collections::BTreeMap;

use beryl_card_records::{PrintingIx, printing};
use beryl_cards::CARDS;
use beryl_cards::decks::{FORGETFUL_FISH, WU_CONTROL};
use beryl_cards::names::{DANDAN, ISLAND};
use beryl_engine::Seat;
use beryl_session::prompt::{Click, ClickReply, ClickRequest};
use beryl_session::protocol::{Candidate, DecisionView, ObjectId, Side, ZoneName};
use beryl_session::script::Subject;
use beryl_session::{
    CandidateIndex, Format, Game, PlayerView, SessionId, StartGame, Status, StopsSetting,
};

const SESSION: SessionId = SessionId(1);

fn owned(deck: &[(u8, &str)]) -> Vec<(u8, String)> {
    deck.iter().map(|&(n, name)| (n, name.to_owned())).collect()
}

fn new_game(seed: u64, format: Format, decks: [Vec<(u8, String)>; 2]) -> Game {
    let start = StartGame {
        format,
        seed,
        decks,
        human: Seat::P0,
        // Greedy has no random stream, so the same answers play the same game again.
        opponent: "greedy".to_owned(),
        stops: StopsSetting::SMART,
        sideboards: Default::default(),
    };
    Game::new(&CARDS, &start, SESSION).unwrap()
}

fn shared_game(seed: u64) -> Game {
    new_game(
        seed,
        Format::SHARED_DECK,
        [owned(FORGETFUL_FISH), Vec::new()],
    )
}

fn deciding(view: &PlayerView) -> Option<&DecisionView> {
    match &view.status {
        Status::Deciding { decision } => Some(decision),
        _ => None,
    }
}

/// The printing each object in `view` shows, by object.
fn printings(view: &PlayerView) -> BTreeMap<ObjectId, Option<PrintingIx>> {
    view.objects.iter().map(|o| (o.slot, o.printing)).collect()
}

/// What the views of a game showed, gathered view by view ([`Watch::see`]).
#[derive(Default)]
struct Watch {
    /// The printing each object last showed, while no shuffle could have dealt it anew.
    kept: BTreeMap<ObjectId, (String, PrintingIx)>,
    /// Each object's zones while it kept one printing.
    zones: BTreeMap<ObjectId, Vec<ZoneName>>,
    /// The most cards seen at once showing one printing, by its name.
    most: BTreeMap<String, usize>,
    /// Printings seen at all.
    shown: usize,
}

impl Watch {
    /// Checks the human's `view` and the other seat's `other` of one moment.
    fn see(&mut self, view: &PlayerView, other: &PlayerView) {
        let mine: BTreeMap<_, _> = view.objects.iter().map(|o| (o.slot, o)).collect();
        let theirs: BTreeMap<_, _> = other.objects.iter().map(|o| (o.slot, o)).collect();
        let mut visible: BTreeMap<ObjectId, (String, ZoneName, PrintingIx)> = BTreeMap::new();
        for o in mine.values().chain(theirs.values()) {
            let Some(name) = &o.name else {
                assert_eq!(
                    o.printing, None,
                    "{:?}: an unknown card shows no art",
                    o.slot
                );
                continue;
            };
            if o.token {
                assert_eq!(o.printing, None, "{name}: a token has no Secret Lair art");
                continue;
            }
            let p = o
                .printing
                .unwrap_or_else(|| panic!("{name}: every card of the deck has a printing"));
            let record = printing(p).expect("a printing of the table");
            assert_eq!(record.name, name, "{:?} shows another card's art", o.slot);
            if let Some((_, _, q)) = visible.insert(o.slot, (name.clone(), o.zone, p)) {
                assert_eq!(p, q, "{name}: both seats see one art");
            }
        }
        let mut counts: BTreeMap<PrintingIx, usize> = BTreeMap::new();
        for (_, _, p) in visible.values() {
            *counts.entry(*p).or_default() += 1;
        }
        for (&p, &n) in &counts {
            let record = printing(p).unwrap();
            assert!(
                n <= record.copies as usize,
                "{n} cards show {} {}/{}, of which the deck has {}",
                record.name,
                record.set,
                record.collector_number,
                record.copies
            );
            let most = self.most.entry(record.name.to_owned()).or_default();
            *most = (*most).max(n);
        }
        self.shown += visible.len();
        // A card some player knows keeps its art; one in a library, or known to no one, may
        // be dealt anew by a shuffle.
        for (&slot, (name, zone, p)) in &visible {
            if let Some((kept_name, q)) = self.kept.get(&slot)
                && kept_name == name
            {
                assert_eq!(p, q, "{name} {slot:?} changed its art in the {zone:?}");
            } else {
                self.zones.remove(&slot);
            }
            let zones = self.zones.entry(slot).or_default();
            if !zones.contains(zone) {
                zones.push(*zone);
            }
            self.kept.insert(slot, (name.clone(), *p));
        }
        self.kept.retain(|slot, _| {
            visible
                .get(slot)
                .is_some_and(|(_, zone, _)| *zone != ZoneName::Library)
        });
        self.zones.retain(|slot, _| self.kept.contains_key(slot));
    }

    /// The most zones one card went through showing one art.
    fn longest_trip(&self) -> usize {
        self.zones.values().map(Vec::len).max().unwrap_or(0)
    }
}

/// Plays on to the human's next decision or the end, checking every view on the way, and
/// returns the view there.
fn to_human(game: &mut Game, watch: &mut Watch) -> PlayerView {
    let mut last = None;
    while let Some(view) = game.next_view() {
        let other = game.opponent_view();
        watch.see(&view, &other);
        last = Some(view);
    }
    last.expect("the game plays on from where it is")
}

/// The human's answer to `d`: a fixed pattern over the candidates.
fn pick(d: &DecisionView) -> u16 {
    let n = u32::try_from(d.candidates.len()).unwrap();
    u16::try_from((d.id.seq * 7 + 3) % n).unwrap()
}

/// Over whole seeded shared-deck games, from both seats: at most five visible cards show one
/// art of Dandân or of Island (the deck's copies of it), each card shows an art of its own
/// name, a card some player knows keeps its art from zone to zone, and both seats see the
/// same art. Every tenth answer is taken back and given again, and the views after the undo
/// show the art they showed before. Each game's replay ends showing the art the game did.
#[test]
fn a_shared_game_shows_each_art_on_at_most_its_copies() {
    let mut watch = Watch::default();
    let mut undos = 0;
    for seed in 1..=2 {
        let mut game = shared_game(seed);
        let mut view = to_human(&mut game, &mut watch);
        for n in 0..600 {
            let Some(d) = deciding(&view) else { break };
            let (id, index) = (d.id, pick(d));
            let before = view.clone();
            game.choose(game.answer(id, CandidateIndex(index)).unwrap());
            view = to_human(&mut game, &mut watch);
            if n % 10 == 9 && deciding(&view).is_some_and(|d| d.undo > 0) {
                let undone = game.undo().unwrap();
                assert_eq!(
                    printings(&undone),
                    printings(&before),
                    "seed {seed}: an undo shows the art of the decision it goes back to"
                );
                undos += 1;
                let id = deciding(&undone).unwrap().id;
                game.choose(game.answer(id, CandidateIndex(index)).unwrap());
                view = to_human(&mut game, &mut watch);
            }
        }
        let (_, replayed) = Game::from_replay(&CARDS, game.replay(), SESSION).unwrap();
        assert_eq!(
            printings(&replayed),
            printings(&view),
            "seed {seed}: a replay shows the art the game did"
        );
    }
    assert!(undos >= 5, "only {undos} undos checked");
    assert!(watch.shown > 1_000, "only {} printings seen", watch.shown);
    for name in [DANDAN, ISLAND] {
        let most = watch.most.get(name).copied().unwrap_or(0);
        assert!(most >= 2, "{name}: at most {most} seen at once");
    }
    // Hand, stack, battlefield and graveyard, or as many of them as a card of these games
    // went through.
    assert!(
        watch.longest_trip() >= 3,
        "no card kept its art through three zones"
    );
}

/// A deck with no Secret Lair printings, the WU Control mirror, shows the ordinary art on
/// every card.
#[test]
fn a_deck_without_printings_shows_none() {
    let mut game = new_game(
        3,
        Format::CONSTRUCTED,
        [owned(WU_CONTROL), owned(WU_CONTROL)],
    );
    let mut views = 0;
    for _ in 0..40 {
        let Some(view) = game.next_view() else {
            let Some(d) = game.open() else { break };
            game.choose(game.answer(d.id(), CandidateIndex(0)).unwrap());
            continue;
        };
        for v in [&view, &game.opponent_view()] {
            assert!(v.objects.iter().all(|o| o.printing.is_none()));
            views += 1;
        }
    }
    assert!(views > 10);
}

/// Clicking a stand-in twin with a different Secret Lair art trades its printing onto the
/// offered slot that the engine plays, so both seats see the chosen art land on the battlefield
/// and the other art remain in hand; undoing restores both slots' original art.
#[test]
fn clicking_a_stand_in_twin_trades_its_printing_and_undo_restores_it() {
    let mut game = shared_game(1);
    let next = |game: &mut Game| {
        let mut last = None;
        while let Some(view) = game.next_view() {
            last = Some(view);
        }
        last.expect("the game plays on from where it is")
    };
    let mut view = next(&mut game);
    let (decision_id, revision, offered, stand_in, offered_art, stand_in_art) = loop {
        let d =
            deciding(&view).expect("game reaches a priority with two differently printed Islands");
        if let Some(offered) = d.candidates.iter().find_map(|c| match c.option {
            Candidate::PlayLand { slot } => {
                let obj = view.objects.iter().find(|o| o.slot == slot)?;
                (obj.name.as_deref() == Some(ISLAND)).then_some(slot)
            }
            _ => None,
        }) {
            let offered_art = printings(&view)[&offered];
            if let Some(stand_in) = view.objects.iter().find(|o| {
                o.zone == ZoneName::Hand
                    && o.owner == Side::You
                    && o.name.as_deref() == Some(ISLAND)
                    && o.slot != offered
                    && o.printing != offered_art
            }) {
                break (
                    d.id,
                    d.prompt.revision,
                    offered,
                    stand_in.slot,
                    offered_art,
                    stand_in.printing,
                );
            }
        }
        // Answer Play/Draw or Mulligan with the first candidate (Play / Keep).
        game.choose(game.answer(d.id, CandidateIndex(0)).unwrap());
        view = next(&mut game);
    };
    assert!(offered_art.is_some() && stand_in_art.is_some());
    let reply = game
        .click(ClickRequest {
            decision: decision_id,
            revision,
            click: Click::On {
                on: Subject::Object { slot: stand_in },
            },
        })
        .unwrap();
    assert!(matches!(reply, ClickReply::Answered));

    let after = next(&mut game);
    let played = after
        .objects
        .iter()
        .find(|o| o.slot == offered)
        .expect("offered Island is on the table");
    let kept = after
        .objects
        .iter()
        .find(|o| o.slot == stand_in)
        .expect("stand-in Island stays in hand");
    assert_eq!(played.zone, ZoneName::Battlefield);
    assert_eq!(played.printing, stand_in_art);
    assert_eq!(kept.zone, ZoneName::Hand);
    assert_eq!(kept.printing, offered_art);

    let opp = game.opponent_view();
    let opp_played = opp
        .objects
        .iter()
        .find(|o| o.slot == offered)
        .expect("opponent sees the played Island");
    assert_eq!(opp_played.printing, stand_in_art);

    let undone = game.undo().unwrap();
    assert_eq!(printings(&undone)[&offered], offered_art);
    assert_eq!(printings(&undone)[&stand_in], stand_in_art);
}
