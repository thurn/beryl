//! Rendering a definition back to Oracle-style rules text, which is compared with
//! Scryfall's Oracle text. A cheap model's typical
//! error is a plausible definition that differs from the card, and the rendered text makes
//! that error mechanical to find. The same templates give the UI the rules text of an
//! ability ([`render_ability`]).
//!
//! Every vocabulary entry has its English template here, written as an exhaustive `match`,
//! so an entry added without one does not compile. The wording follows the rules each entry
//! cites and Oracle's modern templating: an object refers to itself as "this creature" or
//! "this spell", and a legendary one by the first part of its name
//! ([CR 201.5](https://yawgatog.com/resources/magic-rules/#R2015): "Text that refers to the
//! object it's on by name means just that particular object").
//!
//! This module is for tools and the UI only. It allocates `String`s, and nothing the engine
//! runs while it plays, rollouts above all, calls it.
//!
//! - [`words`]: numbers, articles, lists, mana symbols, characteristics.
//! - [`filters`]: what a [`crate::Filter`] or [`crate::TargetSpec`] says as a noun phrase.
//! - [`references`]: objects, players, amounts and conditions.
//! - [`effects`]: one template per [`crate::Effect`] instruction.
//! - [`abilities`]: costs, triggers, static abilities, replacements and whole abilities.
//! - [`card`]: a face's paragraphs, in Oracle's order.

mod abilities;
mod card;
mod clauses;
mod effects;
mod filters;
mod modify;
mod references;
mod words;

#[cfg(test)]
mod tests;

pub use card::{render, render_ability, render_ability_of, render_face};
pub use words::mana_text;

use crate::ability::AbilityKind;
use crate::characteristics::{Subtype, Supertypes, Types};
use crate::definition::Face;
use crate::effect::Players;
use crate::target::TargetSpec;
use crate::trigger::Trigger;

/// What a rendering knows as it writes one ability: whose ability it is, its targets and
/// what the text before has already said, so that a target is "target creature" the first
/// time and "it" after, and a player's things are "their" things in a sentence about them.
#[allow(clippy::struct_excessive_bools)] // Each is one fact about the text written so far.
struct Ctx {
    /// The face the ability is on, for what the ability names by its place on the face: a
    /// delayed ability ([`crate::Effect::Delay`]) or a granted one
    /// ([`crate::Modification::Grant`]). `None` renders those as marked placeholders.
    face: Option<&'static Face>,
    /// How the object refers to itself: "this creature", "Adeline", "Time Spiral".
    this: String,
    /// Whether the object is an Aura or an Equipment, for what it is attached to.
    attached: Attached,
    /// The ability's targets, each instance of the word "target"
    /// ([CR 115.1](https://yawgatog.com/resources/magic-rules/#R1151)).
    targets: &'static [TargetSpec],
    /// Which targets the text has named, so the next mention is "it".
    mentioned: Vec<bool>,
    /// The ability's trigger, for "that card" or "that spell" ([`crate::Objects::TriggeredOn`]).
    trigger: Option<&'static Trigger>,
    /// The player the clause being written is about: their library is "their library".
    subject: Option<Players>,
    /// A "may" waiting for the next clause ([`crate::Effect::May`]).
    may: bool,
    /// Whether the next clause goes on from the one before after ", then", so a subject
    /// other than you is not said again: "each player shuffles ..., then draws seven
    /// cards".
    continued: bool,
    /// The players an [`crate::Effect::ForEachPlayer`] names, when its clauses say them as
    /// their subject: "each player shuffles", not "that player shuffles".
    group: Option<Players>,
    /// Whether the instruction before acted on several objects, so "those" is "them".
    plural: bool,
    /// Whether the object is "it" in the text being written: in the condition of its own
    /// trigger, "when this creature dies, if it isn't a token".
    this_is_it: bool,
    /// Whether the instruction that set "those" discarded them, and whether that was several
    /// cards.
    discarded: Option<bool>,
    /// Set from `discarded` once the text names this object after the discard, so "it" would
    /// mean this object: "those" is then "the discarded card".
    those_named: Option<bool>,
}

/// What "enchanted" or "equipped" names.
#[derive(Clone, Copy)]
enum Attached {
    /// An Aura: "enchanted creature", or "enchanted permanent" when it may enchant more than
    /// creatures ([CR 303.4b](https://yawgatog.com/resources/magic-rules/#R3034b)).
    Aura {
        creature: bool,
    },
    /// An Equipment: "equipped creature"
    /// ([CR 301.5a](https://yawgatog.com/resources/magic-rules/#R3015a)).
    Equipment,
    Neither,
}

impl Ctx {
    /// A rendering of an ability of `face`, named `name`.
    fn of_face(face: &'static Face, name: &str) -> Ctx {
        let this = self_reference(face, name);
        let attached = if face.subtypes.contains(Subtype::Aura) {
            let creature = face
                .spell_ability()
                .and_then(|spell| spell.targets().first())
                .is_some_and(|spec| spec.filter == crate::Filter::Types(Types::CREATURE));
            Attached::Aura { creature }
        } else if face.subtypes.contains(Subtype::Equipment) {
            Attached::Equipment
        } else {
            Attached::Neither
        };
        Ctx {
            face: Some(face),
            this,
            attached,
            ..Ctx::bare(AbilityKind::Static)
        }
    }

    /// A rendering of an ability with no face to read: it refers to itself as "this spell"
    /// or "this permanent".
    fn bare(kind: AbilityKind) -> Ctx {
        Ctx {
            face: None,
            this: match kind {
                AbilityKind::Spell => "this spell".to_owned(),
                _ => "this permanent".to_owned(),
            },
            attached: Attached::Neither,
            targets: &[],
            mentioned: Vec::new(),
            trigger: None,
            subject: None,
            may: false,
            continued: false,
            group: None,
            plural: false,
            this_is_it: false,
            discarded: None,
            those_named: None,
        }
    }

    /// The same object, starting a new ability with these targets and this trigger.
    fn begin(&mut self, targets: &'static [TargetSpec], trigger: Option<&'static Trigger>) {
        self.targets = targets;
        self.mentioned = vec![false; targets.len()];
        self.trigger = trigger;
        self.subject = None;
        self.may = false;
        self.continued = false;
        self.group = None;
        self.plural = false;
        self.this_is_it = false;
        self.discarded = None;
        self.those_named = None;
    }
}

/// How an object refers to itself (CR 201.5): an instant or sorcery and a legendary object
/// by name, the latter by the part before its comma ("Adeline"), and any other permanent as
/// "this" and its card type, or "this permanent" when it has several.
fn self_reference(face: &Face, name: &str) -> String {
    if face.is_instant_or_sorcery() {
        return name.to_owned();
    }
    if face.supertypes.contains(Supertypes::LEGENDARY) {
        return name.split(',').next().unwrap_or(name).to_owned();
    }
    for (subtype, noun) in [
        (Subtype::Aura, "Aura"),
        (Subtype::Equipment, "Equipment"),
        (Subtype::Vehicle, "Vehicle"),
    ] {
        if face.subtypes.contains(subtype) {
            return format!("this {noun}");
        }
    }
    let permanent = face.types.minus(Types::KINDRED);
    if permanent.contains(Types::CREATURE) && !permanent.contains(Types::ENCHANTMENT) {
        return "this creature".to_owned();
    }
    match words::types(permanent).as_slice() {
        [only] => format!("this {only}"),
        _ => "this permanent".to_owned(),
    }
}
