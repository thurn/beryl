//! Templates for instructions that move objects between zones by naming where they go: into
//! a library, onto the battlefield, and whose zone that is (CR 400.3).

use super::Ctx;
use super::filters::{Kind, zone_name};
use super::words::ordinal;
use crate::effect::{Effect, LibraryPosition, Objects, Players, PutOnto};
use crate::ids::ZoneSet;

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

    /// "Each player may put an artifact card from their hand onto the battlefield": each
    /// player's hidden pick of a card of their zone (CR 101.4a), and then the cards picked put
    /// onto the battlefield under their owners' control, said as one instruction. `None` for
    /// any other pair.
    pub(super) fn hidden_put(&mut self, effect: &Effect, next: &Effect) -> Option<String> {
        let (
            &Effect::ForEachPlayer {
                players,
                effect:
                    &Effect::Choose {
                        player,
                        among:
                            Objects::InZoneOf {
                                zone,
                                players: whose,
                                filter,
                            },
                        hidden: true,
                        ..
                    },
            },
            &Effect::PutOntoBattlefield(&PutOnto {
                objects: Objects::Those,
                under: None,
                tapped,
                linked: false,
            }),
        ) = (effect, next)
        else {
            return None;
        };
        let saved = self.group.replace(players);
        self.may = true;
        let tapped = if tapped { " tapped" } else { "" };
        let clause = self.clause(player, ("put", "puts"), |c| {
            let owner = c.possessive(*whose);
            let noun = c
                .noun(filter, Kind::Card(None))
                .indefinite(Kind::Card(None));
            format!(
                "{noun} from {owner} {} onto the battlefield{tapped}",
                zone_name(zone)
            )
        });
        self.group = saved;
        Some(clause)
    }

    /// "Return enchanted creature card to the battlefield under your control", for a
    /// [`PutOnto`]. A card "returns" to the battlefield from a graveyard or from exile.
    pub(super) fn put_onto(&mut self, onto: &PutOnto) -> String {
        let tapped = if onto.tapped { " tapped" } else { "" };
        if onto.objects == Objects::This
            && onto.under.is_none()
            && let Some(ability) = self.ability
        {
            let from = match ability.zones() {
                ZoneSet::GRAVEYARD => Some("your graveyard"),
                zones if zones == ZoneSet::GRAVEYARD.union(ZoneSet::EXILE) => {
                    Some("your graveyard or from exile")
                }
                _ => None,
            };
            if let Some(from) = from {
                return format!("return this card from {from} to the battlefield{tapped}");
            }
        }
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
