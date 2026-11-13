//! The rules text of an ability the UI points at, for where the printed text of the card it
//! shows has no paragraph that says the ability.
//!
//! The UI finds an ability's words in its card's printed text from the card store, as the
//! `nth` paragraph of the ability's kind ([`AbilityRef`]). That works for an ability a card
//! prints, and fails for one it does not: an ability another object grants
//! ([CR 113.2c](https://yawgatog.com/resources/magic-rules/#R1132c): "each paragraph break
//! in a card's text marks a separate ability ... An object may also be granted additional
//! abilities by a spell or ability"), an ability of a token or emblem the card store has no text for, and any ability
//! whose definition has more abilities of its kind than the printed text has paragraphs of it,
//! as the sacrifice trigger of evoke is where Oracle text prints no reminder text.
//! For those the session sends the definition's own text, as `beryl_card::render` writes it.

use beryl_card::render::{render_ability, render_ability_of};
use beryl_card_store::STORE;
use beryl_engine::card::{FaceAbility, FaceIx};
use beryl_engine::{CardId, GameTable};

use super::outcome::{AbilityClass, AbilityRef, ability_ref};

/// The rules text of ability `a` of face `face` of `card`, when the printed text the UI reads
/// for that face has no paragraph for it; none when the UI should find the printed one.
pub(super) fn unprinted(
    t: &GameTable,
    card: impl CardId,
    face: FaceIx,
    a: FaceAbility,
) -> Option<String> {
    let def = t.def(card);
    let ability = def.face(face).ability(a);
    if let FaceAbility::Grant(_) = a {
        // The granting card prints the ability in quotation marks inside its own paragraph,
        // and the object that has it prints nothing for it.
        return Some(render_ability(ability));
    }
    let printed = printed_text(def.face_name(face));
    // A delayed triggered ability is printed inside the paragraph of the ability that makes
    // it ([CR 603.7a](https://yawgatog.com/resources/magic-rules/#R6037a): "Delayed triggered
    // abilities are created during the resolution of spells or abilities"), where the UI
    // finds its words.
    let delayed = ability.trigger().is_some_and(|trigger| trigger.delayed);
    let at = ability_ref(def.face(face), a);
    match printed {
        Some(_) if delayed => None,
        Some(text) if usize::from(at.nth) < paragraphs(&text, at) => None,
        _ => Some(render_ability_of(def, face, ability)),
    }
}

/// The printed rules text of the card face, token or emblem named `name` in the card store.
fn printed_text(name: &str) -> Option<String> {
    let records = STORE.card(name).into_iter().chain(STORE.tokens(name));
    records
        .flat_map(|record| record.faces)
        .find(|face| face.name == name)
        .and_then(|face| face.oracle_text)
}

/// How many paragraphs of `text` say an ability of `at`'s kind, as the UI counts them
/// (`ui/src/game/faces.ts`, `abilityParagraph` and `paragraphOf`).
fn paragraphs(text: &str, at: AbilityRef) -> usize {
    text.split('\n')
        .filter(|&p| match at.kind {
            AbilityClass::Triggered => says_triggered(p),
            AbilityClass::Activated => says_activated(p),
            AbilityClass::Static => {
                (p.ends_with('.') || p.ends_with(".)")) && !says_triggered(p) && !says_activated(p)
            }
        })
        .count()
}

/// Whether paragraph `p` says a triggered ability
/// ([CR 603.1](https://yawgatog.com/resources/magic-rules/#R6031)): "Triggered abilities have
/// a trigger condition and an effect. They are written as '[When/Whenever/At] [trigger
/// condition or event], [effect]. [Instructions (if any).]'". A keyword whose reminder text
/// says one, as offspring's does, counts too.
fn says_triggered(p: &str) -> bool {
    let lead = ["When", "Whenever", "At"]
        .iter()
        .any(|w| p.strip_prefix(w).is_some_and(word_ends));
    let reminder = p.split_once('(').is_some_and(|(_, rest)| {
        ["when", "When", "whenever", "Whenever", "At the beginning"]
            .iter()
            .any(|w| has_word(rest, w))
    });
    lead || reminder
}

/// Whether paragraph `p` says an activated ability
/// ([CR 602.1](https://yawgatog.com/resources/magic-rules/#R6021)): "Activated abilities have
/// a cost and an effect. They are written as '[Cost]: [Effect.] [Activation instructions (if
/// any).]'", a colon outside reminder text.
fn says_activated(p: &str) -> bool {
    p.split('(')
        .next()
        .is_some_and(|before| before.contains(':'))
}

/// Whether `text` holds `word` as a whole word.
fn has_word(text: &str, word: &str) -> bool {
    text.match_indices(word).any(|(i, _)| {
        let starts = text[..i]
            .chars()
            .next_back()
            .is_none_or(|c| !c.is_alphanumeric());
        starts && word_ends(&text[i + word.len()..])
    })
}

/// Whether the text after a word, `rest`, ends that word.
fn word_ends(rest: &str) -> bool {
    rest.chars().next().is_none_or(|c| !c.is_alphanumeric())
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn at(kind: AbilityClass) -> AbilityRef {
        AbilityRef { kind, nth: 0 }
    }

    #[test]
    fn counts_paragraphs_as_the_ui_does() {
        let text = "Flying\nWhen this creature enters, draw a card.\n\
                    {T}: Add {W}.\nOffspring {2} (You may pay an additional {2} as you cast \
                    this spell. If you do, when this creature enters, create a 1/1 token \
                    copy of it.)\nCreatures you control get +1/+1.";
        assert_eq!(paragraphs(text, at(AbilityClass::Triggered)), 2);
        assert_eq!(paragraphs(text, at(AbilityClass::Activated)), 1);
        assert_eq!(paragraphs(text, at(AbilityClass::Static)), 1);
        // "Atop" does not start with the word "At".
        assert_eq!(
            paragraphs("Atop the library.", at(AbilityClass::Triggered)),
            0
        );
    }
}
