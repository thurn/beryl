//! The rules text of copying: a token copy of an object, and a copy of a spell.

use super::Ctx;
use super::abilities::other_types;
use super::filters::Kind;
use super::words::with_article;
use crate::effect::{Amount, CopyChange, Objects, Players};
use crate::filter::Filter;
use crate::index::{ExceptIx, GrantIx};

impl Ctx {
    /// "[Player] creates a token that's a copy of [object]", or "a 1/1 token copy of ...".
    pub(super) fn create_copies(
        &mut self,
        player: Players,
        of: Objects,
        count: Amount,
        except: Option<ExceptIx>,
    ) -> String {
        self.clause(player, ("create", "creates"), |c| {
            let of = c.objects(of);
            let changes = except
                .and_then(|ix| c.face?.excepts.get(ix.index()))
                .map_or(&[][..], |exception| exception.0);
            // The power and toughness the last change sets (CR 707.9b): "The final set of
            // values for that characteristic becomes part of the copiable values".
            let pt = changes.iter().rev().find_map(|change| match *change {
                CopyChange::PowerToughness(p, t) => Some((p, t)),
                CopyChange::Colors(_)
                | CopyChange::AddSubtype(_)
                | CopyChange::WithoutSupertypes(_)
                | CopyChange::Has(_) => None,
            });
            match (pt, count) {
                (Some((p, t)), _) => {
                    format!("{} token copy of {of}", with_article(&format!("{p}/{t}")))
                }
                (None, Amount::Fixed(1)) => format!("a token that's a copy of {of}"),
                (None, count) => {
                    let tokens = c.count_of(count, "token", "tokens");
                    format!("{tokens} that are copies of {of}")
                }
            }
        })
    }

    /// "Copy [spells], except that the copy is red. You may choose new targets for the copy."
    pub(super) fn copy_spell(
        &mut self,
        spells: Objects,
        count: Amount,
        except: Option<ExceptIx>,
        new_targets: bool,
    ) -> Vec<String> {
        let times = match count {
            Amount::Fixed(1) => String::new(),
            Amount::Fixed(2) => " twice".to_owned(),
            Amount::Fixed(n) => format!(" {n} times"),
            other => format!(" for each of {}", self.amount(other)),
        };
        // The colors the last change that sets them sets (CR 707.9b).
        let colors = except
            .and_then(|ix| self.face?.excepts.get(ix.index()))
            .and_then(|exception| {
                exception.0.iter().rev().find_map(|change| match *change {
                    CopyChange::Colors(colors) => Some(colors.read(&self.words)),
                    CopyChange::PowerToughness(..)
                    | CopyChange::AddSubtype(_)
                    | CopyChange::WithoutSupertypes(_)
                    | CopyChange::Has(_) => None,
                })
            });
        // The supertypes the changes take away (CR 707.9b).
        let without = except
            .and_then(|ix| self.face?.excepts.get(ix.index()))
            .into_iter()
            .flat_map(|exception| exception.0)
            .fold(crate::Supertypes::NONE, |all, change| match *change {
                CopyChange::WithoutSupertypes(set) => all.union(set),
                _ => all,
            });
        let the_copy = if count == Amount::Fixed(1) {
            "the copy"
        } else {
            "the copies"
        };
        let mut except = colors.map_or(String::new(), |colors| {
            let words = super::words::colors(colors);
            format!(
                ", except that {the_copy} is {}",
                super::words::list(&words, "and")
            )
        });
        if without.contains(crate::Supertypes::LEGENDARY) {
            except.push_str(", except ");
            except.push_str(if count == Amount::Fixed(1) {
                "it"
            } else {
                "they"
            });
            except.push_str(" isn't legendary if the spell is legendary");
        }
        let copy = self.acts(("copy", "copies"), spells, &format!("{times}{except}"));
        let mut clauses = vec![copy];
        if new_targets {
            clauses.push(format!("you may choose new targets for {the_copy}"));
        }
        clauses
    }

    /// One change a copy exception makes, as "except" says it: "it's 7/7", "it's an Illusion
    /// in addition to its other types", "it has \"...\"" (CR 707.9).
    pub(super) fn copy_change(&mut self, change: CopyChange) -> String {
        match change {
            CopyChange::PowerToughness(p, t) => format!("it's {p}/{t}"),
            CopyChange::AddSubtype(subtype) => format!(
                "it's {} in addition to its other {}",
                with_article(subtype.name()),
                other_types(subtype)
            ),
            CopyChange::WithoutSupertypes(_) => "it isn't legendary".to_owned(),
            CopyChange::Colors(colors) => {
                let words = super::words::colors(colors.read(&self.words));
                format!("it's {}", super::words::list(&words, "and"))
            }
            CopyChange::Has(ix) if self.is_this_ability(ix) => "it has this ability".to_owned(),
            CopyChange::Has(ix) => format!("it has {}", self.granted(ix)),
        }
    }

    /// Whether the ability at `ix` of the face's grants is the ability being written, as
    /// Thespian's Stage's copy keeps the ability that made it one: "except it has this
    /// ability". Abilities are data with no identity, so the two are compared by what they
    /// say.
    fn is_this_ability(&self, ix: GrantIx) -> bool {
        let granted = self.face.and_then(|face| face.grants.get(ix.index()));
        match (granted, self.ability) {
            (Some(granted), Some(this)) => format!("{granted:?}") == format!("{this:?}"),
            _ => false,
        }
    }

    /// "[Objects] become a copy of [of], except ..." (CR 707.2, CR 707.9).
    pub(super) fn become_copy(
        &mut self,
        objects: Objects,
        of: Objects,
        except: Option<ExceptIx>,
    ) -> String {
        let plural = self.is_plural(objects);
        let subject = self.objects(objects);
        let of = self.objects(of);
        let changes = except
            .and_then(|ix| self.face?.excepts.get(ix.index()))
            .map_or(&[][..], |exception| exception.0);
        let excepted: Vec<String> = changes.iter().map(|&c| self.copy_change(c)).collect();
        let except = if excepted.is_empty() {
            String::new()
        } else {
            format!(", except {}", excepted.join(" and "))
        };
        let verb = if plural { "become" } else { "becomes" };
        self.plural = plural;
        format!("{subject} {verb} a copy of {of}{except}")
    }

    /// "You may have this creature enter as a copy of any creature on the battlefield,
    /// except ..." (CR 707.9).
    pub(super) fn enter_as_copy(&mut self, of: Filter, except: Option<ExceptIx>) -> String {
        let this = self.this.clone();
        let noun = self.noun(of, Kind::Permanent).singular(Kind::Permanent);
        let clause =
            format!("you may have {this} enter as a copy of any {noun} on the battlefield");
        let changes = except
            .and_then(|ix| self.face?.excepts.get(ix.index()))
            .map_or(&[][..], |exception| exception.0);
        let excepted: Vec<String> = changes.iter().map(|&c| self.copy_change(c)).collect();
        if excepted.is_empty() {
            clause
        } else {
            format!("{clause}, except {}", excepted.join(" and "))
        }
    }
}
