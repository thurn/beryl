use beryl_table::action::{
    Adjustment, ComputedDestination, Destination, Flow, Portion, Position, SeenObject, Selection,
    SortBy, Target,
};
use beryl_table::state::{Assist, Card, Face, ObjectId, Phase, Row, Zone};
use beryl_table::{Action, Chance, Seat, Table};

/// Each template derives its ids and counts from the current table. The last
/// five templates belong to the assist and log layers. Callers can choose
/// whether their entry point supports those actions.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    Move,
    Reorder,
    Pin,
    Give,
    Tap,
    Face,
    Flag,
    Peek,
    Counter,
    Stats,
    Damage,
    Note,
    Attach,
    Unattach,
    Clone,
    Token,
    Marker,
    Remove,
    Draw,
    Mulligan,
    Shuffle,
    Look,
    Reveal,
    Lend,
    AlwaysRevealTop,
    AlwaysLookAtTop,
    SortHand,
    Life,
    PlayerCounter,
    Phase,
    NextTurn,
    Roll,
    Arrow,
    ClearArrows,
    LoadDeck,
    Sideboard,
    Ready,
    Start,
    NextGame,
    Concede,
    SetAssist,
    Flow,
    Undo,
    Rewind,
    Batch,
}

pub const CORE: &[Kind] = &[
    Kind::Move,
    Kind::Reorder,
    Kind::Pin,
    Kind::Give,
    Kind::Tap,
    Kind::Face,
    Kind::Flag,
    Kind::Peek,
    Kind::Counter,
    Kind::Stats,
    Kind::Damage,
    Kind::Note,
    Kind::Attach,
    Kind::Unattach,
    Kind::Clone,
    Kind::Token,
    Kind::Marker,
    Kind::Remove,
    Kind::Draw,
    Kind::Mulligan,
    Kind::Shuffle,
    Kind::Look,
    Kind::Reveal,
    Kind::Lend,
    Kind::AlwaysRevealTop,
    Kind::AlwaysLookAtTop,
    Kind::SortHand,
    Kind::Life,
    Kind::PlayerCounter,
    Kind::Phase,
    Kind::NextTurn,
    Kind::Roll,
    Kind::Arrow,
    Kind::ClearArrows,
    Kind::LoadDeck,
    Kind::Sideboard,
    Kind::Ready,
    Kind::Start,
    Kind::NextGame,
    Kind::Concede,
];

pub const ASSIST_AND_LOG: &[Kind] = &[
    Kind::SetAssist,
    Kind::Flow,
    Kind::Undo,
    Kind::Rewind,
    Kind::Batch,
];

pub fn card_name(index: u32) -> String {
    format!("<test-card-{index:06}>")
}

pub struct Generator {
    chance: Chance,
    next_name: u32,
}

impl Generator {
    pub fn new(seed: u64) -> Self {
        Self {
            chance: Chance::fresh(seed),
            next_name: 0,
        }
    }

    pub fn next(&mut self, table: &Table, kinds: &[Kind]) -> (Kind, Seat, Action) {
        loop {
            let kind = kinds[self.index(kinds.len())];
            let by = Seat(u8::from(self.coin()));
            if let Some(action) = self.action(table, kind, by) {
                return (kind, by, action);
            }
        }
    }

    pub fn action(&mut self, table: &Table, kind: Kind, by: Seat) -> Option<Action> {
        let toggle = self.coin();
        match kind {
            Kind::Move
            | Kind::Reorder
            | Kind::Pin
            | Kind::Give
            | Kind::Tap
            | Kind::Face
            | Kind::Flag
            | Kind::Peek
            | Kind::Counter
            | Kind::Stats
            | Kind::Damage
            | Kind::Note => self.object_action(table, kind, by, toggle),
            Kind::Attach
            | Kind::Unattach
            | Kind::Clone
            | Kind::Token
            | Kind::Marker
            | Kind::Remove => self.battlefield_action(table, kind, by, toggle),
            Kind::Draw
            | Kind::Mulligan
            | Kind::Shuffle
            | Kind::Look
            | Kind::Reveal
            | Kind::Lend
            | Kind::AlwaysRevealTop
            | Kind::AlwaysLookAtTop
            | Kind::SortHand
            | Kind::Life
            | Kind::PlayerCounter
            | Kind::Phase
            | Kind::NextTurn
            | Kind::Roll
            | Kind::Arrow
            | Kind::ClearArrows => self.play_action(table, kind, by, toggle),
            Kind::LoadDeck
            | Kind::Sideboard
            | Kind::Ready
            | Kind::Start
            | Kind::NextGame
            | Kind::Concede
            | Kind::SetAssist
            | Kind::Flow
            | Kind::Undo
            | Kind::Rewind
            | Kind::Batch => self.match_action(table, kind, by, toggle),
        }
    }

    fn object_action(
        &mut self,
        table: &Table,
        kind: Kind,
        by: Seat,
        toggle: bool,
    ) -> Option<Action> {
        let hand = Zone::Hand(by);
        let battlefield = Zone::Battlefield;
        let other = Seat(1 - by.0);
        let adjust = Adjustment::Add(if toggle { 1 } else { -1 });
        Some(match kind {
            Kind::Move => return self.move_card(table, by),
            Kind::Reorder => Action::Reorder {
                selection: self.select(table, if toggle { hand } else { battlefield }, |_| true)?,
                position: if toggle {
                    Position::Top
                } else {
                    Position::Bottom
                },
            },
            Kind::Pin => Action::Pin {
                selection: self.select(table, battlefield, |_| true)?,
                row: if toggle { Row::Auto } else { Row::Others },
            },
            Kind::Give => Action::Give {
                selection: self.select(table, battlefield, |_| true)?,
                to: other,
            },
            Kind::Tap => Action::Tap {
                selection: self.select(table, battlefield, |_| true)?,
                tapped: toggle,
            },
            Kind::Face => Action::Face {
                selection: self.select(table, battlefield, |_| true)?,
                face: if toggle { Face::Front } else { Face::Down },
            },
            Kind::Flag => Action::Flag {
                selection: self.select(table, battlefield, |_| true)?,
                doesnt_untap: toggle,
            },
            Kind::Peek => {
                let selection = self.select(table, battlefield, |id| {
                    let card = &table.objects[&id];
                    card.face == Face::Down && card.controller == by
                })?;
                Action::Peek { selection }
            }
            Kind::Counter => Action::Counter {
                selection: self.select(table, battlefield, |_| true)?,
                name: "+1/+1".into(),
                adjustment: adjust,
            },
            Kind::Stats => Action::Stats {
                selection: self.select(table, battlefield, |_| true)?,
                stats: toggle.then_some((4, 4)),
            },
            Kind::Damage => Action::Damage {
                selection: self.select(table, battlefield, |_| true)?,
                damage: if toggle { 2 } else { 0 },
            },
            Kind::Note => Action::Note {
                selection: self.select(table, if toggle { hand } else { battlefield }, |_| true)?,
                note: toggle.then(|| "remember the next upkeep".into()),
            },
            _ => unreachable!("wrong action group"),
        })
    }

    fn battlefield_action(
        &mut self,
        table: &Table,
        kind: Kind,
        by: Seat,
        toggle: bool,
    ) -> Option<Action> {
        let hand = Zone::Hand(by);
        let battlefield = Zone::Battlefield;
        Some(match kind {
            Kind::Attach => {
                let host = *self.pick(&table.battlefield)?;
                let zone = if toggle { hand } else { battlefield };
                Action::Attach {
                    selection: self.select(table, zone, |id| {
                        id != host
                            && table.objects[&id].attached_to.is_none()
                            && !table
                                .objects
                                .values()
                                .any(|card| card.attached_to == Some(id))
                    })?,
                    host: SeenObject {
                        id: host,
                        zone: battlefield,
                    },
                }
            }
            Kind::Unattach => Action::Unattach {
                selection: self.select(table, battlefield, |id| {
                    table.objects[&id].attached_to.is_some()
                })?,
            },
            Kind::Clone => Action::Clone {
                selection: self.select(table, battlefield, |id| !table.objects[&id].token)?,
            },
            Kind::Token => Action::Token {
                card: Card::Custom {
                    name: self.fresh_name(),
                    types: "Creature".into(),
                    colors: Vec::new(),
                    stats: Some((1, 1)),
                },
                count: 1,
                attached_to: None,
                note: None,
            },
            Kind::Marker => Action::Marker {
                selection: self.select(table, battlefield, |_| true)?,
                text: "resolve the ability".into(),
            },
            Kind::Remove => {
                let zone = if toggle { Zone::Stack } else { battlefield };
                Action::Remove {
                    selection: self.select(table, zone, |id| {
                        table.objects.get(&id).is_none_or(|card| card.token)
                    })?,
                }
            }
            _ => unreachable!("wrong action group"),
        })
    }

    fn play_action(&mut self, table: &Table, kind: Kind, by: Seat, toggle: bool) -> Option<Action> {
        let hand = Zone::Hand(by);
        let battlefield = Zone::Battlefield;
        let library = Zone::Library(by);
        let other = Seat(1 - by.0);
        let adjust = Adjustment::Add(if toggle { 1 } else { -1 });
        Some(match kind {
            Kind::Draw => Action::Draw { seat: by, count: 1 },
            Kind::Mulligan => Action::Mulligan { seat: by, count: 2 },
            Kind::Shuffle => Action::Shuffle {
                seat: by,
                portion: self.portion(),
            },
            Kind::Look => Action::Look {
                seat: by,
                portion: self.portion(),
            },
            Kind::Reveal => Action::Reveal {
                zone: if toggle { hand } else { library },
                objects: None,
                random: toggle.then_some(1),
                to: vec![other],
            },
            Kind::Lend => Action::Lend {
                zone: library,
                to: other,
            },
            Kind::AlwaysRevealTop => Action::AlwaysRevealTop {
                seat: by,
                on: toggle,
            },
            Kind::AlwaysLookAtTop => Action::AlwaysLookAtTop {
                seat: by,
                on: toggle,
            },
            Kind::SortHand => Action::SortHand {
                seat: by,
                by: [SortBy::Name, SortBy::Type, SortBy::ManaValue][self.index(3)],
            },
            Kind::Life => Action::Life {
                seat: other,
                adjustment: adjust,
            },
            Kind::PlayerCounter => Action::PlayerCounter {
                seat: by,
                name: "energy".into(),
                adjustment: adjust,
            },
            Kind::Phase => Action::Phase {
                phase: if toggle {
                    Phase::DeclareAttackers
                } else {
                    Phase::FirstMain
                },
            },
            Kind::NextTurn => Action::NextTurn {},
            Kind::Roll => Action::Roll {
                count: if toggle { 2 } else { 1 },
                sides: 6,
            },
            Kind::Arrow => Action::Arrow {
                selection: self.select(table, battlefield, |_| true)?,
                to: Target::Player(other),
            },
            Kind::ClearArrows => Action::ClearArrows {
                from: None,
                to: None,
            },
            _ => unreachable!("wrong action group"),
        })
    }

    fn match_action(
        &mut self,
        table: &Table,
        kind: Kind,
        by: Seat,
        toggle: bool,
    ) -> Option<Action> {
        let lobby = !table.game_match.in_game;
        let loaded = &table.game_match.seats[usize::from(by.0)];
        let adjust = Adjustment::Add(if toggle { 1 } else { -1 });
        Some(match kind {
            Kind::LoadDeck if lobby => Action::LoadDeck {
                seat: by,
                main: (0..8).map(|_| (self.fresh_name(), 1)).collect(),
                side: (0..2).map(|_| (self.fresh_name(), 1)).collect(),
            },
            Kind::Sideboard if lobby => Action::Sideboard {
                seat: by,
                into_main: vec![(loaded.sideboard.first()?.0.clone(), 1)],
                into_side: vec![(loaded.deck.first()?.0.clone(), 1)],
            },
            Kind::Ready if lobby => Action::Ready {
                seat: by,
                ready: toggle,
            },
            Kind::Start
                if lobby
                    && table
                        .game_match
                        .seats
                        .iter()
                        .all(|seat| !seat.deck.is_empty()) =>
            {
                Action::Start { first: by }
            }
            Kind::NextGame if !lobby => Action::NextGame {
                winner: toggle.then_some(by),
            },
            Kind::Concede if !lobby => Action::Concede {},
            Kind::SetAssist => Action::SetAssist {
                assist: Assist::Combat,
                on: toggle,
            },
            Kind::Flow if !table.zones[usize::from(by.0)].library.is_empty() => Action::Flow {
                seat: by,
                flow: Flow::Scry,
                count: 1,
            },
            Kind::Undo => Action::Undo { entry: None },
            Kind::Rewind => Action::Rewind { to: 0 },
            Kind::Batch => Action::Batch {
                actions: vec![
                    Action::Life {
                        seat: by,
                        adjustment: adjust,
                    },
                    Action::NextTurn {},
                ],
            },
            Kind::LoadDeck
            | Kind::Sideboard
            | Kind::Ready
            | Kind::Start
            | Kind::NextGame
            | Kind::Concede
            | Kind::Flow => return None,
            _ => unreachable!("wrong action group"),
        })
    }

    fn move_card(&mut self, table: &Table, by: Seat) -> Option<Action> {
        let hand = Zone::Hand(by);
        let library = Zone::Library(by);
        let battlefield = Zone::Battlefield;
        let graveyard = Zone::Graveyard(by);
        let (from, to, face) = *self.pick(&[
            (library, Destination::Zone(battlefield), Some(Face::Down)),
            (hand, Destination::Zone(battlefield), None),
            (hand, Destination::Computed(ComputedDestination::Play), None),
            (
                Zone::Stack,
                Destination::Computed(ComputedDestination::Resolve),
                None,
            ),
            (battlefield, Destination::Zone(graveyard), None),
            (graveyard, Destination::Zone(hand), None),
            (graveyard, Destination::Zone(library), None),
        ])?;
        let selection = self.select(table, from, |id| {
            from == Zone::Stack || table.objects[&id].owner == by
        })?;
        Some(Action::Move {
            selection,
            to,
            position: Position::Top,
            face,
            tapped: None,
        })
    }

    fn select(
        &mut self,
        table: &Table,
        zone: Zone,
        eligible: impl Fn(ObjectId) -> bool,
    ) -> Option<Selection> {
        let mut ids: Vec<_> = table
            .zone(zone)?
            .iter()
            .copied()
            .filter(|id| eligible(*id))
            .collect();
        let index = self.index(ids.len().max(1));
        if ids.is_empty() {
            return None;
        }
        let first = ids.swap_remove(index);
        let mut selection = Selection {
            objects: vec![first],
            zone,
        };
        if self.coin()
            && let Some(second) = self.pick(&ids)
        {
            selection.objects.push(*second);
        }
        Some(selection)
    }

    fn portion(&mut self) -> Portion {
        [Portion::All, Portion::Top(2), Portion::Bottom(3)][self.index(3)]
    }

    fn pick<'a, T>(&mut self, items: &'a [T]) -> Option<&'a T> {
        items.get(self.index(items.len().max(1)))
    }

    fn coin(&mut self) -> bool {
        self.index(2) == 0
    }

    fn index(&mut self, len: usize) -> usize {
        self.chance.below(u32::try_from(len).unwrap()).unwrap() as usize
    }

    fn fresh_name(&mut self) -> String {
        self.next_name += 1;
        card_name(self.next_name)
    }
}
