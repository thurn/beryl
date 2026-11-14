use super::{Action, SeenObject, Selection, Target};
use crate::state::Zone;

impl<Id> Action<Id> {
    /// Translates object references, including selections, targets and nested batches.
    /// The resolver receives the zone specified in the action for each reference.
    #[expect(
        clippy::too_many_lines,
        reason = "every action must account for each field when changing the reference type"
    )]
    pub fn try_map_objects<Mapped, Error>(
        self,
        resolve: &mut impl FnMut(Id, Zone) -> Result<Mapped, Error>,
    ) -> Result<Action<Mapped>, Error> {
        Ok(match self {
            Self::Move {
                selection,
                to,
                position,
                face,
                tapped,
            } => Action::Move {
                selection: selection.try_map(resolve)?,
                to,
                position,
                face,
                tapped,
            },
            Self::Reorder {
                selection,
                position,
            } => Action::Reorder {
                selection: selection.try_map(resolve)?,
                position,
            },
            Self::Pin { selection, row } => Action::Pin {
                selection: selection.try_map(resolve)?,
                row,
            },
            Self::Give { selection, to } => Action::Give {
                selection: selection.try_map(resolve)?,
                to,
            },
            Self::Tap { selection, tapped } => Action::Tap {
                selection: selection.try_map(resolve)?,
                tapped,
            },
            Self::Face { selection, face } => Action::Face {
                selection: selection.try_map(resolve)?,
                face,
            },
            Self::Flag {
                selection,
                doesnt_untap,
            } => Action::Flag {
                selection: selection.try_map(resolve)?,
                doesnt_untap,
            },
            Self::Peek { selection } => Action::Peek {
                selection: selection.try_map(resolve)?,
            },
            Self::Counter {
                selection,
                name,
                adjustment,
            } => Action::Counter {
                selection: selection.try_map(resolve)?,
                name,
                adjustment,
            },
            Self::Stats { selection, stats } => Action::Stats {
                selection: selection.try_map(resolve)?,
                stats,
            },
            Self::Damage { selection, damage } => Action::Damage {
                selection: selection.try_map(resolve)?,
                damage,
            },
            Self::Note { selection, note } => Action::Note {
                selection: selection.try_map(resolve)?,
                note,
            },
            Self::Attach { selection, host } => Action::Attach {
                selection: selection.try_map(resolve)?,
                host: host.try_map(resolve)?,
            },
            Self::Unattach { selection } => Action::Unattach {
                selection: selection.try_map(resolve)?,
            },
            Self::Clone { selection } => Action::Clone {
                selection: selection.try_map(resolve)?,
            },
            Self::Token {
                card,
                count,
                attached_to,
                note,
            } => Action::Token {
                card,
                count,
                attached_to: attached_to
                    .map(|object| object.try_map(resolve))
                    .transpose()?,
                note,
            },
            Self::Marker { selection, text } => Action::Marker {
                selection: selection.try_map(resolve)?,
                text,
            },
            Self::Remove { selection } => Action::Remove {
                selection: selection.try_map(resolve)?,
            },
            Self::Draw { seat, count } => Action::Draw { seat, count },
            Self::Mulligan { seat, count } => Action::Mulligan { seat, count },
            Self::Shuffle { seat, portion } => Action::Shuffle { seat, portion },
            Self::Look { seat, portion } => Action::Look { seat, portion },
            Self::CloseLook {} => Action::CloseLook {},
            Self::Reveal {
                zone,
                objects,
                random,
                to,
            } => Action::Reveal {
                zone,
                objects: objects
                    .map(|ids| resolve_all(ids, zone, resolve))
                    .transpose()?,
                random,
                to,
            },
            Self::Lend { zone, to } => Action::Lend { zone, to },
            Self::AlwaysRevealTop { seat, on } => Action::AlwaysRevealTop { seat, on },
            Self::AlwaysLookAtTop { seat, on } => Action::AlwaysLookAtTop { seat, on },
            Self::SortHand { seat, by } => Action::SortHand { seat, by },
            Self::Life { seat, adjustment } => Action::Life { seat, adjustment },
            Self::PlayerCounter {
                seat,
                name,
                adjustment,
            } => Action::PlayerCounter {
                seat,
                name,
                adjustment,
            },
            Self::Phase { phase } => Action::Phase { phase },
            Self::NextTurn {} => Action::NextTurn {},
            Self::Roll { count, sides } => Action::Roll { count, sides },
            Self::Arrow { selection, to } => Action::Arrow {
                selection: selection.try_map(resolve)?,
                to: to.try_map(resolve)?,
            },
            Self::ClearArrows { from, to } => Action::ClearArrows {
                from: from.map(|object| object.try_map(resolve)).transpose()?,
                to: to.map(|target| target.try_map(resolve)).transpose()?,
            },
            Self::LoadDeck { seat, main, side } => Action::LoadDeck { seat, main, side },
            Self::Sideboard {
                seat,
                into_main,
                into_side,
            } => Action::Sideboard {
                seat,
                into_main,
                into_side,
            },
            Self::Ready { seat, ready } => Action::Ready { seat, ready },
            Self::Start { first } => Action::Start { first },
            Self::NextGame { winner } => Action::NextGame { winner },
            Self::Concede {} => Action::Concede {},
            Self::SetAssist { assist, on } => Action::SetAssist { assist, on },
            Self::Flow { seat, flow, count } => Action::Flow { seat, flow, count },
            Self::Undo { entry } => Action::Undo { entry },
            Self::Rewind { to } => Action::Rewind { to },
            Self::Batch { actions } => Action::Batch {
                actions: actions
                    .into_iter()
                    .map(|action| action.try_map_objects(resolve))
                    .collect::<Result<_, _>>()?,
            },
        })
    }
}

impl<Id> Selection<Id> {
    fn try_map<Mapped, Error>(
        self,
        resolve: &mut impl FnMut(Id, Zone) -> Result<Mapped, Error>,
    ) -> Result<Selection<Mapped>, Error> {
        Ok(Selection {
            objects: resolve_all(self.objects, self.zone, resolve)?,
            zone: self.zone,
        })
    }
}

impl<Id> SeenObject<Id> {
    fn try_map<Mapped, Error>(
        self,
        resolve: &mut impl FnMut(Id, Zone) -> Result<Mapped, Error>,
    ) -> Result<SeenObject<Mapped>, Error> {
        Ok(SeenObject {
            id: resolve(self.id, self.zone)?,
            zone: self.zone,
        })
    }
}

impl<Id> Target<Id> {
    fn try_map<Mapped, Error>(
        self,
        resolve: &mut impl FnMut(Id, Zone) -> Result<Mapped, Error>,
    ) -> Result<Target<Mapped>, Error> {
        Ok(match self {
            Self::Object(seen) => Target::Object(seen.try_map(resolve)?),
            Self::Player(seat) => Target::Player(seat),
        })
    }
}

fn resolve_all<Id, Mapped, Error>(
    ids: Vec<Id>,
    zone: Zone,
    resolve: &mut impl FnMut(Id, Zone) -> Result<Mapped, Error>,
) -> Result<Vec<Mapped>, Error> {
    ids.into_iter().map(|id| resolve(id, zone)).collect()
}
