//! Templates for instructions that move an object by what happens to it: it is countered,
//! destroyed, returned, put into a graveyard, or found by a search of a library or outside
//! the game. Where it goes is named by `zones.rs`'s words for a zone and its owner.

use super::Ctx;
use super::filters::{Kind, zone_name};
use super::words::with_article;
use super::zones::owners;
use crate::effect::{Amount, Found, Objects, Players, Regeneration};
use crate::filter::Filter;
use crate::ids::Zone;

impl Ctx {
    /// "Counter target spell. If that spell is countered this way, exile it instead of
    /// putting it into its owner's graveyard." (Or "put it on top of its owner's library
    /// instead of into that player's graveyard.")
    pub(super) fn counter_into(&mut self, spells: Objects, zone: Zone) -> Vec<String> {
        let counter = self.acts(("counter", "counters"), spells, "");
        let second = match zone {
            Zone::Exile => {
                "if that spell is countered this way, exile it instead of putting it into its \
                 owner's graveyard"
                    .to_owned()
            }
            Zone::Library => {
                "if that spell is countered this way, put it on top of its owner's library \
                 instead of into that player's graveyard"
                    .to_owned()
            }
            other @ (Zone::Hand
            | Zone::Battlefield
            | Zone::Graveyard
            | Zone::Stack
            | Zone::Nowhere
            | Zone::Merged) => format!(
                "if that spell is countered this way, put it into its owner's {} instead of \
                 putting it into its owner's graveyard",
                zone_name(other)
            ),
        };
        vec![counter, second]
    }

    /// "Destroy target creature", with "It can't be regenerated" when no shield applies.
    pub(super) fn destroy(&mut self, objects: Objects, regeneration: Regeneration) -> Vec<String> {
        let destroy = self.acts(("destroy", "destroys"), objects, "");
        match regeneration {
            Regeneration::Allowed => vec![destroy],
            Regeneration::Forbidden => vec![destroy, self.not_regenerated(objects)],
        }
    }

    /// "It can't be regenerated", or "They can't be regenerated" after several
    /// ([CR 701.19c](https://yawgatog.com/resources/magic-rules/#R70119c)). An overload card's
    /// text serves both of its spells, so it names what was destroyed instead of using a
    /// pronoun that "each" would make wrong: "A creature destroyed this way can't be
    /// regenerated."
    fn not_regenerated(&mut self, objects: Objects) -> String {
        let overloaded = self.face.is_some_and(|face| {
            face.casting
                .iter()
                .any(|option| matches!(option, crate::CastingOption::Overload { .. }))
        });
        if overloaded
            && let Objects::Target(n) = objects
            && let Some(&spec) = self.targets.get(n.index())
        {
            let noun = self.target_noun(spec);
            return format!(
                "{} destroyed this way can't be regenerated",
                with_article(&noun)
            );
        }
        let they = if self.is_plural(objects) {
            "they"
        } else {
            "it"
        };
        format!("{they} can't be regenerated")
    }

    /// "Return target creature to its owner's hand".
    pub(super) fn return_to_hand(&mut self, objects: Objects) -> Vec<String> {
        // "Return target land card from your graveyard to your hand": the cards a target in
        // its controller's own graveyard names are theirs.
        if self.own_graveyard_target(objects) {
            return vec![self.acts(("return", "returns"), objects, " to your hand")];
        }
        let to = owners(self.is_plural(objects), "hand");
        vec![self.acts(("return", "returns"), objects, &format!(" to {to}"))]
    }

    /// "Return that card to the battlefield under its owner's control", and "transformed"
    /// when it returns with its back face up.
    pub(super) fn return_to_battlefield(
        &mut self,
        objects: Objects,
        transformed: bool,
    ) -> Vec<String> {
        // "Return this card from your graveyard to the battlefield": a card goes to its
        // owner's graveyard (CR 404.1), and a triggered ability of an object with no
        // controller is its owner's (CR 113.8), so "your" names the card's owner, under whose
        // control it returns.
        if objects == Objects::This && self.in_graveyard && !transformed {
            let rest = " from your graveyard to the battlefield";
            return vec![self.acts(("return", "returns"), objects, rest)];
        }
        let how = if transformed { " transformed" } else { "" };
        // "Return target creature card from your graveyard to the battlefield": a permanent
        // enters under the control of the player who put it there unless the effect says
        // otherwise (CR 110.2), and a card from your own graveyard is yours.
        if self.own_graveyard_target(objects) {
            let rest = format!(" to the battlefield{how}");
            return vec![self.acts(("return", "returns"), objects, &rest)];
        }
        let control = owners(self.is_plural(objects), "control");
        let rest = format!(" to the battlefield{how} under {control}");
        vec![self.acts(("return", "returns"), objects, &rest)]
    }

    /// "Put target card into its owner's graveyard".
    pub(super) fn put_into_graveyard(&mut self, objects: Objects) -> Vec<String> {
        let to = owners(self.is_plural(objects), "graveyard");
        vec![self.acts(("put", "puts"), objects, &format!(" into {to}"))]
    }

    /// "Search your library for a basic land card, reveal it, put it into your hand, then
    /// shuffle" ([CR 701.23](https://yawgatog.com/resources/magic-rules/#R70123)).
    pub(super) fn search(
        &mut self,
        player: Players,
        filter: Filter,
        count: Amount,
        found: Found,
    ) -> Vec<String> {
        let several = !matches!(count, Amount::Fixed(1));
        vec![self.clause(player, ("search", "searches"), |c| {
            let noun = c.noun(filter, Kind::Card(None));
            let (one, many) = (
                noun.singular(Kind::Card(None)),
                noun.plural(Kind::Card(None)),
            );
            let cards = c.count_of(count, &one, &many);
            let library = c.possessive(player);
            let it = if several { "them" } else { "it" };
            let found = match found {
                Found::ToHand => {
                    let that = if several { "those cards" } else { "that card" };
                    format!("{} {that} into {library} hand", c.verb("put", "puts"))
                }
                Found::RevealToHand => format!(
                    "{} {it}, {} {it} into {library} hand",
                    c.verb("reveal", "reveals"),
                    c.verb("put", "puts")
                ),
                Found::OntoBattlefield => {
                    format!("{} {it} onto the battlefield", c.verb("put", "puts"))
                }
                Found::OntoBattlefieldTapped => {
                    format!("{} {it} onto the battlefield tapped", c.verb("put", "puts"))
                }
            };
            let shuffle = c.verb("shuffle", "shuffles");
            format!("{library} library for {cards}, {found}, then {shuffle}")
        })]
    }

    /// "You may reveal a sorcery card you own from outside the game and put it into your
    /// hand."
    pub(super) fn wish(&mut self, filter: Filter) -> Vec<String> {
        self.may = true;
        vec![self.clause(Players::You, ("reveal", "reveals"), |c| {
            let noun = c.noun(filter, Kind::Card(None)).singular(Kind::Card(None));
            format!(
                "{} you own from outside the game and put it into your hand",
                with_article(&noun)
            )
        })]
    }
}
