//! The rules text of copying: a token copy of an object, and a copy of a spell.

use super::Ctx;
use super::words::with_article;
use crate::effect::{Amount, CopyChange, Objects, Players};
use crate::index::ExceptIx;

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
                CopyChange::Colors(_) => None,
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
                    CopyChange::Colors(colors) => Some(colors),
                    CopyChange::PowerToughness(..) => None,
                })
            });
        let the_copy = if count == Amount::Fixed(1) {
            "the copy"
        } else {
            "the copies"
        };
        let except = colors.map_or(String::new(), |colors| {
            let words = super::words::colors(colors);
            format!(
                ", except that {the_copy} is {}",
                super::words::list(&words, "and")
            )
        });
        let copy = self.acts(("copy", "copies"), spells, &format!("{times}{except}"));
        let mut clauses = vec![copy];
        if new_targets {
            clauses.push(format!("you may choose new targets for {the_copy}"));
        }
        clauses
    }
}
