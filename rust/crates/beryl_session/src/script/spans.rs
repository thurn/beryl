//! The arrows and lines of a script (plan K23): which a table calls for, and the steps that
//! begin and end them as the builder plays its steps. The UI drew these afresh from every
//! view (`boardArrows` in `Board.tsx`), so an arrow could outlive what it pointed at; here
//! each is a span with a beginning and an end, held to the rule invariants (`check.rs`).
//!
//! An arrow ends in a beat of its own before the beat that moves either of its ends, and
//! begins with the step that calls for it, or in the first beat after one that moves an end.
//! What a table calls for ([`wanted`]) is settled when the script closes, so the spans a
//! screen holds between scripts are always its table's.

use super::build::Builder;
use super::table::{Entry, Table};
use super::{Act, Arrow, Open, Span, Step, Subject};
use crate::protocol::{
    CardHandling, Combat, ObjectView, OutcomeView, PlayerView, Side, StackItem, Status, TargetRef,
    ZoneName,
};

/// The arrows and lines `table` calls for, in the order they begin: each spell's and
/// ability's from the bottom of the stack, then combat's in the order of the battlefield.
/// `aimless`: the attackers whose line is not drawn, what they attack being not yet chosen
/// ([`crate::protocol::PlayerView::pending_attackers`]).
pub(super) fn wanted(table: &Table, aimless: &[u16]) -> Vec<Open> {
    let mut spans = aimed(table);
    spans.extend(combat(table, aimless));
    spans
}

fn permanent(table: &Table, slot: u16) -> Option<&ObjectView> {
    let object = table.objects.get(&slot);
    object.filter(|o| o.zone == ZoneName::Battlefield)
}

/// The arrows from each spell and ability on `table`'s stack, whose abilities are numbered:
/// to its targets, and from an ability to its source.
fn aimed(table: &Table) -> Vec<Open> {
    let mut spans = Vec::new();
    let mut want = |arrow, from, to| {
        let span = Open::Point { arrow, from, to };
        if !spans.contains(&span) {
            spans.push(span);
        }
    };
    let permanent = |slot: u16| permanent(table, slot);
    for entry in &table.stack {
        let from = entry.subject();
        let (targets, controller) = match &entry.item {
            StackItem::Spell { slot, targets, .. } => {
                (targets, table.objects.get(slot).map(|o| o.controller))
            }
            StackItem::Ability {
                source,
                targets,
                controller,
                ..
            } => {
                // Always shown while the ability is on the stack, thin and faint (UI
                // Conventions, "A line from an ability to its source"; Arena,
                // `IEXcjBn3BXk 20:39.3`).
                if let Some(source) = source.filter(|&slot| permanent(slot).is_some()) {
                    want(Arrow::Source, from, Subject::Object { slot: source });
                }
                (targets, Some(*controller))
            }
        };
        let Some(controller) = controller else {
            continue;
        };
        for target in targets {
            let (to, theirs) = match *target {
                TargetRef::Player { player } => (Subject::Player { player }, player),
                TargetRef::Object { slot } => {
                    // A card in a hand or a library is no longer what was aimed at.
                    let Some(object) = table
                        .objects
                        .get(&slot)
                        .filter(|o| !matches!(o.zone, ZoneName::Hand | ZoneName::Library))
                    else {
                        continue;
                    };
                    (Subject::Object { slot }, object.controller)
                }
            };
            // By relationship, the same from either seat (UI Conventions, exception 4).
            let arrow = if theirs == controller {
                Arrow::FriendlyTarget
            } else {
                Arrow::HostileTarget
            };
            want(arrow, from, to);
        }
    }
    spans
}

/// The lines of the combat `table` shows: each blocker's to its attacker, and each attacker's
/// to the planeswalker it attacks.
pub(super) fn combat(table: &Table, aimless: &[u16]) -> Vec<Open> {
    let mut spans = Vec::new();
    let mut want = |arrow, from, to| spans.push(Open::Point { arrow, from, to });
    let permanent = |slot: u16| permanent(table, slot);
    for &slot in &table.battlefield {
        let Some(object) = permanent(slot).filter(|_| !aimless.contains(&slot)) else {
            continue;
        };
        let from = Subject::Object { slot };
        match object.combat {
            // A blocker's line to its attacker, until combat damage is done (Arena,
            // `IEXcjBn3BXk 22:21.5`).
            Some(Combat::Blocking {
                attacker: Some(attacker),
            }) if permanent(attacker)
                .is_some_and(|o| matches!(o.combat, Some(Combat::Attacking { .. }))) =>
            {
                want(Arrow::Block, from, Subject::Object { slot: attacker });
            }
            // An attacker's line only to the planeswalker it attacks: Arena draws a thin
            // red line to an attacked planeswalker (`UrGalw_f35w 48:56.6`) and none to an
            // attacked player, whose attacker has only its chevron (`IEXcjBn3BXk 22:16.53`).
            // A blocked attacker's line is its blocker's. One whose planeswalker left never
            // swings to the player (by-2wsl).
            Some(Combat::Attacking {
                planeswalker: Some(walker),
                target_departed,
                blocked: false,
            }) if target_departed != Some(true) && permanent(walker).is_some() => {
                want(Arrow::Attack, from, Subject::Object { slot: walker });
            }
            _ => {}
        }
    }
    spans
}

/// The one beat that shows `spans` at once: a reset's.
pub(super) fn shown(spans: &[Open]) -> Vec<Vec<Step>> {
    let begin = |span: &Open| Step {
        act: span.act(Span::Begin),
        cause: None,
    };
    let beat: Vec<Step> = spans.iter().map(begin).collect();
    if beat.is_empty() {
        Vec::new()
    } else {
        vec![beat]
    }
}

impl Builder<'_> {
    /// Writes a step that begins or ends `span`, which changes no table, into the open beat.
    fn span(&mut self, end: Span, span: Open) {
        let act = span.act(end);
        let beat = self.beat();
        self.beats[beat].push(Step { act, cause: None });
        match end {
            Span::Begin => {
                self.spans.push(span);
                self.begun.push(span);
            }
            Span::End => {
                self.spans.retain(|&open| open != span);
                self.ended.push(span);
            }
        }
    }

    /// Before a step moves `subject`: every span on it ends, in a beat of their own before
    /// the step's beat, and the step starts a new beat if this one has shown `subject` moving
    /// or a span on it beginning or ending. `gone`: the step takes `subject` to another
    /// zone, or is the doing (`cause`) of the spell or ability that aims at it. An arrow so
    /// ended is not drawn again: what moved zones is a new object
    /// ([CR 400.7](https://yawgatog.com/resources/magic-rules/#R4007): "An object that moves
    /// from one zone to another becomes a new object with no memory of, or relation to,
    /// its previous existence"), and a spell that has done its work on its target is done
    /// pointing at it.
    pub(super) fn part(&mut self, subject: Subject, gone: bool, cause: Option<Subject>) {
        let on = |spans: &[Open]| spans.iter().any(|span| span.touches(subject));
        if self.open && (self.moved.contains(&subject) || on(&self.begun) || on(&self.ended)) {
            self.open = false;
        }
        let beat = self.beat();
        let (touching, rest): (Vec<Open>, Vec<Open>) =
            self.spans.iter().partition(|span| span.touches(subject));
        self.spans = rest;
        for span in touching {
            self.ends[beat].push(Step {
                act: span.act(Span::End),
                cause: None,
            });
            if let Open::Point { from, to, .. } = span
                && to == subject
                && (gone || cause == Some(from))
            {
                self.broken.push((from, to));
            }
        }
    }

    /// The arrows and lines are the table's: one it no longer calls for ends, and one it
    /// calls for begins, unless an end of it moved in this beat (it then begins in a later
    /// one) or it was ended for good. A line of combat begins as its declaration's event
    /// sets the records it joins (`attack.rs`), so a combat that begins and ends in one
    /// script shows its lines.
    pub(super) fn sync(&mut self) {
        let wanted = wanted(&self.table, &self.aimless);
        let arrow = |span: &&Open| matches!(span, Open::Point { .. });
        let stale: Vec<Open> = self.spans.iter().filter(arrow).copied().collect();
        for span in stale {
            if !wanted.contains(&span) {
                self.span(Span::End, span);
            }
        }
        for span in wanted {
            let Open::Point { from, to, .. } = span else {
                continue;
            };
            // A spell or ability going on the stack takes its arrows with it.
            let leaves = self.moved.contains(&from) && !self.landed.contains(&from);
            if !self.spans.contains(&span)
                && !self.broken.contains(&(from, to))
                && !leaves
                && !self.moved.contains(&to)
            {
                self.span(Span::Begin, span);
            }
        }
    }

    /// The opponent's handling of cards is the view's: a lift that is over ends as the script
    /// begins, before what the look did shows.
    pub(super) fn lower(&mut self, handling: Option<CardHandling>) {
        let lifted =
            self.spans.iter().copied().find(
                |span| matches!(*span, Open::Lift { handling: held } if Some(held) != handling),
            );
        if let Some(span) = lifted {
            self.open = false;
            self.span(Span::End, span);
            self.open = false;
        }
    }

    /// Closes the script's spans, in a last beat: the arrows and lines its table calls for,
    /// and the lift of the cards the opponent is now handling. A view that ends the game
    /// then leaves the table to be seen, in a beat of its own, before the result covers it:
    /// a `dwell` on the player who lost, or on the viewer where no one did.
    pub(super) fn close(&mut self, view: &PlayerView) {
        self.open = false;
        // The records are the view's now: only its own aimless attackers have no line.
        self.aimless = self.pending.to_vec();
        self.sync();
        if let Some(handling) = view.handling {
            let span = Open::Lift { handling };
            if !self.spans.contains(&span) {
                self.span(Span::Begin, span);
            }
        }
        let stack: Vec<Subject> = self.table.stack.iter().map(Entry::subject).collect();
        self.broken.retain(|(from, _)| stack.contains(from));
        if let Status::Over { outcome } = &view.status {
            let player = match outcome {
                OutcomeView::Win {
                    winner: Side::You, ..
                } => Side::Opponent,
                _ => Side::You,
            };
            self.open = false;
            self.emit(
                Act::Dwell {
                    subject: Subject::Player { player },
                },
                None,
            );
            self.open = false;
        }
    }

    /// The script's beats: each beat of steps after the beat of the spans that end before it.
    pub(super) fn beats(&mut self) -> Vec<Vec<Step>> {
        let ends = std::mem::take(&mut self.ends);
        let beats = std::mem::take(&mut self.beats);
        ends.into_iter()
            .zip(beats)
            .flat_map(|(ends, beat)| [ends, beat])
            .filter(|beat| !beat.is_empty())
            .collect()
    }
}
