//! Templates for instructions that move objects between zones by naming where they go: into
//! a library, onto the battlefield, and whose zone that is (CR 400.3).

use super::Ctx;
use super::words::ordinal;
use crate::effect::{LibraryPosition, Objects, Players, PutOnto};

impl Ctx {
    /// "Put [objects] into their owners' libraries [position]" (CR 400.3).
    pub(super) fn put_into_library(
        &mut self,
        objects: Objects,
        position: LibraryPosition,
    ) -> Vec<String> {
        let plural = self.is_plural(objects);
        let library = owners(plural, "library");
        match position {
            LibraryPosition::Top => {
                vec![self.acts(("put", "puts"), objects, &format!(" on top of {library}"))]
            }
            LibraryPosition::Bottom => vec![self.acts(
                ("put", "puts"),
                objects,
                &format!(" on the bottom of {library}"),
            )],
            LibraryPosition::FromTop(n) => vec![self.acts(
                ("put", "puts"),
                objects,
                &format!(" into {library} {} from the top", ordinal(n)),
            )],
            LibraryPosition::TopOrBottom => {
                // "Choose up to one target creature spell. Its owner puts it on their choice
                // of the top or bottom of their library": the target is said first.
                let mut clauses = Vec::new();
                let what = self.objects(objects);
                let (whose, it) = if matches!(what.as_str(), "it" | "them") {
                    (if plural { "their owners" } else { "its owner" }, what)
                } else if matches!(objects, Objects::Target(_)) {
                    clauses.push(format!("choose {what}"));
                    ("its owner", "it".to_owned())
                } else {
                    (if plural { "their owners" } else { "its owner" }, what)
                };
                let puts = if plural { "put" } else { "puts" };
                clauses.push(format!(
                    "{whose} {puts} {it} on their choice of the top or bottom of their library"
                ));
                self.plural = plural;
                clauses
            }
        }
    }

    /// "Return enchanted creature card to the battlefield under your control", for a
    /// [`PutOnto`]. A card "returns" to the battlefield from a graveyard or from exile.
    pub(super) fn put_onto(&mut self, onto: &PutOnto) -> String {
        let plural = self.is_plural(onto.objects);
        let control = match onto.under {
            None => owners(plural, "control"),
            Some(player) => {
                let who = self.player(player);
                if who == "you" {
                    "your control".to_owned()
                } else {
                    format!("{} control", super::abilities::possessive(&who))
                }
            }
        };
        let tapped = if onto.tapped { " tapped" } else { "" };
        let what = self.moved_card(onto.objects);
        let rest = format!(" to the battlefield{tapped} under {control}");
        self.discarded = None;
        self.those_named = None;
        let clause = self.clause(Players::You, ("return", "returns"), |_| {
            format!("{what}{rest}")
        });
        self.plural = plural;
        clause
    }
}

/// "Its owner's hand", or "their owners' hands" for several (CR 400.3).
pub(super) fn owners(plural: bool, zone: &str) -> String {
    if plural {
        let zones = if zone == "control" {
            "control".to_owned()
        } else {
            super::words::plural(zone)
        };
        format!("their owners' {zones}")
    } else {
        format!("its owner's {zone}")
    }
}
